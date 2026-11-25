//! Self-contained SHA-256 verified Quiet.md installer (zero coupling with `tomoyo-core`).

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const QUIET_MD_REPO: &str = "s1dashu/quiet-md";

fn managed_tools_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".local/share"))
                .unwrap_or_else(|_| PathBuf::from("."))
        });
    if cfg!(windows) {
        base.join("Tomoyo").join("tools").join("quiet-md")
    } else {
        base.join("tomoyo").join("tools").join("quiet-md")
    }
}

fn curl_cmd() -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

pub(crate) fn extract_tag_from_url(url: &str) -> Option<String> {
    let (_, rest) = url.rsplit_once("/tag/")?;
    let tag = rest.split(['?', '#', '/']).next()?.trim();
    (!tag.is_empty()).then(|| tag.to_string())
}

pub(crate) fn parse_sha256_for_asset(sums: &str, asset_name: &str) -> Option<String> {
    for line in sums.lines() {
        let mut parts = line.split_whitespace();
        let Some(hex) = parts.next() else { continue };
        let Some(name_raw) = parts.next() else { continue };
        let name = name_raw.trim_start_matches('*');
        if name.eq_ignore_ascii_case(asset_name)
            && hex.len() == 64
            && hex.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Some(hex.to_ascii_lowercase());
        }
    }
    None
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|e| format!("读取下载文件失败: {e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn resolve_latest_tag() -> Result<String, String> {
    let sink = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = curl_cmd()
        .args([
            "-fsSLI",
            "--max-time",
            "10",
            "-o",
            sink,
            "-w",
            "%{url_effective}",
            &format!("https://github.com/{QUIET_MD_REPO}/releases/latest"),
        ])
        .output()
        .map_err(|e| format!("调用 curl 获取 Quiet.md 版本失败: {e}"))?;
    if !out.status.success() {
        return Err("无法访问 GitHub 获取 Quiet.md 最新版本".into());
    }
    let final_url = String::from_utf8_lossy(&out.stdout);
    extract_tag_from_url(final_url.trim())
        .ok_or_else(|| "未能从 Quiet.md Release 链接解析版本号".into())
}

pub fn install_quiet_md_verified() -> Result<String, String> {
    let tag = resolve_latest_tag()?;
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
    let sums_out = curl_cmd()
        .args(["-fsSL", "--max-time", "15", &format!("{base}/SHA256SUMS")])
        .output()
        .map_err(|e| format!("下载 Quiet.md SHA256SUMS 失败: {e}"))?;
    if !sums_out.status.success() {
        return Err("下载 Quiet.md SHA256SUMS 返回失败".into());
    }
    let sums_text = String::from_utf8_lossy(&sums_out.stdout);
    let expected = parse_sha256_for_asset(&sums_text, &asset_name)
        .ok_or_else(|| format!("SHA256SUMS 中未找到 `{asset_name}`"))?;

    let tools_dir = managed_tools_dir();
    fs::create_dir_all(&tools_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let target = tools_dir.join(&asset_name);
    let dl_status = curl_cmd()
        .args([
            "-fsSL",
            "--max-time",
            "120",
            "-o",
            &target.to_string_lossy(),
            &format!("{base}/{asset_name}"),
        ])
        .status()
        .map_err(|e| format!("下载 Quiet.md 安装包失败: {e}"))?;
    if !dl_status.success() {
        let _ = fs::remove_file(&target);
        return Err("下载 Quiet.md 安装包中断".into());
    }

    let actual = sha256_file(&target)?;
    if !actual.eq_ignore_ascii_case(&expected) {
        let _ = fs::remove_file(&target);
        return Err(format!("Quiet.md SHA-256 不匹配：期望 {}，实际 {}", &expected[..12], &actual[..12]));
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let st = Command::new(&target)
            .args([
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CURRENTUSER",
                &format!("/DIR={}", tools_dir.display()),
            ])
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| format!("运行 Quiet.md 安装包失败: {e}"))?;
        let _ = fs::remove_file(&target);
        if !st.success() {
            return Err(format!("Quiet.md 安装包返回异常状态码: {:?}", st.code()));
        }
        Ok(format!("Quiet.md v{ver} 已校验并安装至 {}", tools_dir.display()))
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let dest = tools_dir.join("quiet-md.AppImage");
        if target != dest {
            fs::rename(&target, &dest).map_err(|e| e.to_string())?;
        }
        let mut perms = fs::metadata(&dest).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest, perms).map_err(|e| e.to_string())?;
        Ok(format!("Quiet.md v{ver} 已校验并安装至 {}", dest.display()))
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Ok(format!("Quiet.md v{ver} 已校验下载至 {}", target.display()))
    }
}

pub fn spawn_verified_install() {
    let _ = std::thread::Builder::new()
        .name("quietmd-plugin-install".into())
        .spawn(|| {
            if let Err(e) = install_quiet_md_verified() {
                eprintln!("[tomoyo-plugin-md-editor] Quiet.md install failed: {e}");
            }
        });
}

pub fn spawn_detached_editor(exe: &Path, arg: &str) {
    let build = || {
        let mut cmd = Command::new(exe);
        cmd.arg(arg);
        cmd
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        let mut cmd = build();
        cmd.creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP);
        if cmd.spawn().is_err() {
            let _ = build().spawn();
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = build();
        unsafe {
            cmd.pre_exec(|| {
                unsafe extern "C" { fn setsid() -> i32; }
                let _ = setsid();
                Ok(())
            });
        }
        let _ = cmd.spawn();
    }
    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        let _ = build().spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_tag_and_parses_sha256sums() {
        assert_eq!(
            extract_tag_from_url("https://github.com/s1dashu/quiet-md/releases/tag/v0.9.2"),
            Some("v0.9.2".into())
        );
        let hex = "a".repeat(64);
        let sums = format!("{hex}  quiet-md-0.9.2-linux-x86_64.AppImage\n");
        assert_eq!(
            parse_sha256_for_asset(&sums, "quiet-md-0.9.2-linux-x86_64.AppImage"),
            Some(hex)
        );
    }
}
