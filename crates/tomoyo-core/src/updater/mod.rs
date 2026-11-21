//! Zero-quota-aware GitHub Releases Auto-Updater built on native WinHTTP.
//!
//! Resolution strategy (zero persistent threads):
//! 1. Sends conditional `If-None-Match` (`ETag`) to `api.github.com` when cached (`304` uses 0 API quota).
//! 2. Automatically falls back to zero-quota GitHub Web (`releases/latest`, `releases.atom`,
//!    `expanded_assets`) on `HTTP 403` / `429` rate limits, `5xx`, or transient REST failures.
//! 3. Persists latest release metadata into `%LOCALAPPDATA%\Tomoyo\update_cache.json`.

pub mod cache;
pub mod installer;
pub mod web_feed;

pub use cache::load_cached_update_status;
pub use installer::{
    compute_file_sha256, detect_install_kind, download_and_verify_release_asset,
    download_and_verify_release_asset_with_progress, launch_verified_update_installer,
    parse_sha256_for_asset, pick_release_assets, CandidateAsset, InstallKind,
    UpdateInstallProgress,
};

use crate::config::TomoyoConfig;
use crate::http::http_get_detailed;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub name: String,
    pub notes: String,
    pub html_url: String,
    pub published_at: String,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    #[serde(default)]
    pub sha256_url: Option<String>,
    #[serde(default)]
    pub portable_url: Option<String>,
    #[serde(default)]
    pub portable_name: Option<String>,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate {
        current: String,
        latest: String,
        checked_at_unix: u64,
        #[serde(default)]
        note: String,
    },
    Available {
        current: String,
        release: Box<ReleaseInfo>,
        checked_at_unix: u64,
    },
    Error {
        message: String,
        checked_at_unix: u64,
    },
}

/// Thread-safe auto-update coordinator shared between `tomoyo-app`, `SystemProvider`, and UI.
pub struct UpdateManager {
    status: RwLock<UpdateStatus>,
}

impl UpdateManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            status: RwLock::new(UpdateStatus::Idle),
        })
    }

    pub fn with_cached(repo: &str) -> Arc<Self> {
        let initial = load_cached_update_status(repo, CURRENT_VERSION).unwrap_or(UpdateStatus::Idle);
        Arc::new(Self {
            status: RwLock::new(initial),
        })
    }

    pub fn snapshot(&self) -> UpdateStatus {
        self.status
            .read()
            .map(|g| g.clone())
            .unwrap_or(UpdateStatus::Idle)
    }

    pub fn available_release(&self) -> Option<ReleaseInfo> {
        match self.snapshot() {
            UpdateStatus::Available { release, .. } => Some(*release),
            _ => None,
        }
    }

    pub fn check_async_if_due(self: &Arc<Self>, config: &Arc<RwLock<TomoyoConfig>>) {
        self.check_async_if_due_with_notify(config, || {});
    }

    pub fn check_async_if_due_with_notify<F>(
        self: &Arc<Self>,
        config: &Arc<RwLock<TomoyoConfig>>,
        on_done: F,
    ) where
        F: FnOnce() + Send + 'static,
    {
        let (enabled, repo, interval_secs, last_checked) = match config.read() {
            Ok(c) => (
                c.update.auto_check,
                c.update.repo.clone(),
                c.update.check_interval_hours.max(1) * 3600,
                c.update.last_checked_unix,
            ),
            Err(_) => return,
        };
        if !enabled || repo.trim().is_empty() {
            return;
        }
        let now = now_unix();
        if now.saturating_sub(last_checked) < interval_secs {
            return;
        }

        let mgr = Arc::clone(self);
        let cfg_arc = Arc::clone(config);
        let _ = std::thread::Builder::new()
            .name("tomoyo-update-check".into())
            .spawn(move || {
                let res = mgr.check_now_sync(&repo);
                if matches!(res, UpdateStatus::UpToDate { .. } | UpdateStatus::Available { .. })
                    && let Ok(mut cfg) = cfg_arc.write() {
                        cfg.update.last_checked_unix = now_unix();
                        let _ = cfg.save();
                    }
                on_done();
            });
    }

    pub fn check_now_sync(&self, repo: &str) -> UpdateStatus {
        if let Ok(mut w) = self.status.write() {
            *w = UpdateStatus::Checking;
        }
        let result = fetch_latest_github_release(repo, CURRENT_VERSION);
        if let Ok(mut w) = self.status.write() {
            *w = result.clone();
        }
        result
    }
}

/// Resolves latest GitHub Release with `ETag` conditional caching and zero-quota Web fallback.
pub fn fetch_latest_github_release(repo: &str, current_ver: &str) -> UpdateStatus {
    let now = now_unix();
    let clean_repo = repo.trim().trim_matches('/');
    let api_url = format!("https://api.github.com/repos/{clean_repo}/releases/latest");
    let cached = cache::load_cache_entry(clean_repo);
    let mut hdrs: Vec<(&str, &str)> = Vec::new();
    let etag_val = cached.as_ref().and_then(|c| c.etag.clone());
    if let Some(et) = etag_val.as_deref() {
        hdrs.push(("If-None-Match", et));
    }

    match http_get_detailed(&api_url, &hdrs, 6000, 1024 * 1024) {
        Ok(resp) if resp.status_code == 304 => {
            if let Some(c) = cached {
                let mut rel = c.release;
                rel.source = "github_api_cached".into();
                cache::save_cache_entry(clean_repo, c.etag, &rel, now);
                return cache::status_from_release(current_ver, rel, now);
            }
        }
        Ok(resp) if resp.status_code == 200 => {
            if let Some(rel) = parse_github_api_release_json(&resp.body, clean_repo) {
                cache::save_cache_entry(clean_repo, resp.etag, &rel, now);
                return cache::status_from_release(current_ver, rel, now);
            }
        }
        Ok(resp) if resp.status_code == 404 => {
            // Double-check GitHub Web before declaring no releases exist.
            if let Ok(None) = web_feed::fetch_release_via_github_web(clean_repo) {
                return UpdateStatus::UpToDate {
                    current: current_ver.to_string(),
                    latest: current_ver.to_string(),
                    checked_at_unix: now,
                    note: "仓库尚未发布正式 Release，当前已是最新开发版".into(),
                };
            }
            return resolve_via_web_fallback(clean_repo, current_ver, now, "HTTP 404".into());
        }
        Ok(resp) => {
            return resolve_via_web_fallback(
                clean_repo,
                current_ver,
                now,
                format!("GitHub API HTTP {}", resp.status_code),
            );
        }
        Err(err) => {
            return resolve_via_web_fallback(clean_repo, current_ver, now, err);
        }
    }

    resolve_via_web_fallback(clean_repo, current_ver, now, "GitHub API 响应无效".into())
}

fn resolve_via_web_fallback(
    clean_repo: &str,
    current_ver: &str,
    now: u64,
    api_reason: String,
) -> UpdateStatus {
    match web_feed::fetch_release_via_github_web(clean_repo) {
        Ok(Some(rel)) => {
            cache::save_cache_entry(clean_repo, None, &rel, now);
            cache::status_from_release(current_ver, rel, now)
        }
        Ok(None) => UpdateStatus::UpToDate {
            current: current_ver.to_string(),
            latest: current_ver.to_string(),
            checked_at_unix: now,
            note: "仓库尚未发布正式 Release，当前已是最新开发版".into(),
        },
        Err(web_err) => UpdateStatus::Error {
            message: format!("无法连接 GitHub（API: {api_reason}；Web 直连: {web_err}）"),
            checked_at_unix: now,
        },
    }
}

fn parse_github_api_release_json(body: &[u8], clean_repo: &str) -> Option<ReleaseInfo> {
    let json = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let tag = json.get("tag_name").and_then(|v| v.as_str())?.to_string();
    let latest_ver = normalize_version(&tag);
    if latest_ver.is_empty() {
        return None;
    }

    let mut candidates = Vec::new();
    if let Some(assets) = json.get("assets").and_then(|v| v.as_array()) {
        for a in assets {
            if let (Some(name), Some(url)) = (
                a.get("name").and_then(|v| v.as_str()),
                a.get("browser_download_url").and_then(|v| v.as_str()),
            ) {
                candidates.push(CandidateAsset {
                    name: name.to_string(),
                    url: url.to_string(),
                });
            }
        }
    }
    let picked = pick_release_assets(&candidates);
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("https://github.com/{clean_repo}/releases"));

    Some(ReleaseInfo {
        tag: tag.clone(),
        version: latest_ver,
        name: json
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&tag)
            .to_string(),
        notes: json
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(1500)
            .collect(),
        html_url,
        published_at: json
            .get("published_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        download_url: picked.download_url,
        asset_name: picked.asset_name,
        sha256_url: picked.sha256_url,
        portable_url: picked.portable_url,
        portable_name: picked.portable_name,
        source: "github_api".into(),
    })
}

pub fn normalize_version(v: &str) -> String {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .to_string()
}

/// Returns `true` if `candidate` is strictly newer than `current` under SemVer `major.minor.patch`.
pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    let parse = |s: &str| -> (u64, u64, u64) {
        let norm = normalize_version(s);
        let mut it = norm.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
        (
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
        )
    };
    parse(candidate) > parse(current)
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
    fn compares_semver_versions_correctly() {
        assert!(is_newer_version("0.3.0", "v0.4.0"));
        assert!(is_newer_version("0.4.0", "0.4.1"));
        assert!(is_newer_version("0.4.9", "1.0.0"));
        assert!(!is_newer_version("0.4.0", "v0.4.0"));
        assert!(!is_newer_version("0.4.0", "0.3.9"));
    }

    #[test]
    fn parses_sha256sums_correctly() {
        let txt = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  Tomoyo-v1.0.0-windows-x64-setup.exe\n\
                   fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210 *Tomoyo-v1.0.0-windows-x64-portable.zip\n";
        assert_eq!(
            parse_sha256_for_asset(txt, "Tomoyo-v1.0.0-windows-x64-setup.exe").as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            parse_sha256_for_asset(txt, "Tomoyo-v1.0.0-windows-x64-portable.zip").as_deref(),
            Some("fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210")
        );
        assert!(parse_sha256_for_asset(txt, "other.exe").is_none());
    }

    #[test]
    #[ignore = "live network check against GitHub Releases"]
    fn live_github_update_resolution_handles_api_403() {
        let st = fetch_latest_github_release("q871958549/Tomoyo", "1.0.0");
        match st {
            UpdateStatus::Available { release, .. } => {
                assert!(!release.version.is_empty());
                assert!(release.download_url.is_some());
                assert!(release.sha256_url.is_some());
                println!(
                    "[OK] Resolved v{} via `{}` -> asset={:?}, sha={:?}",
                    release.version, release.source, release.asset_name, release.sha256_url
                );
            }
            other => panic!("Expected Available update from 1.0.0, got: {other:?}"),
        }
    }
}
