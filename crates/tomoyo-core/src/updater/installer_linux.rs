//! Linux release asset selection & self-update installer (tar.gz / AppImage / xdg-open).

use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn launch_linux_update(installer_path: &Path) -> Result<bool, String> {
    let lower = installer_path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        let stage_dir = std::env::temp_dir().join("tomoyo-update").join("linux_stage");
        let _ = std::fs::remove_dir_all(&stage_dir);
        std::fs::create_dir_all(&stage_dir)
            .map_err(|e| format!("create update stage dir: {e}"))?;

        let status = Command::new("tar")
            .args([
                "-xzf",
                &installer_path.to_string_lossy(),
                "-C",
                &stage_dir.to_string_lossy(),
            ])
            .status()
            .map_err(|e| format!("运行 tar 解压更新包失败: {e}"))?;
        if !status.success() {
            return Err("更新包解压失败（tar 返回非零状态）".into());
        }

        let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("tomoyo"));
        let install_dir = current_exe
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(default_linux_bin_dir);
        let target_exe = if current_exe
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "tomoyo")
        {
            current_exe
        } else {
            install_dir.join("tomoyo")
        };

        let script = std::env::temp_dir().join("tomoyo-update").join("apply_linux_update.sh");
        let body = format!(
            "#!/usr/bin/env sh\n\
             set -eu\n\
             sleep 0.4\n\
             pkill -x tomoyo-webview-host >/dev/null 2>&1 || true\n\
             pkill -x tomoyo >/dev/null 2>&1 || true\n\
             sleep 0.2\n\
             mkdir -p \"{inst}\"\n\
             for f in tomoyo tomoyo-webview-host tomoyo-capture tomoyo-plugin-ocr tomoyo-plugin-notes tomoyo-cli; do\n\
               src=$(find \"{stage}\" -maxdepth 3 -type f -name \"$f\" | head -n 1 || true)\n\
               if [ -n \"$src\" ]; then install -m 0755 \"$src\" \"{inst}/$f\"; fi\n\
             done\n\
             rm -rf \"{stage}\" \"{arch}\"\n\
             if [ -x \"{exe}\" ]; then nohup \"{exe}\" --updated >/dev/null 2>&1 & fi\n",
            inst = install_dir.display(),
            stage = stage_dir.display(),
            arch = installer_path.display(),
            exe = target_exe.display(),
        );
        std::fs::write(&script, body).map_err(|e| format!("write update script: {e}"))?;
        Command::new("sh")
            .arg(&script)
            .spawn()
            .map_err(|e| format!("拉起更新脚本失败: {e}"))?;
        return Ok(true);
    }

    if let Some(parent) = installer_path.parent() {
        let _ = Command::new("xdg-open").arg(parent).spawn();
    }
    Ok(false)
}

fn default_linux_bin_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local").join("bin")
    } else {
        PathBuf::from(".")
    }
}
