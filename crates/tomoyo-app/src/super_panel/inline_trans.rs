//! Non-blocking lock-free inline translation worker for Tomoyo Super Panel.
//!
//! Runs `tomoyo_core::translate::translate_parallel` directly on a short-lived worker thread
//! using the user's effective `dev.tomoyo.translate` plugin settings & DPAPI secrets, so it
//! NEVER locks `PluginHost::state` or contends with `tomoyo-plugin-translate` STDIO execution.

use super::InlineTransState;
use serde_json::Value;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use tomoyo_core::translate::{
    translate_parallel, TranslateBatchResponse, TranslateConfig, ALLOWED_TARGET_LANGS,
};
use tomoyo_core::{is_safe_http_url, Lang, TomoyoConfig, PLUGIN_TRANSLATE};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub struct TransOutcome {
    pub seq: u64,
    pub state: InlineTransState,
}

/// Resolves the effective `TranslateConfig` by overlaying `config.plugin_settings["dev.tomoyo.translate"]`
/// onto `config.translate`, matching `tomoyo-plugin-translate` identically.
pub fn effective_translate_config(cfg: &TomoyoConfig) -> TranslateConfig {
    let mut out = cfg.translate.clone();
    let Some(map) = cfg.plugin_settings.get(PLUGIN_TRANSLATE) else {
        return out;
    };
    if let Some(t) = map.get("target_lang").and_then(Value::as_str)
        && ALLOWED_TARGET_LANGS.contains(&t)
    {
        out.target_lang = t.to_string();
    }
    let has_engine_flags = [
        "engine_google",
        "engine_mymemory",
        "engine_deepl",
        "engine_openai",
        "engine_ollama",
    ]
    .iter()
    .any(|k| map.contains_key(*k));
    if has_engine_flags {
        let mut engines = Vec::new();
        let flag = |k: &str, def: bool| map.get(k).and_then(Value::as_bool).unwrap_or(def);
        if flag("engine_google", true) {
            engines.push("google".into());
        }
        if flag("engine_mymemory", true) {
            engines.push("mymemory".into());
        }
        if flag("engine_deepl", false) {
            engines.push("deepl".into());
        }
        if flag("engine_openai", false) {
            engines.push("openai".into());
        }
        if flag("engine_ollama", false) {
            engines.push("ollama".into());
        }
        if !engines.is_empty() {
            out.engines = engines;
        }
    }
    if let Some(u) = map.get("openai_base_url").and_then(Value::as_str)
        && is_safe_http_url(u)
    {
        out.openai_base_url = u.trim().to_string();
    }
    if let Some(m) = map.get("openai_model").and_then(Value::as_str) {
        let clean = m.trim();
        if !clean.is_empty() {
            out.openai_model = clean.to_string();
        }
    }
    if let Some(u) = map.get("ollama_url").and_then(Value::as_str)
        && is_safe_http_url(u)
    {
        out.ollama_url = u.trim().to_string();
    }
    if let Some(m) = map.get("ollama_model").and_then(Value::as_str) {
        let clean = m.trim();
        if !clean.is_empty() {
            out.ollama_model = clean.to_string();
        }
    }
    out
}

pub fn spawn_inline_translate(
    text: String,
    tr_cfg: TranslateConfig,
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
            let batch = translate_parallel(&tr_cfg, &text, "auto", "");
            let outcome = parse_translate_batch(&batch, lang);
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

pub fn parse_translate_batch(batch: &TranslateBatchResponse, lang: Lang) -> InlineTransState {
    if let Some(hit) = batch
        .results
        .iter()
        .find(|r| r.ok && !r.translated.trim().is_empty())
    {
        return InlineTransState::Ready {
            translated: hit.translated.trim().to_string(),
            engine_name: hit.engine_name.clone(),
            phonetic: hit
                .phonetic
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string),
        };
    }
    if let Some(err) = batch.results.iter().find_map(|r| r.error.as_deref()) {
        return InlineTransState::Error { message: err.to_string() };
    }
    InlineTransState::Error {
        message: lang
            .pick("所有翻译引擎暂未返回结果", "No translation engine returned a result")
            .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomoyo_core::translate::EngineTranslation;

    #[test]
    fn parses_first_successful_engine_translation_with_phonetic() {
        let batch = TranslateBatchResponse {
            trace_id: "TRN-1".into(),
            source_text: "hello".into(),
            target_lang: "zh-Hans".into(),
            results: vec![
                EngineTranslation {
                    engine_id: "deepl".into(),
                    engine_name: "DeepL".into(),
                    ok: false,
                    translated: String::new(),
                    detected_lang: None,
                    phonetic: None,
                    extra_dict: None,
                    error: Some("missing key".into()),
                    elapsed_ms: 5,
                },
                EngineTranslation {
                    engine_id: "google".into(),
                    engine_name: "Google".into(),
                    ok: true,
                    translated: "你好，世界".into(),
                    detected_lang: Some("en".into()),
                    phonetic: Some("həˈloʊ".into()),
                    extra_dict: None,
                    error: None,
                    elapsed_ms: 80,
                },
            ],
        };
        match parse_translate_batch(&batch, Lang::Zh) {
            InlineTransState::Ready { translated, engine_name, phonetic } => {
                assert_eq!(translated, "你好，世界");
                assert_eq!(engine_name, "Google");
                assert_eq!(phonetic.as_deref(), Some("həˈloʊ"));
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }
}
