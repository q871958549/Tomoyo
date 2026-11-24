//! Per-plugin isolated persistent KV store for WASM Runtime ABI v2 (`host_kv_get` / `host_kv_set`).
//!
//! Each plugin gets its own bounded JSON file under `<data_dir>/plugin-kv/<sanitized-id>.json`.
//! Strict isolation: plugin A can never read or mutate plugin B's keys, and keys/values are bounded.

use std::collections::BTreeMap;
use std::path::PathBuf;

const MAX_KV_KEYS: usize = 128;
const MAX_KEY_BYTES: usize = 128;
const MAX_VAL_BYTES: usize = 16 * 1024;
const MAX_FILE_BYTES: usize = 256 * 1024;

fn kv_file_path(plugin_id: &str) -> PathBuf {
    let safe: String = plugin_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(80)
        .collect();
    let stem = if safe.is_empty() { "unnamed".to_string() } else { safe };
    tomoyo_core::data_dir().join("plugin-kv").join(format!("{stem}.json"))
}

fn load_map(plugin_id: &str) -> BTreeMap<String, String> {
    let path = kv_file_path(plugin_id);
    let Ok(meta) = std::fs::metadata(&path) else {
        return BTreeMap::new();
    };
    if meta.len() as usize > MAX_FILE_BYTES {
        return BTreeMap::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<BTreeMap<String, String>>(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn kv_get(plugin_id: &str, key: &str) -> Option<String> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return None;
    }
    load_map(plugin_id).remove(key)
}

pub(crate) fn kv_set(plugin_id: &str, key: &str, val: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return Err("invalid key length".into());
    }
    if val.len() > MAX_VAL_BYTES {
        return Err("value exceeds 16 KB limit".into());
    }
    let mut map = load_map(plugin_id);
    if !map.contains_key(key) && map.len() >= MAX_KV_KEYS {
        return Err("kv key cap (128) reached".into());
    }
    if val.is_empty() {
        map.remove(key);
    } else {
        map.insert(key.to_string(), val.to_string());
    }
    let path = kv_file_path(plugin_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(&map).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes()).map_err(|e| e.to_string())?;
    if std::fs::rename(&tmp, &path).is_err() {
        std::fs::copy(&tmp, &path).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn kv_clear_for_test(plugin_id: &str) {
    let _ = std::fs::remove_file(kv_file_path(plugin_id));
}
