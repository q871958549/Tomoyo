//! Tomoyo Plugin Protocol v3: Hybrid WebView↔Backend RPC, declarative tray & file actions,
//! conditional settings, and direct WebView launch responses.
//!
//! 100% backward-compatible with v1/v2 manifests (`#[serde(default)]` everywhere).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Optional WebView frontend attached to a STDIO or WASM plugin (Hybrid Plugin Architecture).
/// Enables `window.tomoyo.callPlugin(method, params)` bidirectional RPC to the plugin's own backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginUiConfig {
    /// Relative HTML entrypoint inside the plugin directory (e.g. `"ui/index.html"`).
    pub entry: String,
    #[serde(default)]
    pub utools_compat: bool,
    #[serde(default = "default_ui_width")]
    pub width: u32,
    #[serde(default = "default_ui_height")]
    pub height: u32,
}

fn default_ui_width() -> u32 {
    820
}
fn default_ui_height() -> u32 {
    600
}

/// Declarative system-tray action exposed dynamically only while this plugin is installed & enabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginTrayActionDecl {
    /// Stable action identifier unique within the plugin.
    pub id: String,
    /// Human-readable label shown in the OS system tray menu.
    pub label: String,
    /// Optional launcher query filled when clicked (e.g. `"md "`).
    #[serde(default)]
    pub trigger_query: String,
    /// Optional backend action ID executed via `tomoyo/execute` when `trigger_query` is empty.
    #[serde(default)]
    pub action_id: String,
    /// If true and the plugin declares `[ui]` or `runtime.kind = "webview"`, opens its WebView directly.
    #[serde(default)]
    pub open_ui: bool,
}

/// Declarative file-extension secondary action appended to matching file results (`Tab` menu).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginFileActionDecl {
    /// Stable action identifier (also used as `action_id` for `PluginExecute` when `query_template` is empty).
    pub id: String,
    /// Action label displayed in the file result's `Tab` action menu.
    pub label: String,
    /// File extensions matched case-insensitively without leading dot (e.g. `["md", "markdown"]` or `["*"]`).
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
    /// Optional launcher query template supporting `{path}` placeholder; when empty, dispatches `PluginExecute`.
    #[serde(default)]
    pub query_template: String,
}

/// Conditional display rule for a `[[settings]]` field in Settings Center.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingVisibleIf {
    pub key: String,
    pub equals: Value,
}

/// Request returned by a plugin's `ExecuteResponse` to directly open its own WebView / Hybrid UI
/// without forcing the launcher search box open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenWebViewRequest {
    /// Optional relative entry override; defaults to manifest `[ui].entry` or `runtime.entry`.
    #[serde(default)]
    pub entry: Option<String>,
    /// Optional window title override; defaults to manifest `name`.
    #[serde(default)]
    pub title: Option<String>,
    /// Initial query/context passed to `window.tomoyo.initialQuery` & `onEnter`.
    #[serde(default)]
    pub initial_query: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

/// JSON-RPC `tomoyo/call` request params forwarded from a Hybrid plugin's WebView frontend
/// (`window.tomoyo.callPlugin(method, params)`) to its isolated STDIO/WASM backend.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCallRequest {
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub plugin_settings: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugin_secrets: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use crate::{Permission, PluginManifest, SettingFieldKind};

    #[test]
    fn parses_protocol_v3_hybrid_ui_tray_file_actions_and_rich_settings() {
        let toml_src = r#"
id = "dev.tomoyo.hybrid-demo"
name = "Hybrid Translator"
version = "3.0.0"
permissions = ["network", "selection_read"]

[runtime]
kind = "stdio_jsonrpc"
command = "hybrid-demo"

[ui]
entry = "ui/index.html"
width = 780
height = 620

[triggers]
keywords = ["tr"]
file_extensions = ["md", "txt"]

[[hotkeys]]
id = "sel_tr"
label = "划词翻译"
default_hotkey = ""
capture_selection = true

[[tray_actions]]
id = "open_ui"
label = "打开翻译工作台..."
open_ui = true

[[file_actions]]
id = "open_md"
label = "使用 Hybrid 打开"
extensions = ["md", "markdown"]

[[settings]]
key = "api_key"
label = "API Key"
section = "凭据配置"
type = "secret"
placeholder = "sk-..."

[[settings]]
key = "save_dir"
label = "保存目录"
section = "存储路径"
type = "dir"
placeholder = "/tmp"

[[settings]]
key = "bin_path"
label = "自定义程序"
type = "file"
extensions = ["exe", "sh"]

[[settings]]
key = "quick_key"
label = "二级快捷键"
type = "hotkey"
presets = ["Alt+T", "Ctrl+Shift+T"]
visible_if = { key = "enable_extra", equals = true }
"#;
        let m = PluginManifest::from_toml_str(toml_src).unwrap();
        assert!(m.has_permission(Permission::SelectionRead));
        assert_eq!(m.ui.as_ref().unwrap().entry, "ui/index.html");
        assert!(m.hotkeys[0].capture_selection);
        assert_eq!(m.tray_actions.len(), 1);
        assert!(m.tray_actions[0].open_ui);
        assert_eq!(m.file_actions.len(), 1);
        assert_eq!(m.file_actions[0].extensions, vec!["md", "markdown"]);
        assert_eq!(m.triggers.file_extensions, vec!["md", "txt"]);
        assert!(matches!(m.settings[0].kind, SettingFieldKind::Secret { .. }));
        assert_eq!(m.settings[0].section, "凭据配置");
        assert!(matches!(m.settings[1].kind, SettingFieldKind::Dir { .. }));
        assert!(matches!(m.settings[2].kind, SettingFieldKind::File { .. }));
        assert!(matches!(m.settings[3].kind, SettingFieldKind::Hotkey { .. }));
        assert_eq!(m.settings[3].visible_if.as_ref().unwrap().key, "enable_extra");
    }
}
