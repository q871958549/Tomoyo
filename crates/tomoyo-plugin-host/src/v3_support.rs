//! Protocol v3 runtime helpers: Hybrid WebView↔Plugin RPC, secret injection,
//! dynamic system-tray actions, file-extension actions & direct WebView launch resolution.

use crate::discovery::resolve_plugin_icon_path;
use crate::host::{HostState, PluginHost};
use crate::permissions::is_bundled_plugin_dir;
use crate::process::ProcessError;
use crate::runtime::{ensure_live, ensure_live_stdio, LiveRuntime, RegisteredPlugin};
use crate::v2_support::effective_plugin_settings;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tomoyo_core::{load_plugin_secrets, PermissionStore};
use tomoyo_protocol::{
    ActionKind, ActionSpec, ExecuteRequest, OpenWebViewRequest, Permission, PluginCallRequest,
    PluginManifest, RuntimeConfig, SettingFieldKind, METHOD_CALL,
};

/// Active system-tray entry contributed by an enabled Protocol v3 plugin.
#[derive(Debug, Clone)]
pub struct ActivePluginTrayAction {
    pub plugin_id: String,
    pub plugin_name: String,
    pub id: String,
    pub label: String,
    pub trigger_query: String,
    pub action_id: String,
    pub open_ui: bool,
    pub ui_action: Option<ActionKind>,
}

/// Loads decrypted secrets ONLY for `type = "secret"` fields declared in `manifest.settings`.
pub(crate) fn load_declared_plugin_secrets(manifest: &PluginManifest) -> BTreeMap<String, String> {
    let keys: Vec<String> = manifest
        .settings
        .iter()
        .filter(|f| matches!(f.kind, SettingFieldKind::Secret { .. }))
        .map(|f| f.key.clone())
        .collect();
    if keys.is_empty() {
        return BTreeMap::new();
    }
    load_plugin_secrets(&manifest.id, &keys)
}

/// Resolves an `OpenWebViewRequest` (or plugin's `[ui]` / `RuntimeConfig::Webview`) into an `ActionKind::OpenWebView`.
pub(crate) fn resolve_plugin_webview_open(
    plug: &RegisteredPlugin,
    req: &OpenWebViewRequest,
) -> Option<ActionKind> {
    let (def_entry, def_utools, def_w, def_h) = if let Some(ui) = &plug.manifest.ui {
        (ui.entry.as_str(), ui.utools_compat, ui.width, ui.height)
    } else if let RuntimeConfig::Webview {
        entry,
        utools_compat,
        width,
        height,
    } = &plug.manifest.runtime
    {
        (entry.as_str(), *utools_compat, *width, *height)
    } else {
        ("", false, 820, 600)
    };

    let raw_entry = req
        .entry
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(def_entry)
        .trim();
    if raw_entry.is_empty() || raw_entry.contains("..") {
        return None;
    }
    let entry_path = if raw_entry.starts_with("http://") || raw_entry.starts_with("https://") {
        raw_entry.to_string()
    } else {
        plug.dir.join(raw_entry).to_string_lossy().into_owned()
    };
    let icon_path = resolve_plugin_icon_path(&plug.manifest, &plug.dir);
    let mode = plug.manifest.display_mode.as_str();
    Some(ActionKind::OpenWebView {
        plugin_id: plug.manifest.id.clone(),
        title: req
            .title
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&plug.manifest.name)
            .to_string(),
        entry_path,
        utools_compat: def_utools,
        initial_query: req.initial_query.clone(),
        width: req.width.unwrap_or(def_w).clamp(320, 1920),
        height: req.height.unwrap_or(def_h).clamp(240, 1440),
        max_memory_mb: plug.manifest.resources.max_memory_mb.max(256),
        icon_path,
        display_mode: (mode != "auto").then(|| mode.to_string()),
    })
}

/// Collects up to 16 active system-tray actions declared by enabled Protocol v3 plugins.
pub(crate) fn collect_active_tray_actions(
    plugins: &HashMap<String, RegisteredPlugin>,
    disabled_ids: &HashSet<String>,
) -> Vec<ActivePluginTrayAction> {
    let mut sorted: Vec<&RegisteredPlugin> = plugins.values().collect();
    sorted.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    let mut out = Vec::new();
    for plug in sorted {
        if disabled_ids.contains(&plug.manifest.id.to_ascii_lowercase()) {
            continue;
        }
        for decl in &plug.manifest.tray_actions {
            if decl.id.trim().is_empty() || decl.label.trim().is_empty() {
                continue;
            }
            let ui_action = decl
                .open_ui
                .then(|| resolve_plugin_webview_open(plug, &OpenWebViewRequest::default()))
                .flatten();
            out.push(ActivePluginTrayAction {
                plugin_id: plug.manifest.id.clone(),
                plugin_name: plug.manifest.name.clone(),
                id: decl.id.clone(),
                label: decl.label.clone(),
                trigger_query: decl.trigger_query.clone(),
                action_id: if decl.action_id.trim().is_empty() {
                    decl.id.clone()
                } else {
                    decl.action_id.clone()
                },
                open_ui: decl.open_ui,
                ui_action,
            });
            if out.len() >= 16 {
                return out;
            }
        }
    }
    out
}

/// Collects zero-I/O extension-aware file actions declared via `[[file_actions]]` by enabled plugins.
pub(crate) fn collect_file_actions_for(
    plugins: &HashMap<String, RegisteredPlugin>,
    disabled_ids: &HashSet<String>,
    full_path: &str,
    ext_lower: &str,
) -> Vec<ActionSpec> {
    let mut sorted: Vec<&RegisteredPlugin> = plugins.values().collect();
    sorted.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    let mut out = Vec::new();
    for plug in sorted {
        if disabled_ids.contains(&plug.manifest.id.to_ascii_lowercase()) {
            continue;
        }
        for decl in &plug.manifest.file_actions {
            let matched = decl.extensions.iter().any(|raw| {
                let clean = raw.trim().trim_start_matches('.').to_ascii_lowercase();
                clean == "*" || clean == ext_lower
            });
            if !matched {
                continue;
            }
            let kind = if !decl.query_template.trim().is_empty() {
                ActionKind::FillQuery {
                    query: decl.query_template.replace("{path}", full_path),
                }
            } else {
                ActionKind::PluginExecute {
                    plugin_id: plug.manifest.id.clone(),
                    action_id: decl.id.clone(),
                    payload: serde_json::json!({
                        "path": full_path,
                        "extension": ext_lower,
                    }),
                }
            };
            out.push(ActionSpec {
                id: format!("plug:{}:{}", plug.manifest.id, decl.id),
                label: decl.label.clone(),
                shortcut: decl.shortcut.clone(),
                kind,
            });
        }
    }
    out
}

/// Dispatches a Protocol v3 Hybrid `tomoyo/call` RPC from a plugin's WebView to its own backend.
#[allow(clippy::too_many_arguments)]
pub(crate) fn call_plugin_rpc_live(
    plug: &mut RegisteredPlugin,
    perms: &mut PermissionStore,
    all_settings: &BTreeMap<String, BTreeMap<String, Value>>,
    method: &str,
    params: Value,
    interactive: bool,
) -> Result<(Value, bool), ProcessError> {
    let spawned = ensure_live(plug, perms, interactive)?;
    let plugin_settings = effective_plugin_settings(&plug.manifest, all_settings);
    let plugin_secrets = load_declared_plugin_secrets(&plug.manifest);
    let timeout = Duration::from_millis(plug.manifest.resources.query_timeout_ms.max(4000));

    let res = match plug.live.as_mut() {
        Some(LiveRuntime::Stdio(proc)) => {
            let call_req = PluginCallRequest {
                method: method.to_string(),
                params,
                plugin_settings,
                plugin_secrets,
            };
            let body = serde_json::to_value(&call_req)?;
            proc.call(METHOD_CALL, body, timeout)
        }
        Some(LiveRuntime::Wasm(wasm)) => {
            let exec_req = ExecuteRequest {
                action_id: format!("call:{method}"),
                item_id: format!("rpc:{method}"),
                payload: params,
                plugin_settings,
                plugin_secrets,
            };
            wasm.execute(&exec_req)
                .map_err(|e| ProcessError::Spawn(e.to_string()))
                .and_then(|r| serde_json::to_value(r).map_err(Into::into))
        }
        None => Err(ProcessError::Spawn(format!(
            "plugin `{}` has no STDIO/WASM backend runtime for callPlugin",
            plug.manifest.id
        ))),
    };
    match res {
        Ok(val) => Ok((val, spawned)),
        Err(err) => {
            plug.live = None;
            Err(err)
        }
    }
}

impl PluginHost {
    /// Lists up to 16 active system-tray actions declared by enabled Protocol v3 plugins.
    pub fn active_tray_actions(&self) -> Vec<ActivePluginTrayAction> {
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        collect_active_tray_actions(&st.plugins, &st.disabled_ids)
    }

    /// Checks whether an enabled plugin currently holds `perm` (respecting user revocations).
    pub fn plugin_has_permission(&self, plugin_id: &str, perm: Permission) -> bool {
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        if st.disabled_ids.contains(&plugin_id.to_ascii_lowercase()) {
            return false;
        }
        let Some(plug) = st.plugins.get(plugin_id) else {
            return false;
        };
        if !plug.manifest.has_permission(perm) {
            return false;
        }
        let bundled = is_bundled_plugin_dir(&plug.dir);
        st.perms
            .effective_granted(&plug.manifest.id, &plug.manifest.permissions, bundled)
            .contains(&perm)
    }

    /// Resolves a plugin's `OpenWebViewRequest` into a concrete `ActionKind::OpenWebView`.
    pub fn resolve_webview_open(
        &self,
        plugin_id: &str,
        req: &OpenWebViewRequest,
    ) -> Option<ActionKind> {
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        let plug = st.plugins.get(plugin_id)?;
        resolve_plugin_webview_open(plug, req)
    }

    /// Invokes a Protocol v3 Hybrid `tomoyo/call` RPC on `plugin_id`'s isolated STDIO/WASM backend.
    pub fn call_plugin(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, ProcessError> {
        let interactive = self.interactive_prompts.load(Ordering::Acquire);
        let (lock, cvar) = &*self.state;
        let mut st = lock.lock().unwrap();
        if st.disabled_ids.contains(&plugin_id.to_ascii_lowercase()) {
            return Err(ProcessError::Spawn(format!("plugin `{plugin_id}` is disabled")));
        }
        let HostState { plugins, perms, plugin_settings, .. } = &mut *st;
        let Some(plug) = plugins.get_mut(plugin_id) else {
            return Err(ProcessError::Spawn(format!("plugin `{plugin_id}` not found")));
        };
        let (val, spawned) =
            call_plugin_rpc_live(plug, perms, plugin_settings, method, params, interactive)?;
        if spawned {
            cvar.notify_one();
        }
        Ok(val)
    }

    /// Lightweight STDIO-only Hybrid RPC path that never links `wasmi`, keeping ephemeral
    /// WebView hosts compact (`< 5.5 MB`) while still supporting full bidirectional `callPlugin`.
    pub fn call_plugin_stdio(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, ProcessError> {
        let interactive = self.interactive_prompts.load(Ordering::Acquire);
        let (lock, cvar) = &*self.state;
        let mut st = lock.lock().unwrap();
        if st.disabled_ids.contains(&plugin_id.to_ascii_lowercase()) {
            return Err(ProcessError::Spawn(format!("plugin `{plugin_id}` is disabled")));
        }
        let HostState { plugins, perms, plugin_settings, .. } = &mut *st;
        let Some(plug) = plugins.get_mut(plugin_id) else {
            return Err(ProcessError::Spawn(format!("plugin `{plugin_id}` not found")));
        };
        if !matches!(plug.manifest.runtime, RuntimeConfig::StdioJsonrpc { .. }) {
            return Err(ProcessError::Spawn(format!(
                "plugin `{plugin_id}` requires STDIO backend runtime for WebView callPlugin"
            )));
        }
        let spawned = ensure_live_stdio(plug, perms, interactive)?;
        let plugin_settings = effective_plugin_settings(&plug.manifest, plugin_settings);
        let plugin_secrets = load_declared_plugin_secrets(&plug.manifest);
        let timeout = Duration::from_millis(plug.manifest.resources.query_timeout_ms.max(4000));
        let res = match plug.live.as_mut() {
            Some(LiveRuntime::Stdio(proc)) => {
                let body = serde_json::to_value(PluginCallRequest {
                    method: method.to_string(),
                    params,
                    plugin_settings,
                    plugin_secrets,
                })?;
                proc.call(METHOD_CALL, body, timeout)
            }
            _ => Err(ProcessError::Spawn("STDIO backend failed to start".into())),
        };
        if spawned {
            cvar.notify_one();
        }
        match res {
            Ok(val) => Ok(val),
            Err(err) => {
                plug.live = None;
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn collects_matching_file_actions_and_tray_actions_from_enabled_plugins_only() {
        let m_on = PluginManifest::from_toml_str(
            r#"
id = "dev.tomoyo.md"
name = "MD"
version = "3.0.0"
[runtime]
kind = "stdio_jsonrpc"
command = "md-bin"
[ui]
entry = "ui/index.html"
[[tray_actions]]
id = "open"
label = "Open MD"
open_ui = true
[[file_actions]]
id = "edit_md"
label = "Edit in MD"
extensions = [".md", "markdown"]
"#,
        )
        .unwrap();
        let m_off = PluginManifest::from_toml_str(
            r#"
id = "dev.tomoyo.disabled"
name = "Off"
version = "1.0.0"
[runtime]
kind = "stdio_jsonrpc"
command = "off-bin"
[[file_actions]]
id = "noop"
label = "Disabled Action"
extensions = ["*"]
"#,
        )
        .unwrap();

        let mut plugins = HashMap::new();
        plugins.insert(
            m_on.id.clone(),
            RegisteredPlugin { manifest: m_on, dir: PathBuf::from("/tmp/md"), live: None },
        );
        plugins.insert(
            m_off.id.clone(),
            RegisteredPlugin { manifest: m_off, dir: PathBuf::from("/tmp/off"), live: None },
        );
        let disabled: HashSet<String> = ["dev.tomoyo.disabled".into()].into_iter().collect();

        let tray = collect_active_tray_actions(&plugins, &disabled);
        assert_eq!(tray.len(), 1);
        assert!(matches!(tray[0].ui_action, Some(ActionKind::OpenWebView { .. })));

        let md_acts = collect_file_actions_for(&plugins, &disabled, "/docs/readme.md", "md");
        assert_eq!(md_acts.len(), 1);
        assert_eq!(md_acts[0].label, "Edit in MD");
        assert!(collect_file_actions_for(&plugins, &disabled, "/docs/pic.png", "png").is_empty());
    }
}
