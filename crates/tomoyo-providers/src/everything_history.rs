//! Bounded persisted Everything search history ring (`%LOCALAPPDATA%\Tomoyo\ev_history.json`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_EV_HISTORY: usize = 25;
static HISTORY: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn history_path() -> PathBuf {
    tomoyo_core::data_dir().join("ev_history.json")
}

fn load_from(path: &Path) -> VecDeque<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .map(|v| v.into_iter().take(MAX_EV_HISTORY).collect())
        .unwrap_or_else(|| VecDeque::with_capacity(MAX_EV_HISTORY))
}

fn save_to(path: &Path, items: &VecDeque<String>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let vec: Vec<&str> = items.iter().map(String::as_str).collect();
    if let Ok(json) = serde_json::to_string(&vec) {
        let _ = std::fs::write(path, json);
    }
}

fn history_lock() -> &'static Mutex<VecDeque<String>> {
    HISTORY.get_or_init(|| Mutex::new(load_from(&history_path())))
}

pub fn record_everything_query(raw_query: &str) {
    let trimmed = raw_query.trim();
    if trimmed.is_empty() {
        return;
    }
    let clean = trimmed
        .strip_prefix("f ")
        .or_else(|| trimmed.strip_prefix("ev "))
        .unwrap_or(trimmed)
        .trim();
    if clean.chars().count() < 2 {
        return;
    }
    if let Ok(mut guard) = history_lock().lock() {
        push_bounded(&mut guard, clean);
        save_to(&history_path(), &guard);
    }
}

pub fn recent_everything_queries(limit: usize) -> Vec<String> {
    history_lock()
        .lock()
        .map(|g| g.iter().take(limit).cloned().collect())
        .unwrap_or_default()
}

fn push_bounded(ring: &mut VecDeque<String>, query: &str) {
    ring.retain(|existing| !existing.eq_ignore_ascii_case(query));
    ring.push_front(query.to_string());
    while ring.len() > MAX_EV_HISTORY {
        ring.pop_back();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_history_is_deduplicated_and_bounded() {
        let mut ring = VecDeque::new();
        for i in 0..35 {
            push_bounded(&mut ring, &format!("ext:rs query_{i}"));
        }
        assert_eq!(ring.len(), MAX_EV_HISTORY);
        push_bounded(&mut ring, "ext:rs query_20");
        assert_eq!(ring.front().map(String::as_str), Some("ext:rs query_20"));
        assert_eq!(ring.len(), MAX_EV_HISTORY);
    }
}
