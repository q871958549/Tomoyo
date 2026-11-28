//! Local plain-JSON history ring buffer for recent AI queries & answers (`ai_history.json`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_HISTORY: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiHistoryEntry {
    pub id: String,
    pub mode: String,
    pub query: String,
    pub reply: String,
    pub model: String,
    pub timestamp_secs: u64,
}

pub fn tomoyo_data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("TOMOYO_DATA_DIR")
        && !custom.trim().is_empty()
    {
        return PathBuf::from(custom.trim());
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Tomoyo");
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg.trim()).join("tomoyo");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local").join("share").join("tomoyo");
    }
    PathBuf::from(".tomoyo")
}

pub fn history_path() -> PathBuf {
    tomoyo_data_dir().join("ai_history.json")
}

pub fn load_history_from(path: &PathBuf) -> Vec<AiHistoryEntry> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut list: Vec<AiHistoryEntry> = serde_json::from_str(&raw).unwrap_or_default();
    list.truncate(MAX_HISTORY);
    list
}

pub fn save_history_to(path: &PathBuf, entries: &[AiHistoryEntry]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string_pretty(entries)
        && std::fs::write(&tmp, json).is_ok()
    {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub fn push_history_to(path: &PathBuf, mode: &str, query: &str, reply: &str, model: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut entries = load_history_from(path);
    entries.retain(|e| !(e.mode == mode && e.query == query));
    entries.insert(
        0,
        AiHistoryEntry {
            id: format!("{now}_{}", entries.len()),
            mode: mode.to_string(),
            query: query.chars().take(400).collect(),
            reply: reply.chars().take(12_000).collect(),
            model: model.to_string(),
            timestamp_secs: now,
        },
    );
    entries.truncate(MAX_HISTORY);
    save_history_to(path, &entries);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_roundtrip_caps_and_dedups() {
        let p = std::env::temp_dir().join(format!("tomoyo_ai_hist_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        for i in 0..30 {
            push_history_to(&p, "ask", &format!("q{i}"), &format!("a{i}"), "qwen");
        }
        push_history_to(&p, "ask", "q29", "updated29", "qwen");
        let list = load_history_from(&p);
        assert_eq!(list.len(), MAX_HISTORY);
        assert_eq!(list[0].reply, "updated29");
        let _ = std::fs::remove_file(&p);
    }
}
