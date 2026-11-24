//! Protocol v3 Hybrid WebView ↔ Plugin Backend RPC bridge, generic plugin UI launcher,
//! and encrypted OS credential vault helpers (`plugin:<id>:<key>`).

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use tomoyo_core::{delete_secret, has_secret, plugin_secret_key, set_secret, TomoyoConfig};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};
use tomoyo_protocol::{ActionKind, OpenWebViewRequest, SettingFieldKind};

static RPC_HOST: OnceLock<Arc<PluginHost>> = OnceLock::new();

fn rpc_host() -> &'static Arc<PluginHost> {
    RPC_HOST.get_or_init(|| {
        let host = PluginHost::new(default_plugin_dirs());
        host.set_interactive_prompts(false);
        host
    })
}

/// Executes `window.tomoyo.callPlugin(method, params)` strictly scoped to the WebView's own `plugin_id`.
/// Returns an envelope without an `"event"` field so it bypasses Settings `HostEvent` parity
/// while routing cleanly through WebView2 / WebKitGTK `message` listeners.
pub fn execute_plugin_rpc(plugin_id: &str, req_id: &str, method: &str, params: Value) -> String {
    if plugin_id.trim().is_empty() || plugin_id == "dev.tomoyo.webview" {
        return json!({
            "__tomoyo_plugin_rpc__": true,
            "req_id": req_id,
            "ok": false,
            "error": "No plugin_id bound to this WebView instance",
        })
        .to_string();
    }
    match rpc_host().call_plugin_stdio(plugin_id, method, params) {
        Ok(result) => json!({
            "__tomoyo_plugin_rpc__": true,
            "req_id": req_id,
            "ok": true,
            "result": result,
        })
        .to_string(),
        Err(err) => json!({
            "__tomoyo_plugin_rpc__": true,
            "req_id": req_id,
            "ok": false,
            "error": err.to_string(),
        })
        .to_string(),
    }
}

/// Shuts down any backend plugin spawned via `callPlugin` when the WebView window exits.
pub fn shutdown_rpc_host() {
    if let Some(host) = RPC_HOST.get() {
        host.shutdown_all();
    }
}

/// Launches any installed Protocol v3 WebView / Hybrid plugin UI cleanly by `plugin_id`.
pub fn open_plugin_ui(plugin_id: &str, query: &str, notify_hwnd: Option<isize>) -> bool {
    let pid = plugin_id.trim();
    if pid.is_empty() {
        return false;
    }
    let host = PluginHost::new(default_plugin_dirs());
    let req = OpenWebViewRequest {
        entry: None,
        title: None,
        initial_query: query.to_string(),
        width: None,
        height: None,
    };
    let resolved = host.resolve_webview_open(pid, &req);
    host.shutdown_all();
    let Some(ActionKind::OpenWebView {
        plugin_id,
        title,
        entry_path,
        utools_compat,
        initial_query,
        width,
        height,
        icon_path,
        ..
    }) = resolved
    else {
        tracing::warn!(plugin_id = %pid, "Requested plugin UI is not installed or has no [ui] entry");
        return false;
    };
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let theme = TomoyoConfig::load_or_create().theme;
    let mut cmd = std::process::Command::new(exe);
    cmd.args([
        "--plugin-id",
        &plugin_id,
        "--title",
        &title,
        "--entry",
        &entry_path,
        "--query",
        &initial_query,
        "--theme",
        &theme,
        "--width",
        &width.to_string(),
        "--height",
        &height.to_string(),
    ]);
    if utools_compat {
        cmd.arg("--utools-compat");
    }
    if theme.eq_ignore_ascii_case("light") {
        cmd.arg("--light");
    }
    if let Some(icon) = icon_path.as_deref().filter(|s| !s.is_empty()) {
        cmd.args(["--icon", icon]);
    }
    if let Some(h) = notify_hwnd {
        cmd.args(["--notify-hwnd", &h.to_string()]);
    }
    cmd.spawn().is_ok()
}

/// Collects encrypted configured-state booleans (`"plugin:<id>:<key>" -> bool`) for all installed
/// plugins' declarative `type = "secret"` fields without ever exposing plaintext values.
pub fn collect_plugin_secrets_status() -> BTreeMap<String, bool> {
    let host = PluginHost::new(default_plugin_dirs());
    let mut map = BTreeMap::new();
    for p in host.list_plugins() {
        let is_tr = p.id.eq_ignore_ascii_case(tomoyo_core::PLUGIN_TRANSLATE);
        for field in &p.settings_schema {
            if matches!(field.kind, SettingFieldKind::Secret { .. }) {
                let full_key = plugin_secret_key(&p.id, &field.key);
                let saved = has_secret(&full_key) || (is_tr && has_secret(&field.key));
                map.insert(full_key, saved);
            }
        }
    }
    host.shutdown_all();
    map
}

pub fn secrets_status_json() -> Value {
    json!({
        "plugins": collect_plugin_secrets_status(),
    })
}

pub fn handle_set_secret(key: &str, value: &str) -> String {
    match set_secret(key, value) {
        Ok(()) => json!({
            "event": "secrets_status",
            "ok": true,
            "message": if cfg!(windows) {
                "API 密钥已通过 Windows DPAPI 加密保存（不写入明文配置）"
            } else {
                "API 密钥已通过系统密钥环 / 本地加密保存（不写入明文配置）"
            },
            "status": secrets_status_json(),
        })
        .to_string(),
        Err(e) => json!({
            "event": "secrets_status",
            "ok": false,
            "message": e,
            "status": secrets_status_json(),
        })
        .to_string(),
    }
}

pub fn handle_delete_secret(key: &str) -> String {
    match delete_secret(key) {
        Ok(()) => json!({
            "event": "secrets_status",
            "ok": true,
            "message": "已清除保存的 API 密钥",
            "status": secrets_status_json(),
        })
        .to_string(),
        Err(e) => json!({
            "event": "secrets_status",
            "ok": false,
            "message": e,
            "status": secrets_status_json(),
        })
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unbound_plugin_id_with_structured_rpc_envelope() {
        let raw = execute_plugin_rpc("", "rpc_1", "ping", Value::Null);
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["__tomoyo_plugin_rpc__"], Value::Bool(true));
        assert_eq!(v["req_id"], "rpc_1");
        assert_eq!(v["ok"], Value::Bool(false));
        assert!(v.get("event").is_none());
    }
}
