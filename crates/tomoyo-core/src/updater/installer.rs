//! Install-mode detection, SHA-256 checksum download & zero-gap self-upgrade + auto-relaunch.

#[cfg(windows)]
mod win;

use super::ReleaseInfo;
use crate::http::{http_download_to_file_with_progress, http_get};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    Setup,
    Portable,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInstallProgress {
    pub stage: String,
    pub message: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: u8,
    pub asset_name: String,
}

#[derive(Debug, Clone)]
pub struct CandidateAsset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Default)]
pub struct PickedAssets {
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    pub sha256_url: Option<String>,
    pub portable_url: Option<String>,
    pub portable_name: Option<String>,
}

pub fn detect_install_kind() -> InstallKind {
    let Ok(exe) = std::env::current_exe() else {
        return InstallKind::Setup;
    };
    let Some(dir) = exe.parent() else {
        return InstallKind::Setup;
    };
    if dir.join("unins000.exe").is_file() {
        return InstallKind::Setup;
    }
    let dir_low = dir.to_string_lossy().to_ascii_lowercase();
    if dir_low.contains("\\programs\\tomoyo") || dir_low.contains("\\target\\") {
        InstallKind::Setup
    } else {
        InstallKind::Portable
    }
}

fn find_asset_matching<'a>(
    assets: &'a [CandidateAsset],
    suffixes: &[&str],
    excluded_tokens: &[&str],
) -> Option<&'a CandidateAsset> {
    for suffix in suffixes {
        if let Some(hit) = assets.iter().find(|a| {
            let lower = a.name.to_ascii_lowercase();
            lower.ends_with(suffix) && !excluded_tokens.iter().any(|tok| lower.contains(tok))
        }) {
            return Some(hit);
        }
    }
    None
}

pub fn pick_release_assets(assets: &[CandidateAsset]) -> PickedAssets {
    let mut out = PickedAssets::default();
    #[cfg(target_arch = "aarch64")]
    {
        for name in ["SHA256SUMS-arm64.txt", "SHA256SUMS.txt"] {
            if let Some(hit) = assets.iter().find(|a| a.name.eq_ignore_ascii_case(name)) {
                out.sha256_url = Some(hit.url.clone());
                break;
            }
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        if let Some(hit) = assets
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case("SHA256SUMS.txt"))
        {
            out.sha256_url = Some(hit.url.clone());
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(hit) = find_asset_matching(
            assets,
            &["-linux-x86_64.tar.gz", "-linux-x64.tar.gz", ".tar.gz", ".appimage"],
            &["arm64", "aarch64"],
        ) {
            out.portable_url = Some(hit.url.clone());
            out.portable_name = Some(hit.name.clone());
            out.download_url = Some(hit.url.clone());
            out.asset_name = Some(hit.name.clone());
        }
        out
    }
    #[cfg(not(target_os = "linux"))]
    {
        #[cfg(target_arch = "aarch64")]
        let (excluded, portable_suffixes, setup_suffixes): (&[&str], &[&str], &[&str]) = (
            &["x64", "x86_64", "amd64"],
            &[
                "-windows-arm64-portable.zip",
                "-arm64-portable.zip",
                "-arm64.zip",
                "-portable.zip",
                ".zip",
            ],
            &[
                "-windows-arm64-setup.exe",
                "-arm64-setup.exe",
                "-setup.exe",
                ".exe",
                ".msi",
                ".zip",
            ],
        );
        #[cfg(not(target_arch = "aarch64"))]
        let (excluded, portable_suffixes, setup_suffixes): (&[&str], &[&str], &[&str]) = (
            &["arm64", "aarch64"],
            &[
                "-windows-x64-portable.zip",
                "-x64-portable.zip",
                "-portable.zip",
                ".zip",
            ],
            &["-windows-x64-setup.exe", "-setup.exe", ".exe", ".msi", ".zip"],
        );

        if let Some(hit) = find_asset_matching(assets, portable_suffixes, excluded)
            .or_else(|| find_asset_matching(assets, portable_suffixes, &[]))
        {
            out.portable_url = Some(hit.url.clone());
            out.portable_name = Some(hit.name.clone());
        }
        if let Some(hit) = find_asset_matching(assets, setup_suffixes, excluded)
            .or_else(|| find_asset_matching(assets, setup_suffixes, &[]))
        {
            out.download_url = Some(hit.url.clone());
            out.asset_name = Some(hit.name.clone());
        }
        if detect_install_kind() == InstallKind::Portable && out.portable_url.is_some() {
            out.download_url = out.portable_url.clone();
            out.asset_name = out.portable_name.clone();
        }
        out
    }
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
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("open `{}`: {e}", path.display()))?;
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

fn choose_download_target(release: &ReleaseInfo) -> Option<(&str, &str)> {
    if detect_install_kind() == InstallKind::Portable
        && let Some(purl) = release.portable_url.as_deref() {
            let pname = release
                .portable_name
                .as_deref()
                .unwrap_or("Tomoyo-portable-update.zip");
            return Some((purl, pname));
        }
    release.download_url.as_deref().map(|u| {
        let n = release
            .asset_name
            .as_deref()
            .unwrap_or("Tomoyo-update-setup.exe");
        (u, n)
    })
}

pub fn download_and_verify_release_asset(release: &ReleaseInfo) -> Result<PathBuf, String> {
    download_and_verify_release_asset_with_progress(release, |_| {})
}

/// Streams release installer/portable package to `%TEMP%\tomoyo-update\` and verifies SHA-256.
pub fn download_and_verify_release_asset_with_progress<F>(
    release: &ReleaseInfo,
    mut on_progress: F,
) -> Result<PathBuf, String>
where
    F: FnMut(UpdateInstallProgress),
{
    let (url, raw_name) =
        choose_download_target(release).ok_or_else(|| "Release 不包含可下载的安装包资源".to_string())?;
    let safe_name = Path::new(raw_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Tomoyo-update-setup.exe")
        .to_string();

    on_progress(UpdateInstallProgress {
        stage: "checksum".into(),
        message: "正在获取 SHA-256 官方校验清单...".into(),
        downloaded_bytes: 0,
        total_bytes: None,
        percent: 2,
        asset_name: safe_name.clone(),
    });

    let expected_sha = if let Some(sums_url) = &release.sha256_url {
        let resp = http_get(sums_url, 12_000, 128 * 1024)
            .or_else(|_| http_get(sums_url, 15_000, 128 * 1024))?;
        if resp.status_code == 200 {
            parse_sha256_for_asset(&resp.text_lossy(), &safe_name)
        } else if resp.status_code == 404 {
            None
        } else {
            return Err(format!("获取 SHA256SUMS.txt 失败（HTTP {}）", resp.status_code));
        }
    } else {
        None
    };

    let dest_dir = std::env::temp_dir().join("tomoyo-update");
    let _ = std::fs::create_dir_all(&dest_dir);
    let dest_path = dest_dir.join(&safe_name);

    let asset_for_cb = safe_name.clone();
    let _ = http_download_to_file_with_progress(
        url,
        &dest_path,
        60_000,
        160 * 1024 * 1024,
        |dl, tot| {
            let pct = match tot {
                Some(t) if t > 0 => ((dl * 100 / t) as u8).clamp(3, 95),
                _ => 15,
            };
            let dl_mb = dl as f64 / (1024.0 * 1024.0);
            let msg = match tot {
                Some(t) if t > 0 => {
                    let tot_mb = t as f64 / (1024.0 * 1024.0);
                    format!("正在下载 {asset_for_cb} ({dl_mb:.1} / {tot_mb:.1} MB · {pct}%)")
                }
                _ => format!("正在下载 {asset_for_cb} ({dl_mb:.1} MB)..."),
            };
            on_progress(UpdateInstallProgress {
                stage: "downloading".into(),
                message: msg,
                downloaded_bytes: dl,
                total_bytes: tot,
                percent: pct,
                asset_name: asset_for_cb.clone(),
            });
        },
    )?;

    on_progress(UpdateInstallProgress {
        stage: "verifying".into(),
        message: "正在校验安装包 SHA-256 完整性...".into(),
        downloaded_bytes: 0,
        total_bytes: None,
        percent: 97,
        asset_name: safe_name.clone(),
    });

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

    on_progress(UpdateInstallProgress {
        stage: "launching".into(),
        message: "校验通过！正在应用更新并自动重启 Tomoyo...".into(),
        downloaded_bytes: 0,
        total_bytes: None,
        percent: 100,
        asset_name: safe_name,
    });
    Ok(dest_path)
}

#[cfg(windows)]
pub fn launch_verified_update_installer(installer_path: &Path) -> Result<bool, String> {
    win::launch_windows_update(installer_path)
}

#[cfg(target_os = "linux")]
pub fn launch_verified_update_installer(installer_path: &Path) -> Result<bool, String> {
    super::installer_linux::launch_linux_update(installer_path)
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn launch_verified_update_installer(_installer_path: &Path) -> Result<bool, String> {
    Err("Self-update installer is only supported on Windows and Linux".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_release_assets_prefers_current_arch_portable_and_setup() {
        let assets = vec![
            CandidateAsset {
                name: "SHA256SUMS-arm64.txt".into(),
                url: "https://example.invalid/SHA256SUMS-arm64.txt".into(),
            },
            CandidateAsset {
                name: "SHA256SUMS.txt".into(),
                url: "https://example.invalid/SHA256SUMS.txt".into(),
            },
            CandidateAsset {
                name: "Tomoyo-v2.3.0-windows-arm64-portable.zip".into(),
                url: "https://example.invalid/arm64.zip".into(),
            },
            CandidateAsset {
                name: "Tomoyo-v2.3.0-windows-x64-portable.zip".into(),
                url: "https://example.invalid/x64.zip".into(),
            },
            CandidateAsset {
                name: "Tomoyo-v2.3.0-windows-x64-setup.exe".into(),
                url: "https://example.invalid/setup.exe".into(),
            },
            CandidateAsset {
                name: "Tomoyo-v2.3.0-linux-x86_64.tar.gz".into(),
                url: "https://example.invalid/linux.tar.gz".into(),
            },
        ];
        let picked = pick_release_assets(&assets);
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                picked.asset_name.as_deref(),
                Some("Tomoyo-v2.3.0-linux-x86_64.tar.gz")
            );
        }
        #[cfg(all(not(target_os = "linux"), not(target_arch = "aarch64")))]
        {
            assert_eq!(
                picked.portable_name.as_deref(),
                Some("Tomoyo-v2.3.0-windows-x64-portable.zip")
            );
            assert_eq!(
                picked.sha256_url.as_deref(),
                Some("https://example.invalid/SHA256SUMS.txt")
            );
            assert_eq!(
                picked.asset_name.as_deref(),
                Some("Tomoyo-v2.3.0-windows-x64-setup.exe")
            );
        }
        #[cfg(all(not(target_os = "linux"), target_arch = "aarch64"))]
        {
            assert_eq!(
                picked.portable_name.as_deref(),
                Some("Tomoyo-v2.3.0-windows-arm64-portable.zip")
            );
            assert_eq!(
                picked.sha256_url.as_deref(),
                Some("https://example.invalid/SHA256SUMS-arm64.txt")
            );
        }
    }
}
