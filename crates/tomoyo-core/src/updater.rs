//! Lightweight GitHub Releases Auto-Updater built on zero-bloat native WinHTTP.
//!
//! - Zero persistent background threads: startup check runs on a single one-shot thread that
//!   exits immediately after updating the shared `UpdateStatus`.
//! - Supports SemVer comparison, interval gating (`config.update.check_interval_hours`),
//!   interactive `:update` checks, and Settings Center update UI.

use crate::config::TomoyoConfig;
use crate::http::{http_download_to_file, http_get};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
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
        release: ReleaseInfo,
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

    pub fn snapshot(&self) -> UpdateStatus {
        self.status
            .read()
            .map(|g| g.clone())
            .unwrap_or(UpdateStatus::Idle)
    }

    /// Returns latest available version string if a newer GitHub Release was found.
    pub fn available_release(&self) -> Option<ReleaseInfo> {
        match self.snapshot() {
            UpdateStatus::Available { release, .. } => Some(release),
            _ => None,
        }
    }

    /// Performs a one-shot non-blocking startup check if `auto_check` is enabled and interval elapsed.
    pub fn check_async_if_due(self: &Arc<Self>, config: &Arc<RwLock<TomoyoConfig>>) {
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
                if matches!(res, UpdateStatus::UpToDate { .. } | UpdateStatus::Available { .. }) {
                    if let Ok(mut cfg) = cfg_arc.write() {
                        cfg.update.last_checked_unix = now_unix();
                        let _ = cfg.save();
                    }
                }
            });
    }

    /// Runs a live GitHub Releases check synchronously (used by `:update` and Settings "Check Now").
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

/// Queries `https://api.github.com/repos/{repo}/releases/latest` via native WinHTTP.
pub fn fetch_latest_github_release(repo: &str, current_ver: &str) -> UpdateStatus {
    let now = now_unix();
    let clean_repo = repo.trim().trim_matches('/');
    let api_url = format!("https://api.github.com/repos/{clean_repo}/releases/latest");

    let resp = match http_get(&api_url, 6000, 1024 * 1024) {
        Ok(r) => r,
        Err(e) => {
            return UpdateStatus::Error {
                message: e,
                checked_at_unix: now,
            };
        }
    };

    if resp.status_code == 404 {
        return UpdateStatus::UpToDate {
            current: current_ver.to_string(),
            latest: current_ver.to_string(),
            checked_at_unix: now,
            note: "仓库尚未发布正式 Release，当前已是最新开发版".into(),
        };
    }

    if resp.status_code != 200 {
        return UpdateStatus::Error {
            message: format!("GitHub API HTTP {}", resp.status_code),
            checked_at_unix: now,
        };
    }

    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&resp.body) else {
        return UpdateStatus::Error {
            message: "无法解析 GitHub Release JSON".into(),
            checked_at_unix: now,
        };
    };

    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let latest_ver = normalize_version(&tag);
    if latest_ver.is_empty() {
        return UpdateStatus::Error {
            message: "Release 缺少 tag_name".into(),
            checked_at_unix: now,
        };
    }

    if !is_newer_version(current_ver, &latest_ver) {
        return UpdateStatus::UpToDate {
            current: current_ver.to_string(),
            latest: latest_ver,
            checked_at_unix: now,
            note: String::new(),
        };
    }

    let mut download_url = None;
    let mut asset_name = None;
    let mut sha256_url = None;
    if let Some(assets) = json.get("assets").and_then(|v| v.as_array()) {
        for a in assets {
            let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.eq_ignore_ascii_case("SHA256SUMS.txt") {
                sha256_url = a
                    .get("browser_download_url")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string);
            }
        }
        // Prefer Windows installer (`-setup.exe` / `.exe` / `.msi`), fall back to portable `.zip`.
        for suffix in ["-setup.exe", ".exe", ".msi", ".zip"] {
            for a in assets {
                let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let dl = a.get("browser_download_url").and_then(|v| v.as_str());
                if let Some(url) = dl {
                    if name.to_ascii_lowercase().ends_with(suffix) {
                        asset_name = Some(name.to_string());
                        download_url = Some(url.to_string());
                        break;
                    }
                }
            }
            if download_url.is_some() {
                break;
            }
        }
    }

    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("https://github.com/{clean_repo}/releases"));

    let release = ReleaseInfo {
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
        download_url,
        asset_name,
        sha256_url,
    };

    UpdateStatus::Available {
        current: current_ver.to_string(),
        release,
        checked_at_unix: now,
    }
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

pub fn parse_sha256_for_asset(sums_txt: &str, asset_name: &str) -> Option<String> {
    for line in sums_txt.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(file_part) = parts.next() else {
            continue;
        };
        let clean_name = file_part.trim_start_matches('*');
        if clean_name.eq_ignore_ascii_case(asset_name)
            && hash.len() == 64
            && hash.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

pub fn compute_file_sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("open `{}`: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 32 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("read error: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Streams a GitHub Release installer/zip to `%TEMP%\tomoyo-update\` and verifies `SHA256SUMS.txt`.
pub fn download_and_verify_release_asset(release: &ReleaseInfo) -> Result<PathBuf, String> {
    let url = release
        .download_url
        .as_deref()
        .ok_or_else(|| "Release 不包含可下载的安装包资源".to_string())?;
    let name = release
        .asset_name
        .as_deref()
        .unwrap_or("Tomoyo-update-setup.exe");
    let safe_name = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Tomoyo-update-setup.exe");

    let expected_sha = if let Some(sums_url) = &release.sha256_url {
        let resp = http_get(sums_url, 10_000, 128 * 1024)?;
        if resp.status_code == 200 {
            parse_sha256_for_asset(&resp.text_lossy(), safe_name)
        } else {
            None
        }
    } else {
        None
    };

    let dest_dir = std::env::temp_dir().join("tomoyo-update");
    let _ = std::fs::create_dir_all(&dest_dir);
    let dest_path = dest_dir.join(safe_name);
    let _ = http_download_to_file(url, &dest_path, 45_000, 128 * 1024 * 1024)?;

    if let Some(expected) = expected_sha {
        let actual = compute_file_sha256(&dest_path)?;
        if !actual.eq_ignore_ascii_case(&expected) {
            let _ = std::fs::remove_file(&dest_path);
            return Err(format!(
                "安装包 SHA-256 校验失败（预期 {}，实际 {}），已阻止运行",
                &expected[..12],
                &actual[..12]
            ));
        }
    }
    Ok(dest_path)
}

#[cfg(windows)]
pub fn launch_verified_update_installer(installer_path: &Path) -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let ext = installer_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "exe" {
        std::process::Command::new(installer_path)
            .args(["/SP-", "/VERYSILENT", "/NORESTART", "/CLOSEAPPLICATIONS"])
            .creation_flags(CREATE_NEW_PROCESS_GROUP)
            .spawn()
            .map_err(|e| format!("拉起安装程序失败: {e}"))?;
        Ok(true)
    } else {
        let _ = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", installer_path.display()))
            .spawn();
        Ok(false)
    }
}

#[cfg(not(windows))]
pub fn launch_verified_update_installer(_installer_path: &Path) -> Result<bool, String> {
    Err("Self-update installer is only supported on Windows".into())
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
}
