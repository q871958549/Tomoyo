//! Per-plugin local JSON document database backing `window.tomoyo.db` and `window.utools.db` / `dbStorage`.

use serde_json::{Map, Value};
use std::path::PathBuf;

const MAX_DOCS_PER_PLUGIN: usize = 2000;

pub struct PluginDb {
    path: PathBuf,
    docs: Map<String, Value>,
}

impl PluginDb {
    pub fn open(plugin_id: &str) -> Self {
        let safe_id: String = plugin_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = tomoyo_core::data_dir()
            .join("plugin-data")
            .join(format!("{safe_id}.json"));
        Self::open_at(path)
    }

    pub fn open_at(path: PathBuf) -> Self {
        let docs = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Map<String, Value>>(&s).ok())
            .unwrap_or_default();
        Self { path, docs }
    }

    pub fn snapshot_json(&self) -> String {
        serde_json::to_string(&self.docs).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn put(&mut self, id: String, doc: Value) {
        if self.docs.len() >= MAX_DOCS_PER_PLUGIN && !self.docs.contains_key(&id) {
            if let Some(first_key) = self.docs.keys().next().cloned() {
                self.docs.remove(&first_key);
            }
        }
        self.docs.insert(id, doc);
        self.save();
    }

    pub fn remove(&mut self, id: &str) {
        if self.docs.remove(id).is_some() {
            self.save();
        }
    }

    pub fn get(&self, id: &str) -> Option<&Value> {
        self.docs.get(id)
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = self.path.with_extension("json.tmp");
        if let Ok(json) = serde_json::to_string(&self.docs) {
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_db_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_wv_db_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let mut db = PluginDb::open_at(tmp.clone());
            db.put("k1".into(), serde_json::json!({"_id": "k1", "value": "hello"}));
            assert_eq!(db.get("k1").unwrap()["value"], "hello");
        }
        {
            let mut db2 = PluginDb::open_at(tmp.clone());
            assert_eq!(db2.get("k1").unwrap()["value"], "hello");
            db2.remove("k1");
            assert!(db2.get("k1").is_none());
        }
        let _ = std::fs::remove_file(&tmp);
    }
}
