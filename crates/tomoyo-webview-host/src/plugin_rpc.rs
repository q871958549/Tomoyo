//! Protocol v3 Hybrid WebView ↔ Plugin Backend RPC bridge & plugin secrets status helper.
//!
//! Lazily instantiates a process-local `PluginHost` ONLY when a Hybrid WebView calls
//! `window.tomoyo.callPlugin(method, params)`, and shuts it down cleanly on window close
//! so non-hybrid WebViews pay 0 bytes of overhead and child processes never leak.

use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use tomoyo_core::{has_secret, plugin_secret_key};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};
use tomoyo_protocol::SettingFieldKind;

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
        return serde_json::json!({
            "__tomoyo_plugin_rpc__": true,
            "req_id": req_id,
            "ok": false,
            "error": "No plugin_id bound to this WebView instance",
        })
        .to_string();
    }
    match rpc_host().call_plugin_stdio(plugin_id, method, params) {
        Ok(result) => serde_json::json!({
            "__tomoyo_plugin_rpc__": true,
            "req_id": req_id,
            "ok": true,
            "result": result,
        })
        .to_string(),
        Err(err) => serde_json::json!({
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

/// Collects encrypted configured-state booleans (`"plugin:<id>:<key>" -> bool`) for all installed
/// plugins' declarative `type = "secret"` fields without ever exposing plaintext values.
pub fn collect_plugin_secrets_status() -> BTreeMap<String, bool> {
    let host = PluginHost::new(default_plugin_dirs());
    let mut map = BTreeMap::new();
    for p in host.list_plugins() {
        for field in &p.settings_schema {
            if matches!(field.kind, SettingFieldKind::Secret { .. }) {
                let full_key = plugin_secret_key(&p.id, &field.key);
                map.insert(full_key.clone(), has_secret(&full_key));
            }
        }
    }
    host.shutdown_all();
    map
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
