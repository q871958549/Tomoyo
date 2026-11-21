//! Protocol v2 runtime helpers: effective plugin settings merge, declared hotkey resolution & event dispatch.

use crate::host::PluginRuntimeInfo;
use crate::permissions::is_bundled_plugin_dir;
use crate::runtime::{ensure_live, LiveRuntime, RegisteredPlugin};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;
use tomoyo_core::{permission_key, PermissionStore, TomoyoConfig};
use tomoyo_protocol::{
    ExecuteResponse, Permission, PluginEventKind, PluginEventPayload, PluginManifest, METHOD_EVENT,
};

/// Builds a serializable runtime snapshot of a discovered plugin including Protocol v2 metadata.
pub(crate) fn build_runtime_info(p: &RegisteredPlugin, perms: &PermissionStore) -> PluginRuntimeInfo {
    let bundled = is_bundled_plugin_dir(&p.dir);
    let granted: Vec<String> = perms
        .effective_granted(&p.manifest.id, &p.manifest.permissions, bundled)
        .into_iter()
        .map(|perm| permission_key(perm).to_string())
        .collect();
    let declared: Vec<String> = p
        .manifest
        .permissions
        .iter()
        .map(|perm| permission_key(*perm).to_string())
        .collect();
    PluginRuntimeInfo {
        id: p.manifest.id.clone(),
        name: p.manifest.name.clone(),
        version: p.manifest.version.clone(),
        description: p.manifest.description.clone(),
        runtime_badge: p.manifest.runtime.kind_badge().to_string(),
        keywords: p.manifest.triggers.keywords.clone(),
        running: p.live.is_some(),
        pid: p.live.as_ref().and_then(|l| l.pid()),
        working_set_bytes: p.live.as_ref().and_then(|l| l.memory_bytes()),
        max_memory_mb: p.manifest.resources.max_memory_mb,
        idle_timeout_secs: p.manifest.resources.idle_timeout_secs,
        declared_permissions: declared,
        granted_permissions: granted,
        bundled_trusted: bundled,
        declared_hotkeys: p.manifest.hotkeys.clone(),
        declared_events: p.manifest.events.clone(),
        settings_schema: p.manifest.settings.clone(),
    }
}

/// Resolved active hotkey binding declared by an enabled Protocol v2 plugin.
#[derive(Debug, Clone)]
pub struct ActivePluginHotkey {
    pub plugin_id: String,
    pub hotkey_id: String,
    pub label: String,
    pub spec: String,
    pub trigger_query: String,
}

/// Merges manifest `[[settings]]` default values with user overrides in `config.toml`.
pub(crate) fn effective_plugin_settings(
    manifest: &PluginManifest,
    all_overrides: &BTreeMap<String, BTreeMap<String, Value>>,
) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for field in &manifest.settings {
        if !field.default.is_null() {
            out.insert(field.key.clone(), field.default.clone());
        }
    }
    let key_lower = manifest.id.to_ascii_lowercase();
    if let Some(ov) = all_overrides.get(&key_lower).or_else(|| all_overrides.get(&manifest.id)) {
        for (k, v) in ov {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Collects up to 16 active global hotkeys declared by enabled Protocol v2 plugins.
pub(crate) fn collect_active_plugin_hotkeys(
    plugins: &HashMap<String, RegisteredPlugin>,
    disabled_ids: &HashSet<String>,
    cfg: &TomoyoConfig,
) -> Vec<ActivePluginHotkey> {
    let mut out = Vec::new();
    let mut sorted: Vec<&RegisteredPlugin> = plugins.values().collect();
    sorted.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    for plug in sorted {
        let pid_low = plug.manifest.id.to_ascii_lowercase();
        if disabled_ids.contains(&pid_low) {
            continue;
        }
        for decl in &plug.manifest.hotkeys {
            let slot_key = format!("{pid_low}:{}", decl.id.to_ascii_lowercase());
            let spec = cfg
                .plugin_hotkeys
                .get(&slot_key)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| decl.default_hotkey.trim().to_string());
            if spec.is_empty() {
                continue;
            }
            out.push(ActivePluginHotkey {
                plugin_id: plug.manifest.id.clone(),
                hotkey_id: decl.id.clone(),
                label: decl.label.clone(),
                spec,
                trigger_query: decl.trigger_query.clone(),
            });
            if out.len() >= 16 {
                return out;
            }
        }
    }
    out
}

/// Dispatches a Protocol v2 `tomoyo/event` to enabled subscribed STDIO plugins that hold permissions.
pub(crate) fn dispatch_plugin_event(
    plugins: &mut HashMap<String, RegisteredPlugin>,
    disabled_ids: &HashSet<String>,
    perms: &mut PermissionStore,
    target_plugin: Option<&str>,
    payload: &PluginEventPayload,
    interactive: bool,
) -> (Vec<ExecuteResponse>, bool) {
    let mut responses = Vec::new();
    let mut any_spawned = false;
    let needs_clip = matches!(
        payload.event,
        PluginEventKind::ClipboardText | PluginEventKind::ClipboardImage
    );

    for (id, plug) in plugins.iter_mut() {
        if let Some(target) = target_plugin
            && !id.eq_ignore_ascii_case(target) {
                continue;
            }
        if disabled_ids.contains(&id.to_ascii_lowercase()) {
            continue;
        }
        if !plug.manifest.events.contains(&payload.event) {
            continue;
        }
        if needs_clip {
            let bundled = is_bundled_plugin_dir(&plug.dir);
            let granted = perms.effective_granted(&plug.manifest.id, &plug.manifest.permissions, bundled);
            if !granted.contains(&Permission::Clipboard) {
                continue;
            }
        }
        match ensure_live(plug, perms, interactive) {
            Ok(spawned) => {
                if spawned {
                    any_spawned = true;
                }
            }
            Err(_) => continue,
        }
        if let Some(LiveRuntime::Stdio(proc)) = plug.live.as_mut()
            && let Ok(params) = serde_json::to_value(payload) {
                let timeout = Duration::from_millis(plug.manifest.resources.query_timeout_ms.min(1500));
                if let Ok(val) = proc.call(METHOD_EVENT, params, timeout)
                    && let Ok(resp) = serde_json::from_value::<ExecuteResponse>(val) {
                        responses.push(resp);
                    }
            }
    }
    (responses, any_spawned)
}
