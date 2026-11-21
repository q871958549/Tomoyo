//! Install-mode detection, SHA-256 checksum download & zero-gap self-upgrade + auto-relaunch.

use super::ReleaseInfo;
use crate::http::{http_download_to_file_with_progress, http_get};
#[cfg(windows)]
use crate::pkg_zip::unpack_zip_to_dir;
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

pub fn pick_release_assets(assets: &[CandidateAsset]) -> PickedAssets {
    let mut out = PickedAssets::default();
    for a in assets {
        if a.name.eq_ignore_ascii_case("SHA256SUMS.txt") {
            out.sha256_url = Some(a.url.clone());
        }
    }
    for suffix in ["-portable.zip", ".zip"] {
        if let Some(hit) = assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(suffix))
        {
            out.portable_url = Some(hit.url.clone());
            out.portable_name = Some(hit.name.clone());
            break;
        }
    }
    for suffix in ["-setup.exe", ".exe", ".msi", ".zip"] {
        if let Some(hit) = assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(suffix))
        {
            out.download_url = Some(hit.url.clone());
            out.asset_name = Some(hit.name.clone());
            break;
        }
    }
    if detect_install_kind() == InstallKind::Portable && out.portable_url.is_some() {
        out.download_url = out.portable_url.clone();
        out.asset_name = out.portable_name.clone();
    }
    out
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
fn write_update_helper_ps1(is_portable: bool, is_msi: bool) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("tomoyo-update");
    let _ = std::fs::create_dir_all(&dir);
    let script_path = dir.join(if is_portable {
        "apply_portable_update.ps1"
    } else {
        "apply_setup_update.ps1"
    });
    let body = if is_portable {
        r#"param([string]$StageDir,[string]$InstallDir,[string]$TargetExe,[string]$ArchivePath)
Start-Sleep -Milliseconds 350
foreach ($name in @('tomoyo-webview-host','tomoyo')) {
  $deadline = (Get-Date).AddSeconds(6)
  while ((Get-Process -Name $name -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 150 }
  Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}
try {
  Copy-Item -Path (Join-Path $StageDir '*') -Destination $InstallDir -Recurse -Force
  Remove-Item -Path $StageDir -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item -Path $ArchivePath -Force -ErrorAction SilentlyContinue
  if (Test-Path $TargetExe) { Start-Process -FilePath $TargetExe -ArgumentList '--updated' }
} catch {
  if (Test-Path $TargetExe) { Start-Process -FilePath $TargetExe -ArgumentList '--update-failed' }
}
"#
    } else if is_msi {
        r#"param([string]$Installer,[string]$TargetExe,[string]$FallbackExe)
Start-Sleep -Milliseconds 350
$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', "`"$Installer`"", '/qn', '/norestart') -Wait -PassThru
Remove-Item -Path $Installer -Force -ErrorAction SilentlyContinue
$launch = if ($p.ExitCode -eq 0 -and (Test-Path $TargetExe)) { $TargetExe } else { $FallbackExe }
$arg = if ($p.ExitCode -eq 0) { '--updated' } else { '--update-failed' }
if ($launch -and (Test-Path $launch) -and -not (Get-Process -Name 'tomoyo' -ErrorAction SilentlyContinue)) {
  Start-Process -FilePath $launch -ArgumentList $arg
}
"#
    } else {
        r#"param([string]$Installer,[string]$TargetExe,[string]$FallbackExe)
Start-Sleep -Milliseconds 350
foreach ($name in @('tomoyo-webview-host','tomoyo')) {
  $deadline = (Get-Date).AddSeconds(5)
  while ((Get-Process -Name $name -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 150 }
  Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}
$p = Start-Process -FilePath $Installer -ArgumentList @('/SP-','/VERYSILENT','/NORESTART','/CLOSEAPPLICATIONS') -Wait -PassThru
Remove-Item -Path $Installer -Force -ErrorAction SilentlyContinue
$launch = if ($p.ExitCode -eq 0 -and (Test-Path $TargetExe)) { $TargetExe } elseif (Test-Path $FallbackExe) { $FallbackExe } else { $TargetExe }
$arg = if ($p.ExitCode -eq 0) { '--updated' } else { '--update-failed' }
if ($launch -and (Test-Path $launch) -and -not (Get-Process -Name 'tomoyo' -ErrorAction SilentlyContinue)) {
  Start-Process -FilePath $launch -ArgumentList $arg
}
"#
    };
    std::fs::write(&script_path, body).map_err(|e| format!("write update helper failed: {e}"))?;
    Ok(script_path)
}

#[cfg(windows)]
pub fn launch_verified_update_installer(installer_path: &Path) -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let ext = installer_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("tomoyo.exe"));
    let exe_dir = current_exe
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let fallback_exe = if current_exe
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("tomoyo.exe"))
    {
        current_exe.clone()
    } else {
        exe_dir.join("tomoyo.exe")
    };
    let default_setup_exe = crate::data_dir()
        .parent()
        .map(|l| l.join("Programs").join("Tomoyo").join("tomoyo.exe"))
        .unwrap_or_else(|| fallback_exe.clone());

    if ext == "zip" {
        let stage_dir = std::env::temp_dir().join("tomoyo-update").join("portable_stage");
        let _ = std::fs::remove_dir_all(&stage_dir);
        unpack_zip_to_dir(installer_path, &stage_dir)?;
        if !stage_dir.join("tomoyo.exe").is_file() {
            return Err("便携压缩包中未包含 tomoyo.exe，已取消覆盖".into());
        }
        let script = write_update_helper_ps1(true, false)?;
        std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script.to_string_lossy(),
                "-StageDir",
                &stage_dir.to_string_lossy(),
                "-InstallDir",
                &exe_dir.to_string_lossy(),
                "-TargetExe",
                &fallback_exe.to_string_lossy(),
                "-ArchivePath",
                &installer_path.to_string_lossy(),
            ])
            .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("拉起便携更新器失败: {e}"))?;
        return Ok(true);
    }

    if ext == "exe" || ext == "msi" {
        let script = write_update_helper_ps1(false, ext == "msi")?;
        std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script.to_string_lossy(),
                "-Installer",
                &installer_path.to_string_lossy(),
                "-TargetExe",
                &default_setup_exe.to_string_lossy(),
                "-FallbackExe",
                &fallback_exe.to_string_lossy(),
            ])
            .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("拉起安装更新器失败: {e}"))?;
        return Ok(true);
    }

    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", installer_path.display()))
        .spawn();
    Ok(false)
}

#[cfg(not(windows))]
pub fn launch_verified_update_installer(_installer_path: &Path) -> Result<bool, String> {
    Err("Self-update installer is only supported on Windows".into())
}
