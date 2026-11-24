//! Persistent bounded Translation History, Wordbook store & settings resolver.
//!
//! Reads and writes the canonical `<tomoyo_data_dir>/translate_store.json` so existing users
//! keep 100% of their history and wordbook entries seamlessly after Phase 3b externalization.

use crate::http_curl::is_safe_http_url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_HISTORY_ITEMS: usize = 200;
const MAX_WORDBOOK_ITEMS: usize = 500;
const ALLOWED_TARGET_LANGS: &[&str] = &["auto", "zh-Hans", "en", "ja", "ko", "fr", "de"];

#[derive(Debug, Clone)]
pub struct TranslateRuntimeConfig {
    pub target_lang: String,
    pub engines: Vec<String>,
    pub openai_base_url: String,
    pub openai_model: String,
    pub ollama_url: String,
    pub ollama_model: String,
}

impl Default for TranslateRuntimeConfig {
    fn default() -> Self {
        Self {
            target_lang: "auto".into(),
            engines: vec!["google".into(), "mymemory".into()],
            openai_base_url: "https://api.openai.com/v1".into(),
            openai_model: "gpt-4o-mini".into(),
            ollama_url: "http://127.0.0.1:11434".into(),
            ollama_model: "qwen2.5:7b".into(),
        }
    }
}

pub fn resolve_runtime_config(settings: &BTreeMap<String, Value>) -> TranslateRuntimeConfig {
    let mut cfg = TranslateRuntimeConfig::default();
    if let Some(t) = settings.get("target_lang").and_then(Value::as_str)
        && ALLOWED_TARGET_LANGS.contains(&t)
    {
        cfg.target_lang = t.to_string();
    }
    let mut engines = Vec::new();
    let flag = |k: &str, default_on: bool| -> bool {
        settings.get(k).and_then(Value::as_bool).unwrap_or(default_on)
    };
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
    if engines.is_empty() {
        engines = vec!["google".into(), "mymemory".into()];
    }
    cfg.engines = engines;

    if let Some(u) = settings.get("openai_base_url").and_then(Value::as_str)
        && is_safe_http_url(u)
    {
        cfg.openai_base_url = u.trim().to_string();
    }
    if let Some(m) = settings.get("openai_model").and_then(Value::as_str) {
        let clean = sanitize_model(m);
        if !clean.is_empty() {
            cfg.openai_model = clean;
        }
    }
    if let Some(u) = settings.get("ollama_url").and_then(Value::as_str)
        && is_safe_http_url(u)
    {
        cfg.ollama_url = u.trim().to_string();
    }
    if let Some(m) = settings.get("ollama_model").and_then(Value::as_str) {
        let clean = sanitize_model(m);
        if !clean.is_empty() {
            cfg.ollama_model = clean;
        }
    }
    cfg
}

fn sanitize_model(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'))
        .take(64)
        .collect()
}

pub fn tomoyo_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("Tomoyo");
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
            && !xdg.trim().is_empty()
        {
            return PathBuf::from(xdg).join("tomoyo");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".local").join("share").join("tomoyo");
        }
    }
    std::env::temp_dir().join("tomoyo")
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateHistoryEntry {
    pub source: String,
    pub translated: String,
    pub target_lang: String,
    pub engine: String,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordbookEntry {
    pub word: String,
    pub translation: String,
    #[serde(default)]
    pub phonetic: String,
    #[serde(default)]
    pub note: String,
    pub saved_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranslateStore {
    #[serde(default)]
    pub history: Vec<TranslateHistoryEntry>,
    #[serde(default)]
    pub wordbook: Vec<WordbookEntry>,
}

impl TranslateStore {
    pub fn default_path() -> PathBuf {
        tomoyo_data_dir().join("translate_store.json")
    }

    pub fn load_default() -> Self {
        Self::load_at(&Self::default_path())
    }

    pub fn load_at(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_default(&self) {
        let _ = self.save_at(&Self::default_path());
    }

    pub fn save_at(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn record_history(&mut self, source: &str, translated: &str, target_lang: &str, engine: &str) {
        let src = source.trim();
        let tr = translated.trim();
        if src.is_empty() || tr.is_empty() {
            return;
        }
        self.history.retain(|h| !(h.source == src && h.target_lang == target_lang));
        self.history.insert(
            0,
            TranslateHistoryEntry {
                source: src.chars().take(600).collect(),
                translated: tr.chars().take(1200).collect(),
                target_lang: target_lang.to_string(),
                engine: engine.to_string(),
                ts: unix_now(),
            },
        );
        self.history.truncate(MAX_HISTORY_ITEMS);
    }

    pub fn save_word(&mut self, word: &str, translation: &str, phonetic: &str, note: &str) -> bool {
        let w = word.trim();
        let tr = translation.trim();
        if w.is_empty() || tr.is_empty() {
            return false;
        }
        self.wordbook.retain(|e| !e.word.eq_ignore_ascii_case(w));
        self.wordbook.insert(
            0,
            WordbookEntry {
                word: w.chars().take(240).collect(),
                translation: tr.chars().take(800).collect(),
                phonetic: phonetic.trim().chars().take(64).collect(),
                note: note.trim().chars().take(160).collect(),
                saved_at: unix_now(),
            },
        );
        self.wordbook.truncate(MAX_WORDBOOK_ITEMS);
        true
    }

    pub fn delete_word(&mut self, word: &str) -> bool {
        let before = self.wordbook.len();
        self.wordbook.retain(|e| e.word != word);
        self.wordbook.len() != before
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

pub fn write_last_req(text: &str, target_lang: &str) {
    let dir = tomoyo_data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let body = serde_json::json!({ "text": text, "target_lang": target_lang }).to_string();
    let _ = std::fs::write(dir.join("last_translate_req.json"), body);
}

pub fn read_last_req() -> (String, String) {
    let path = tomoyo_data_dir().join("logs").join("last_translate_req.json");
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let v: Value = serde_json::from_str(&raw).unwrap_or_default();
    let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
    let target = v.get("target_lang").and_then(Value::as_str).unwrap_or("").to_string();
    (text, target)
}
