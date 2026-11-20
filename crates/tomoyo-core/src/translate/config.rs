//! Translation subsystem configuration schema & validation helpers.

use crate::config::{is_safe_http_url, parse_hotkey};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const ALL_TRANSLATE_ENGINES: &[&str] = &["google", "mymemory", "deepl", "openai", "ollama"];
pub const ALLOWED_TARGET_LANGS: &[&str] = &["auto", "zh-Hans", "en", "ja", "ko", "fr", "de"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateConfig {
    /// Dedicated global shortcut for selection-translate (default `"Alt+T"`, empty `""` disables).
    #[serde(default = "default_translate_hotkey")]
    pub hotkey: String,
    /// Default target language (`"auto"` = Zh <-> En automatic toggle).
    #[serde(default = "default_target_lang")]
    pub target_lang: String,
    /// Enabled engines queried in parallel (`bing`, `mymemory`, `deepl`, `openai`, `ollama`).
    #[serde(default = "default_engines")]
    pub engines: Vec<String>,
    /// OpenAI-compatible API base URL (e.g. `https://api.openai.com/v1` or DeepSeek endpoint).
    #[serde(default = "default_openai_url")]
    pub openai_base_url: String,
    /// Model name for OpenAI-compatible translation.
    #[serde(default = "default_openai_model")]
    pub openai_model: String,
    /// Local Ollama HTTP endpoint (100% offline LLM translation).
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    /// Local Ollama model name (e.g. `qwen2.5:7b`).
    #[serde(default = "default_ollama_model")]
    pub ollama_model: String,
}

fn default_translate_hotkey() -> String {
    "Alt+T".into()
}
fn default_target_lang() -> String {
    "auto".into()
}
fn default_engines() -> Vec<String> {
    vec!["google".into(), "mymemory".into()]
}
fn default_openai_url() -> String {
    "https://api.openai.com/v1".into()
}
fn default_openai_model() -> String {
    "gpt-4o-mini".into()
}
fn default_ollama_url() -> String {
    "http://127.0.0.1:11434".into()
}
fn default_ollama_model() -> String {
    "qwen2.5:7b".into()
}

impl Default for TranslateConfig {
    fn default() -> Self {
        Self {
            hotkey: default_translate_hotkey(),
            target_lang: default_target_lang(),
            engines: default_engines(),
            openai_base_url: default_openai_url(),
            openai_model: default_openai_model(),
            ollama_url: default_ollama_url(),
            ollama_model: default_ollama_model(),
        }
    }
}

/// Sanitizes `TranslateConfig` against colliding hotkeys, unknown engine IDs, and unsafe endpoints.
pub fn sanitize_translate_config(
    tr: &mut TranslateConfig,
    main_hotkey: &str,
    clip_hotkey: &str,
    notes: &mut Vec<String>,
) {
    let hk = tr.hotkey.trim().to_string();
    if hk.is_empty() {
        tr.hotkey.clear();
    } else if parse_hotkey(&hk).is_none()
        || hk.eq_ignore_ascii_case(main_hotkey)
        || (!clip_hotkey.is_empty() && hk.eq_ignore_ascii_case(clip_hotkey))
    {
        let fallback = if !main_hotkey.eq_ignore_ascii_case("Alt+T")
            && !clip_hotkey.eq_ignore_ascii_case("Alt+T")
        {
            "Alt+T".into()
        } else {
            String::new()
        };
        notes.push(format!("翻译快捷键 `{hk}` 无效或与其他快捷键冲突，已重置"));
        tr.hotkey = fallback;
    } else {
        tr.hotkey = hk;
    }

    if !ALLOWED_TARGET_LANGS.contains(&tr.target_lang.as_str()) {
        tr.target_lang = default_target_lang();
    }

    let mut seen = HashSet::new();
    let mut clean_eng = Vec::new();
    for raw in std::mem::take(&mut tr.engines) {
        let mut low = raw.trim().to_ascii_lowercase();
        if low == "bing" {
            low = "google".into();
        }
        if ALL_TRANSLATE_ENGINES.contains(&low.as_str()) && seen.insert(low.clone()) {
            clean_eng.push(low);
        }
    }
    if clean_eng.is_empty() {
        clean_eng = default_engines();
    }
    tr.engines = clean_eng;

    if !is_safe_http_url(&tr.openai_base_url) {
        tr.openai_base_url = default_openai_url();
        notes.push("OpenAI 接口地址无效，已恢复默认".into());
    }
    if !is_safe_http_url(&tr.ollama_url) {
        tr.ollama_url = default_ollama_url();
        notes.push("Ollama 接口地址无效，已恢复默认".into());
    }
    tr.openai_model = sanitize_model_name(&tr.openai_model, "gpt-4o-mini");
    tr.ollama_model = sanitize_model_name(&tr.ollama_model, "qwen2.5:7b");
}

fn sanitize_model_name(raw: &str, fallback: &str) -> String {
    let clean: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'))
        .take(64)
        .collect();
    if clean.is_empty() { fallback.into() } else { clean }
}
