//! Bounded persistent cache (`%LOCALAPPDATA%\Tomoyo\update_cache.json`) for release metadata & ETag.

use super::{is_newer_version, normalize_version, ReleaseInfo, UpdateStatus};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCacheEntry {
    pub repo: String,
    #[serde(default)]
    pub etag: Option<String>,
    pub release: ReleaseInfo,
    pub checked_at_unix: u64,
}

fn cache_path() -> PathBuf {
    crate::data_dir().join("update_cache.json")
}

pub fn load_cache_entry_at(path: &std::path::Path, repo: &str) -> Option<UpdateCacheEntry> {
    let raw = std::fs::read_to_string(path).ok()?;
    let entry: UpdateCacheEntry = serde_json::from_str(&raw).ok()?;
    let clean_target = repo.trim().trim_matches('/');
    let clean_saved = entry.repo.trim().trim_matches('/');
    if clean_saved.eq_ignore_ascii_case(clean_target) {
        Some(entry)
    } else {
        None
    }
}

pub fn save_cache_entry_at(path: &std::path::Path, entry: &UpdateCacheEntry) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(entry) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

pub fn load_cache_entry(repo: &str) -> Option<UpdateCacheEntry> {
    load_cache_entry_at(&cache_path(), repo)
}

pub fn save_cache_entry(repo: &str, etag: Option<String>, release: &ReleaseInfo, checked_at: u64) {
    let entry = UpdateCacheEntry {
        repo: repo.trim().trim_matches('/').to_string(),
        etag,
        release: release.clone(),
        checked_at_unix: checked_at,
    };
    save_cache_entry_at(&cache_path(), &entry);
}

/// Converts a cached entry into `Available` / `UpToDate` against `current_ver`.
pub fn load_cached_update_status(repo: &str, current_ver: &str) -> Option<UpdateStatus> {
    let entry = load_cache_entry(repo)?;
    Some(status_from_release(
        current_ver,
        entry.release,
        entry.checked_at_unix,
    ))
}

pub fn status_from_release(
    current_ver: &str,
    release: ReleaseInfo,
    checked_at_unix: u64,
) -> UpdateStatus {
    let latest_ver = normalize_version(&release.version);
    if latest_ver.is_empty() || !is_newer_version(current_ver, &latest_ver) {
        let note = match release.source.as_str() {
            "github_web" => "来源：GitHub Web 直连检查".to_string(),
            "github_api_cached" => "来源：GitHub API 条件缓存 (304)".to_string(),
            _ => String::new(),
        };
        UpdateStatus::UpToDate {
            current: current_ver.to_string(),
            latest: if latest_ver.is_empty() {
                current_ver.to_string()
            } else {
                latest_ver
            },
            checked_at_unix,
            note,
        }
    } else {
        UpdateStatus::Available {
            current: current_ver.to_string(),
            release: Box::new(release),
            checked_at_unix,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_update_cache_and_flips_when_upgraded() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_upd_cache_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let rel = ReleaseInfo {
            tag: "v1.4.0".into(),
            version: "1.4.0".into(),
            name: "Tomoyo v1.4.0".into(),
            notes: "Multi-engine translator".into(),
            html_url: "https://github.com/q871958549/Tomoyo/releases/tag/v1.4.0".into(),
            published_at: "2026-11-20T10:44:41Z".into(),
            download_url: Some("https://github.com/q871958549/Tomoyo/releases/download/v1.4.0/Tomoyo-v1.4.0-windows-x64-setup.exe".into()),
            asset_name: Some("Tomoyo-v1.4.0-windows-x64-setup.exe".into()),
            sha256_url: None,
            portable_url: None,
            portable_name: None,
            source: "github_web".into(),
        };
        save_cache_entry_at(
            &tmp,
            &UpdateCacheEntry {
                repo: "q871958549/Tomoyo".into(),
                etag: Some("\"abc\"".into()),
                release: rel,
                checked_at_unix: 12345,
            },
        );

        let loaded = load_cache_entry_at(&tmp, "q871958549/Tomoyo").unwrap();
        assert_eq!(loaded.etag.as_deref(), Some("\"abc\""));
        assert!(matches!(
            status_from_release("1.0.0", loaded.release.clone(), loaded.checked_at_unix),
            UpdateStatus::Available { .. }
        ));
        assert!(matches!(
            status_from_release("1.4.0", loaded.release, loaded.checked_at_unix),
            UpdateStatus::UpToDate { .. }
        ));

        let _ = std::fs::remove_file(&tmp);
    }
}
