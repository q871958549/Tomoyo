//! Disk-persisted sharded registry cache with HTTP `ETag` / `If-None-Match` (`304 Not Modified`),
//! parallel shard fetching, offline fallback, and pure-Rust SemVer comparison.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tomoyo_protocol::PluginRegistryIndex;

const CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_PARALLEL_SHARDS: usize = 6;

pub fn store_cache_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg).join("tomoyo").join("store");
    }
    tomoyo_core::data_dir().join("cache").join("store")
}

pub fn cache_key_for_url(url: &str) -> String {
    let mut h = Sha256::new();
    h.update(url.as_bytes());
    let digest = h.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..24].to_string()
}

fn cache_paths(cache_dir: &Path, url: &str) -> (PathBuf, PathBuf) {
    let key = cache_key_for_url(url);
    (cache_dir.join(format!("{key}.json")), cache_dir.join(format!("{key}.etag")))
}

pub fn read_cached_index_at(cache_dir: &Path, url: &str) -> Option<(PluginRegistryIndex, Option<String>, bool)> {
    let (json_path, etag_path) = cache_paths(cache_dir, url);
    let raw = std::fs::read(&json_path).ok()?;
    let idx: PluginRegistryIndex = serde_json::from_slice(&raw).ok()?;
    let etag = std::fs::read_to_string(&etag_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let fresh = std::fs::metadata(&json_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|mt| SystemTime::now().duration_since(mt).ok())
        .is_some_and(|age| age < CACHE_TTL);
    Some((idx, etag, fresh))
}

pub fn write_cached_index_at(
    cache_dir: &Path,
    url: &str,
    body: &[u8],
    etag: Option<&str>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir)?;
    let (json_path, etag_path) = cache_paths(cache_dir, url);
    let tmp_json = cache_dir.join(format!("{}.tmp", cache_key_for_url(url)));
    std::fs::write(&tmp_json, body)?;
    if std::fs::rename(&tmp_json, &json_path).is_err() {
        std::fs::copy(&tmp_json, &json_path)?;
        let _ = std::fs::remove_file(&tmp_json);
    }
    if let Some(tag) = etag.map(str::trim).filter(|s| !s.is_empty()) {
        let _ = std::fs::write(&etag_path, tag);
    }
    Ok(())
}

/// Fetches a remote registry index or shard using disk cache + conditional `If-None-Match` (`304`).
/// Falls back cleanly to stale disk cache if offline.
pub fn fetch_remote_index_cached(
    url: &str,
    force_refresh: bool,
) -> Result<(PluginRegistryIndex, bool), String> {
    let dir = store_cache_dir();
    let cached = read_cached_index_at(&dir, url);
    if !force_refresh
        && let Some((idx, _, true)) = &cached
    {
        return Ok((idx.clone(), true));
    }

    let mut headers: Vec<(&str, &str)> = Vec::new();
    let etag_owned = cached.as_ref().and_then(|(_, e, _)| e.clone());
    if let Some(tag) = etag_owned.as_deref() {
        headers.push(("If-None-Match", tag));
    }

    match tomoyo_core::http::http_get_detailed(url, &headers, 8000, 8 * 1024 * 1024) {
        Ok(resp) if resp.status_code == 304 => {
            if let Some((idx, _, _)) = cached {
                // Touch cache file so TTL refreshes cleanly on 304 Not Modified.
                if let Ok(bytes) = serde_json::to_vec(&idx) {
                    let _ = write_cached_index_at(&dir, url, &bytes, resp.etag.as_deref().or(etag_owned.as_deref()));
                }
                return Ok((idx, true));
            }
            Err("HTTP 304 without local cache".into())
        }
        Ok(resp) if resp.status_code == 200 => {
            let idx: PluginRegistryIndex =
                serde_json::from_slice(&resp.body).map_err(|e| e.to_string())?;
            let _ = write_cached_index_at(&dir, url, &resp.body, resp.etag.as_deref());
            Ok((idx, false))
        }
        Ok(resp) => {
            if let Some((idx, _, _)) = cached {
                Ok((idx, true))
            } else {
                Err(format!("HTTP {}", resp.status_code))
            }
        }
        Err(e) => {
            if let Some((idx, _, _)) = cached {
                Ok((idx, true))
            } else {
                Err(e)
            }
        }
    }
}

/// Loads a list of `(shard_id, shard_category, resolved_source)` entries in bounded parallel batches.
pub fn fetch_shards_parallel<F>(
    items: &[(String, String, String)],
    loader: F,
) -> Vec<(String, String, Result<PluginRegistryIndex, String>)>
where
    F: Fn(&str) -> Result<PluginRegistryIndex, String> + Sync,
{
    let mut results = Vec::with_capacity(items.len());
    for chunk in items.chunks(MAX_PARALLEL_SHARDS) {
        std::thread::scope(|s| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|(id, cat, src)| {
                    let loader_ref = &loader;
                    s.spawn(move || (id.clone(), cat.clone(), loader_ref(src)))
                })
                .collect();
            for h in handles {
                if let Ok(item) = h.join() {
                    results.push(item);
                }
            }
        });
    }
    results
}

/// Returns true iff `candidate` is strictly newer than `current` under SemVer (`major.minor.patch`).
pub fn is_newer_semver(candidate: &str, current: &str) -> bool {
    let parse = |raw: &str| -> Option<((u64, u64, u64), bool, String)> {
        let s = raw.trim().trim_start_matches(['v', 'V']);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p.split('+').next().unwrap_or(p).to_string())),
            None => (s.split('+').next().unwrap_or(s), None),
        };
        let mut parts = core.split('.');
        let maj = parts.next()?.parse::<u64>().ok()?;
        let min = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        let pat = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        Some(((maj, min, pat), pre.is_none(), pre.unwrap_or_default()))
    };
    let (Some((c_ver, c_stable, c_pre)), Some((cur_ver, cur_stable, cur_pre))) =
        (parse(candidate), parse(current))
    else {
        return false;
    };
    if c_ver != cur_ver {
        return c_ver > cur_ver;
    }
    match (c_stable, cur_stable) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => false,
        (false, false) => c_pre > cur_pre,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_versions_accurately() {
        assert!(is_newer_semver("2.7.0", "2.6.1"));
        assert!(is_newer_semver("v1.10.0", "1.9.9"));
        assert!(is_newer_semver("1.0.0", "1.0.0-rc.1"));
        assert!(!is_newer_semver("2.6.1", "2.6.1"));
        assert!(!is_newer_semver("2.6.0", "2.6.1"));
        assert!(!is_newer_semver("1.0.0-beta.1", "1.0.0"));
    }

    #[test]
    fn persists_and_reloads_sharded_index_cache_with_etag() {
        let dir = std::env::temp_dir().join(format!("tomoyo_store_cache_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let url = "https://example.invalid/registry/shards/official.json";
        let payload = serde_json::json!({
            "schema_version": 2,
            "name": "Official Shard",
            "plugins": []
        })
        .to_string();
        write_cached_index_at(&dir, url, payload.as_bytes(), Some("\"etag-v2\"")).unwrap();
        let (idx, etag, fresh) = read_cached_index_at(&dir, url).unwrap();
        assert_eq!(idx.name, "Official Shard");
        assert_eq!(etag.as_deref(), Some("\"etag-v2\""));
        assert!(fresh);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
