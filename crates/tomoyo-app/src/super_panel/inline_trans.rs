//! Non-blocking inline translation worker calling `dev.tomoyo.translate` Protocol v3 RPC.

use super::InlineTransState;
use serde_json::Value;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use tomoyo_core::{Lang, PLUGIN_TRANSLATE};
use tomoyo_plugin_host::PluginHost;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub struct TransOutcome {
    pub seq: u64,
    pub state: InlineTransState,
}

pub fn spawn_inline_translate(
    text: String,
    host: Arc<PluginHost>,
    slot: Arc<Mutex<Option<TransOutcome>>>,
    hwnd: HWND,
    ready_msg: u32,
    seq: u64,
    lang: Lang,
) {
    let hwnd_raw = hwnd.0 as usize;
    let _ = std::thread::Builder::new()
        .name("tomoyo-sp-trans".into())
        .spawn(move || {
            let outcome = match host.call_plugin(
                PLUGIN_TRANSLATE,
                "translate",
                serde_json::json!({ "text": text, "source_lang": "auto", "target_lang": "" }),
            ) {
                Ok(val) => parse_translate_rpc_value(&val, lang),
                Err(e) => InlineTransState::Error {
                    message: format!("{e}"),
                },
            };
            if let Ok(mut g) = slot.lock() {
                *g = Some(TransOutcome { seq, state: outcome });
            }
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd_raw as *mut c_void)),
                    ready_msg,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        });
}

pub fn parse_translate_rpc_value(val: &Value, lang: Lang) -> InlineTransState {
    let data = val.get("data").unwrap_or(val);
    let target_lang = data
        .get("target_lang")
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_string();
    let results = data.get("results").and_then(Value::as_array);
    if let Some(arr) = results {
        if let Some(hit) = arr.iter().find(|r| {
            r.get("ok").and_then(Value::as_bool).unwrap_or(false)
                && !r.get("translated").and_then(Value::as_str).unwrap_or("").trim().is_empty()
        }) {
            let translated = hit.get("translated").and_then(Value::as_str).unwrap_or("").trim().to_string();
            let engine_name = hit.get("engine_name").and_then(Value::as_str).unwrap_or("Translator").to_string();
            let phonetic = hit
                .get("phonetic")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string);
            return InlineTransState::Ready {
                translated,
                engine_name,
                phonetic,
                target_lang,
            };
        }
        if let Some(err_msg) = arr.iter().find_map(|r| r.get("error").and_then(Value::as_str)) {
            return InlineTransState::Error {
                message: err_msg.to_string(),
            };
        }
    }
    InlineTransState::Error {
        message: lang.pick("所有翻译引擎暂未返回结果", "No translation engine returned a result").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_first_successful_engine_translation_with_phonetic() {
        let payload = serde_json::json!({
            "data": {
                "target_lang": "zh-Hans",
                "results": [
                    { "engine_name": "DeepL", "ok": false, "translated": "", "error": "missing key" },
                    { "engine_name": "Google", "ok": true, "translated": "你好，世界", "phonetic": "həˈloʊ" }
                ]
            }
        });
        match parse_translate_rpc_value(&payload, Lang::Zh) {
            InlineTransState::Ready { translated, engine_name, phonetic, target_lang, .. } => {
                assert_eq!(translated, "你好，世界");
                assert_eq!(engine_name, "Google");
                assert_eq!(phonetic.as_deref(), Some("həˈloʊ"));
                assert_eq!(target_lang, "zh-Hans");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }
}
