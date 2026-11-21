use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Hard ceiling on persisted global frecency entries to guarantee bounded memory.
const MAX_FRECENCY_ENTRIES: usize = 2000;
/// Hard ceiling on persisted query-bound keys (`normalized_query -> item_id -> record`).
const MAX_QUERY_KEYS: usize = 400;
/// Hard ceiling on distinct items remembered per normalized query.
const MAX_ITEMS_PER_QUERY: usize = 8;
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
    /// Query-aware frecency map: `normalized_query -> (item_id -> record)`.
    #[serde(default)]
    query_entries: HashMap<String, HashMap<String, FrecencyRecord>>,
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
        self.record_use_for_query("", item_id);
    }

    /// Records both global usage and query-bound usage (so typing `as` -> Enter on
    /// `Android Studio` locks it in for future `as` queries without hijacking unrelated queries).
    pub fn record_use_for_query(&mut self, raw_query: &str, item_id: &str) {
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

        if let Some(norm_q) = normalize_frecency_query(raw_query) {
            let bucket = self.query_entries.entry(norm_q).or_default();
            let qrec = bucket
                .entry(item_id.to_string())
                .or_insert(FrecencyRecord {
                    count: 0,
                    last_used_unix: now,
                });
            qrec.count = qrec.count.saturating_add(1);
            qrec.last_used_unix = now;
        }

        self.enforce_bound();
        self.save_best_effort();
    }

    /// Returns global frequency/recency bonus (`0..=1400`) used for tie-breaking and empty query.
    pub fn bonus(&self, item_id: &str) -> i64 {
        let Some(rec) = self.entries.get(item_id) else {
            return 0;
        };
        let score = decayed_score(rec, now_unix(), 520.0);
        (score.round() as i64).clamp(0, 1_400)
    }

    /// Returns combined global + query-bound frecency bonus (`0..=5000`).
    /// Exact query matches receive a strong boost (`0..=3800`), while unrelated queries
    /// only receive the gentle global tie-breaker (`0..=1400`).
    pub fn bonus_for_query(&self, raw_query: &str, item_id: &str) -> i64 {
        let now = now_unix();
        let global = self
            .entries
            .get(item_id)
            .map(|r| decayed_score(r, now, 520.0).round() as i64)
            .unwrap_or(0)
            .clamp(0, 1_400);

        let Some(norm_q) = normalize_frecency_query(raw_query) else {
            return (global * 2).clamp(0, 2_500);
        };

        let mut query_boost = 0i64;
        if let Some(bucket) = self.query_entries.get(&norm_q)
            && let Some(rec) = bucket.get(item_id) {
                query_boost = (decayed_score(rec, now, 1_650.0).round() as i64).clamp(0, 3_800);
            }
        if query_boost == 0 && norm_q.chars().count() >= 2 {
            // Prefix carry-over: selecting under `android` gently lifts `an` / `and` too.
            for (saved_q, bucket) in &self.query_entries {
                if saved_q.len() > norm_q.len() && saved_q.starts_with(&norm_q)
                    && let Some(rec) = bucket.get(item_id) {
                        let ratio = (norm_q.len() as f64 / saved_q.len() as f64).clamp(0.3, 0.9);
                        let b = (decayed_score(rec, now, 950.0) * ratio).round() as i64;
                        query_boost = query_boost.max(b.clamp(0, 1_600));
                    }
            }
        }

        (global + query_boost).clamp(0, 5_000)
    }

    /// Returns top globally frequent item IDs sorted by decayed score (for empty-query launchpad).
    pub fn top_global_ids(&self, limit: usize) -> Vec<(String, i64)> {
        let now = now_unix();
        let mut items: Vec<(String, i64)> = self
            .entries
            .iter()
            .map(|(k, rec)| (k.clone(), decayed_score(rec, now, 900.0).round() as i64))
            .filter(|(_, s)| *s > 0)
            .collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items.truncate(limit);
        items
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
        self.query_entries.shrink_to_fit();
    }

    fn enforce_bound(&mut self) {
        let now = now_unix();
        if self.entries.len() > MAX_FRECENCY_ENTRIES {
            let mut scored: Vec<(String, f64)> = self
                .entries
                .iter()
                .map(|(k, v)| (k.clone(), decayed_score(v, now, 1.0)))
                .collect();
            scored.sort_by(|a, b| a.1.total_cmp(&b.1));
            let remove_count = self.entries.len().saturating_sub(MAX_FRECENCY_ENTRIES);
            for (k, _) in scored.into_iter().take(remove_count) {
                self.entries.remove(&k);
            }
        }

        for bucket in self.query_entries.values_mut() {
            if bucket.len() > MAX_ITEMS_PER_QUERY {
                let mut scored: Vec<(String, f64)> = bucket
                    .iter()
                    .map(|(k, v)| (k.clone(), decayed_score(v, now, 1.0)))
                    .collect();
                scored.sort_by(|a, b| a.1.total_cmp(&b.1));
                let remove_count = bucket.len().saturating_sub(MAX_ITEMS_PER_QUERY);
                for (k, _) in scored.into_iter().take(remove_count) {
                    bucket.remove(&k);
                }
            }
        }

        if self.query_entries.len() > MAX_QUERY_KEYS {
            let mut scored: Vec<(String, f64)> = self
                .query_entries
                .iter()
                .map(|(q, b)| {
                    let top = b
                        .values()
                        .map(|v| decayed_score(v, now, 1.0))
                        .fold(0.0f64, f64::max);
                    (q.clone(), top)
                })
                .collect();
            scored.sort_by(|a, b| a.1.total_cmp(&b.1));
            let remove_count = self.query_entries.len().saturating_sub(MAX_QUERY_KEYS);
            for (q, _) in scored.into_iter().take(remove_count) {
                self.query_entries.remove(&q);
            }
        }
    }

    fn save_best_effort(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        if let Ok(json) = serde_json::to_string(self)
            && std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
    }
}

fn normalize_frecency_query(raw: &str) -> Option<String> {
    let t = raw.trim().to_lowercase();
    if t.is_empty() || t.starts_with(':') || t.starts_with("= ") || t.chars().count() > 32 {
        return None;
    }
    Some(t)
}

fn decayed_score(rec: &FrecencyRecord, now: u64, weight: f64) -> f64 {
    let age = now.saturating_sub(rec.last_used_unix) as f64;
    let decay = 0.5_f64.powf(age / HALF_LIFE_SECS);
    (rec.count as f64).ln_1p() * weight * decay
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_aware_frecency_boosts_bound_query_without_hijacking_others() {
        let mut store = FrecencyStore::default();
        for _ in 0..5 {
            store.record_use_for_query("wx", "app:wechat");
        }
        store.record_use_for_query("as", "app:android_studio");

        let as_android = store.bonus_for_query("as", "app:android_studio");
        let as_wechat = store.bonus_for_query("as", "app:wechat");
        assert!(
            as_android > as_wechat + 400,
            "query-bound `as` ({as_android}) should beat unrelated global `wechat` ({as_wechat})"
        );
        assert!(store.bonus_for_query("wx", "app:wechat") > store.bonus_for_query("wx", "app:android_studio"));
    }
}
