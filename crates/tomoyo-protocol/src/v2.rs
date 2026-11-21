//! Tomoyo Plugin Protocol v2: declarative global hotkeys, event subscriptions & typed settings schema.
//!
//! 100% backward-compatible with v1 manifests (`#[serde(default)]` everywhere).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Declarative global hotkey registered & governed centrally by Tomoyo's microkernel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHotkeyDecl {
    /// Stable slot ID unique within the plugin (e.g. `"quick_note"`).
    pub id: String,
    /// Human-readable label shown in Settings Center & Hotkey Doctor.
    pub label: String,
    /// Optional factory default shortcut (keep `""` for opt-in unbound).
    #[serde(default)]
    pub default_hotkey: String,
    /// Optional launcher query filled when pressed (e.g. `"note "`).
    /// When empty, dispatches `tomoyo/event` (`hotkey_triggered`) directly to the plugin.
    #[serde(default)]
    pub trigger_query: String,
}

/// Event kinds a plugin may subscribe to via `events = [...]` in `tomoyo-plugin.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginEventKind {
    ClipboardText,
    ClipboardImage,
    HotkeyTriggered,
}

/// Payload delivered over JSON-RPC `tomoyo/event` to subscribed plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEventPayload {
    pub event: PluginEventKind,
    #[serde(default)]
    pub data: Value,
}

/// Select dropdown option inside a declarative `PluginSettingField`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingOption {
    pub value: String,
    pub label: String,
}

/// Controlled UI widget type for declarative plugin settings (rendered automatically by Settings Center).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettingFieldKind {
    Text {
        #[serde(default)]
        placeholder: String,
    },
    Bool,
    Number {
        #[serde(default)]
        min: i64,
        #[serde(default = "default_num_max")]
        max: i64,
        #[serde(default = "default_num_step")]
        step: i64,
    },
    Select {
        #[serde(default)]
        options: Vec<SettingOption>,
    },
}

fn default_num_max() -> i64 {
    100
}
fn default_num_step() -> i64 {
    1
}

/// Single declarative setting entry defined via `[[settings]]` in `tomoyo-plugin.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginSettingField {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    pub kind: SettingFieldKind,
    #[serde(default)]
    pub default: Value,
}

#[cfg(test)]
mod tests {
    use crate::PluginManifest;

    #[test]
    fn parses_protocol_v2_hotkeys_events_and_settings_schema() {
        let toml_src = r#"
id = "dev.tomoyo.notes"
name = "Local Notes"
version = "2.0.0"
events = ["hotkey_triggered"]

[runtime]
kind = "stdio_jsonrpc"
command = "tomoyo-plugin-notes"

[triggers]
keywords = ["note"]

[[hotkeys]]
id = "quick_note"
label = "快速记笔记"
default_hotkey = ""
trigger_query = "note "

[[settings]]
key = "max_results"
label = "最多显示条数"
description = "单次检索上限"
type = "number"
min = 5
max = 30
step = 5
default = 15

[[settings]]
key = "mode"
label = "排序方式"
type = "select"
default = "recent"
options = [
  { value = "recent", label = "最近修改" },
  { value = "title", label = "标题排序" }
]
"#;
        let m = PluginManifest::from_toml_str(toml_src).unwrap();
        assert_eq!(m.hotkeys.len(), 1);
        assert_eq!(m.hotkeys[0].id, "quick_note");
        assert_eq!(m.events.len(), 1);
        assert_eq!(m.settings.len(), 2);
        assert_eq!(m.settings[0].key, "max_results");
        assert_eq!(m.settings[1].key, "mode");
    }
}
