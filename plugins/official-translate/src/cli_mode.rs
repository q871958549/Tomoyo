use crate::engines::translate_parallel;
use crate::store::resolve_runtime_config;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;

pub(crate) fn run_quick_translate_cli() {
    let mut raw = String::new();
    let _ = std::io::stdin().take(64 * 1024).read_to_string(&mut raw);
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        println!("{}", json!({ "results": [] }));
        return;
    };
    let text = val.get("text").and_then(Value::as_str).unwrap_or("");
    let src = val.get("source_lang").and_then(Value::as_str).unwrap_or("auto");
    let tgt = val.get("target_lang").and_then(Value::as_str).unwrap_or("");
    let settings: BTreeMap<String, Value> = val
        .get("plugin_settings")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let secrets: BTreeMap<String, String> = val
        .get("plugin_secrets")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let cfg = resolve_runtime_config(&settings);
    let (batch, _) = translate_parallel(&cfg, &secrets, text, src, tgt, "standard");
    println!("{}", serde_json::to_string(&batch).unwrap_or_else(|_| "{}".into()));
}

#[cfg(test)]
mod tests {
    use crate::{parse_translate_args, TranslatePlugin};
    use serde_json::json;
    use tomoyo_sdk::{PluginEventKind, PluginEventPayload, TomoyoPlugin};

    #[test]
    fn parses_prefix_and_handles_selection_hotkey_event() {
        assert_eq!(parse_translate_args("ja hello"), (Some("ja".into()), "hello".into()));
        let resp = TranslatePlugin.on_event(&PluginEventPayload::new(
            PluginEventKind::HotkeyTriggered,
            json!({ "hotkey_id": "selection_translate", "selected_text": "resilient" }),
        ));
        assert!(resp.hide_window);
        assert!(resp.open_webview.is_some());
    }
}
