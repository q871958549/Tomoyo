//! SHA-256 verified Quiet.md companion Markdown editor installer & launcher helper.

use crate::http::{http_download_to_file, http_get, http_head_redirect};
use crate::plugin_assets::{detect_quiet_md_binary, quiet_md_tools_dir};
use crate::updater::installer::{compute_file_sha256, parse_sha256_for_asset};
use crate::updater::web_feed::extract_tag_from_release_url;
use std::path::PathBuf;

pub const QUIET_MD_REPO: &str = "s1dashu/quiet-md";

fn resolve_quiet_md_latest_tag() -> Result<String, String> {
    let latest_url = format!("https://github.com/{QUIET_MD_REPO}/releases/latest");
    if let Ok(resp) = http_head_redirect(&latest_url, 6000) {
        let loc = resp.location.as_deref().unwrap_or(&resp.final_url);
        if let Some(tag) = extract_tag_from_release_url(loc)
            .or_else(|| extract_tag_from_release_url(&resp.final_url))
        {
            return Ok(tag);
        }
    }
    let api_url = format!("https://api.github.com/repos/{QUIET_MD_REPO}/releases/latest");
    let resp = http_get(&api_url, 6000, 1024 * 1024)?;
    if resp.status_code == 200
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp.body)
        && let Some(tag) = v.get("tag_name").and_then(|s| s.as_str())
    {
        return Ok(tag.to_string());
    }
    Err("无法从 GitHub 获取 Quiet.md 最新版本信息".into())
}

/// Downloads Quiet.md from `s1dashu/quiet-md` Releases, verifies SHA-256 against `SHA256SUMS`,
/// and installs it cleanly under user space without polluting Tomoyo core binaries.
pub fn install_quiet_md_verified() -> Result<String, String> {
    let tag = resolve_quiet_md_latest_tag()?;
    let ver = tag.trim().trim_start_matches(['v', 'V']);
    let asset_name = if cfg!(target_os = "linux") {
        format!("quiet-md-{ver}-linux-x86_64.AppImage")
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            format!("quiet-md-{ver}-macos-arm64.zip")
        } else {
            format!("quiet-md-{ver}-macos-x86_64.zip")
        }
    } else {
        format!("quiet-md-{ver}-windows-x64-setup.exe")
    };

    let base = format!("https://github.com/{QUIET_MD_REPO}/releases/download/{tag}");
    let sums_url = format!("{base}/SHA256SUMS");
    let sums_resp = http_get(&sums_url, 8000, 256 * 1024)?;
    if sums_resp.status_code != 200 {
        return Err(format!("下载 Quiet.md SHA256SUMS 失败: HTTP {}", sums_resp.status_code));
    }
    let sums_text = String::from_utf8_lossy(&sums_resp.body);
    let expected_sha = parse_sha256_for_asset(&sums_text, &asset_name)
        .ok_or_else(|| format!("SHA256SUMS 中未找到 `{asset_name}` 摘要"))?;

    let tools_dir = quiet_md_tools_dir();
    std::fs::create_dir_all(&tools_dir).map_err(|e| format!("创建工具目录失败: {e}"))?;
    let target_file = tools_dir.join(&asset_name);
    let download_url = format!("{base}/{asset_name}");
    http_download_to_file(&download_url, &target_file, 120_000, 96 * 1024 * 1024)?;

    let actual_sha = compute_file_sha256(&target_file)?;
    if !actual_sha.eq_ignore_ascii_case(&expected_sha) {
        let _ = std::fs::remove_file(&target_file);
        return Err(format!(
            "Quiet.md SHA-256 校验失败！期望 {}，实际 {}",
            &expected_sha[..12],
            &actual_sha[..12]
        ));
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = std::process::Command::new(&target_file)
            .args([
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CURRENTUSER",
                &format!("/DIR={}", tools_dir.display()),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("运行 Quiet.md 安装包失败: {e}"))?;
        let _ = std::fs::remove_file(&target_file);
        if !status.success() {
            return Err(format!("Quiet.md 安装包返回异常状态码: {:?}", status.code()));
        }
        let installed = detect_quiet_md_binary(None)
            .unwrap_or_else(|| tools_dir.join("quiet-md.exe"));
        Ok(format!(
            "Quiet.md v{ver} 已通过 SHA-256 校验并安装 ({})",
            installed.display()
        ))
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let dest = tools_dir.join("quiet-md.AppImage");
        if target_file != dest {
            std::fs::rename(&target_file, &dest).map_err(|e| e.to_string())?;
        }
        let mut perms = std::fs::metadata(&dest).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dest, perms).map_err(|e| e.to_string())?;
        Ok(format!("Quiet.md v{ver} 已校验安装到 {}", dest.display()))
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Ok(format!("Quiet.md v{ver} 已校验下载至 {}", target_file.display()))
    }
}

/// Resolves preferred binary to open a `.md` file (`Quiet.md` first when installed, optional fallback).
pub fn resolve_markdown_open_target(path: &str, custom_bin: Option<&str>) -> (String, Option<String>) {
    if let Some(qm) = detect_quiet_md_binary(custom_bin) {
        (qm.to_string_lossy().into_owned(), Some(format!("\"{path}\"")))
    } else {
        (path.to_string(), None)
    }
}

pub fn default_markdown_docs_dir() -> PathBuf {
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let p = PathBuf::from(home).join("Documents").join("Markdown");
        if p.is_dir() {
            return p;
        }
    }
    crate::sync_root_dir().join("markdown")
}
