use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Hard ceiling on persisted frecency entries to guarantee bounded memory.
const MAX_FRECENCY_ENTRIES: usize = 2000;
/// Half-life in days (14 days).
const HALF_LIFE_SECS: f64 = 14.0 * 86_400.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrecencyRecord {
    pub count: u32,
    pub last_used_unix: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrecencyStore {
    entries: HashMap<String, FrecencyRecord>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl FrecencyStore {
    pub fn load_or_default(path: PathBuf) -> Self {
        let mut store = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default();
        store.path = Some(path);
        store.enforce_bound();
        store
    }

    pub fn record_use(&mut self, item_id: &str) {
        let now = now_unix();
        let entry = self
            .entries
            .entry(item_id.to_string())
            .or_insert(FrecencyRecord {
                count: 0,
                last_used_unix: now,
            });
        entry.count = entry.count.saturating_add(1);
        entry.last_used_unix = now;
        self.enforce_bound();
        self.save_best_effort();
    }

    /// Returns a bounded score bonus `0..=2500` based on frequency and recency decay.
    pub fn bonus(&self, item_id: &str) -> i64 {
        let Some(rec) = self.entries.get(item_id) else {
            return 0;
        };
        let now = now_unix();
        let age = now.saturating_sub(rec.last_used_unix) as f64;
        let decay = 0.5_f64.powf(age / HALF_LIFE_SECS);
        let raw = (rec.count as f64).ln_1p() * 900.0 * decay;
        (raw.round() as i64).clamp(0, 2_500)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn shrink(&mut self) {
        self.enforce_bound();
        self.entries.shrink_to_fit();
    }

    fn enforce_bound(&mut self) {
        if self.entries.len() <= MAX_FRECENCY_ENTRIES {
            return;
        }
        let now = now_unix();
        let mut scored: Vec<(String, f64)> = self
            .entries
            .iter()
            .map(|(k, v)| {
                let age = now.saturating_sub(v.last_used_unix) as f64;
                let s = (v.count as f64) * 0.5_f64.powf(age / HALF_LIFE_SECS);
                (k.clone(), s)
            })
            .collect();
        scored.sort_by(|a, b| a.1.total_cmp(&b.1));
        let remove_count = self.entries.len().saturating_sub(MAX_FRECENCY_ENTRIES);
        for (k, _) in scored.into_iter().take(remove_count) {
            self.entries.remove(&k);
        }
    }

    fn save_best_effort(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        if let Ok(json) = serde_json::to_string(self) {
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
