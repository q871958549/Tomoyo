//! Multi-engine parallel translation orchestrator + bounded Translation History & Wordbook store.

pub mod config;
pub mod engines;
pub mod secrets;

pub use config::{sanitize_translate_config, TranslateConfig, ALL_TRANSLATE_ENGINES, ALLOWED_TARGET_LANGS};
pub use engines::{resolve_target_lang, url_encode_component, EngineTranslation};
pub use secrets::{
    delete_secret, get_secret, has_secret, is_valid_secret_key, load_plugin_secrets,
    plugin_secret_key, set_secret,
};

use crate::{data_dir, next_trace_id};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_HISTORY_ITEMS: usize = 200;
const MAX_WORDBOOK_ITEMS: usize = 500;
const MAX_INPUT_CHARS: usize = 4000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateBatchResponse {
    pub trace_id: String,
    pub source_text: String,
    pub target_lang: String,
    pub results: Vec<EngineTranslation>,
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
        data_dir().join("translate_store.json")
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
                ts: Utc::now().timestamp(),
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
                saved_at: Utc::now().timestamp(),
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

/// Translates `text` across all configured engines concurrently using scoped OS threads.
pub fn translate_parallel(
    cfg: &TranslateConfig,
    raw_text: &str,
    source_lang: &str,
    requested_target: &str,
) -> TranslateBatchResponse {
    let trace_id = next_trace_id("TRN");
    let text: String = raw_text.trim().chars().take(MAX_INPUT_CHARS).collect();
    let eff_target = resolve_target_lang(
        &text,
        if requested_target.is_empty() { &cfg.target_lang } else { requested_target },
    );
    if text.is_empty() {
        return TranslateBatchResponse {
            trace_id,
            source_text: text,
            target_lang: eff_target,
            results: Vec::new(),
        };
    }

    let engine_ids: Vec<String> = if cfg.engines.is_empty() {
        vec!["google".into(), "mymemory".into()]
    } else {
        cfg.engines.clone()
    };

    let results: Vec<EngineTranslation> = std::thread::scope(|s| {
        let handles: Vec<_> = engine_ids
            .iter()
            .map(|id| {
                let id_ref = id.as_str();
                let txt_ref = text.as_str();
                let tgt_ref = eff_target.as_str();
                s.spawn(move || engines::run_engine(id_ref, cfg, txt_ref, source_lang, tgt_ref))
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });

    if let Some(first_ok) = results.iter().find(|r| r.ok && !r.translated.trim().is_empty()) {
        let mut store = TranslateStore::load_default();
        store.record_history(&text, &first_ok.translated, &eff_target, &first_ok.engine_name);
        store.save_default();
    }
    tracing::info!(
        trace_id = %trace_id,
        target = %eff_target,
        engines = results.len(),
        ok_count = results.iter().filter(|r| r.ok).count(),
        "Completed multi-engine translation"
    );

    TranslateBatchResponse {
        trace_id,
        source_text: text,
        target_lang: eff_target,
        results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_store_persists_wordbook_and_history_bounded() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_tr_store_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let mut store = TranslateStore::default();
        store.record_history("resilient", "有韧性的", "zh-Hans", "Bing");
        assert!(store.save_word("resilient", "有韧性的", "/rɪˈzɪliənt/", "高频词"));
        store.save_at(&tmp).unwrap();

        let loaded = TranslateStore::load_at(&tmp);
        assert_eq!(loaded.history.len(), 1);
        assert_eq!(loaded.wordbook.len(), 1);
        assert_eq!(loaded.wordbook[0].word, "resilient");
        let _ = std::fs::remove_file(&tmp);
    }
}
