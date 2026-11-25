//! Linux release asset self-update installer (tar.gz atomic binary + plugin sync & auto-relaunch).

use crate::diagnostics::{format_beijing_now, logs_dir};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn append_linux_update_log(msg: &str) {
    let path = logs_dir().join("update.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {msg}", format_beijing_now());
    }
}

pub(super) fn build_linux_apply_script(
    install_dir: &Path,
    data_dir: &Path,
    stage_dir: &Path,
    archive_path: &Path,
    target_exe: &Path,
    log_file: &Path,
) -> String {
    format!(
        "#!/usr/bin/env sh\n\
         set -eu\n\
         LOG=\"{log}\"\n\
         log() {{ printf '%s [linux-updater] %s\\n' \"$(date '+%Y-%m-%d %H:%M:%S')\" \"$1\" >> \"$LOG\" 2>/dev/null || true; }}\n\
         log \"Updater started (PID=$$)\"\n\
         sleep 0.45\n\
         if command -v systemctl >/dev/null 2>&1; then\n\
           for u in app-tomoyo@launcher.service tomoyo-launcher.service tomoyo-tray.service; do\n\
             systemctl --user stop \"$u\" >/dev/null 2>&1 || true\n\
             systemctl --user reset-failed \"$u\" >/dev/null 2>&1 || true\n\
           done\n\
         fi\n\
         pkill -x tomoyo >/dev/null 2>&1 || true\n\
         pkill -x tomoyo-capture >/dev/null 2>&1 || true\n\
         pkill -f '(^|/)tomoyo-(webview-host|plugin-[a-z0-9-]+)([[:space:]]|$)' >/dev/null 2>&1 || true\n\
         sleep 0.25\n\
         mkdir -p \"{inst}\" \"{data}/fonts\" \"{data}/plugin-icons\" \"{data}/assets\" \"{data}/registry\" \"{data}/plugins\"\n\
         updated_bins=0\n\
         for src in \"{stage}\"/tomoyo*; do\n\
           if [ -f \"$src\" ] && [ -x \"$src\" ]; then\n\
             name=$(basename \"$src\")\n\
             install -m 0755 \"$src\" \"{inst}/$name\"\n\
             updated_bins=$((updated_bins + 1))\n\
           fi\n\
         done\n\
         [ -d \"{stage}/assets/fonts\" ] && cp -af \"{stage}/assets/fonts/.\" \"{data}/fonts/\" 2>/dev/null || true\n\
         [ -d \"{stage}/assets/plugin-icons\" ] && cp -af \"{stage}/assets/plugin-icons/.\" \"{data}/plugin-icons/\" 2>/dev/null || true\n\
         [ -f \"{stage}/assets/tomoyo-icon.png\" ] && install -m 0644 \"{stage}/assets/tomoyo-icon.png\" \"{data}/assets/tomoyo-icon.png\" || true\n\
         [ -d \"{stage}/registry\" ] && cp -af \"{stage}/registry/.\" \"{data}/registry/\" 2>/dev/null || true\n\
         if [ -d \"{stage}/plugins\" ]; then\n\
           for pdir in \"{stage}/plugins\"/builtin-* \"{stage}/plugins\"/official-*; do\n\
             if [ -d \"$pdir\" ]; then\n\
               pname=$(basename \"$pdir\")\n\
               rm -rf \"{data}/plugins/$pname\"\n\
               cp -a \"$pdir\" \"{data}/plugins/$pname\"\n\
             fi\n\
           done\n\
         fi\n\
         rm -rf \"{stage}\" \"{arch}\"\n\
         log \"Installed $updated_bins binary(ies); relaunching {exe} --updated\"\n\
         if [ -x \"{exe}\" ]; then\n\
           if ! (command -v systemd-run >/dev/null 2>&1 && systemd-run --user --unit=app-tomoyo@launcher.service --collect --quiet --slice=app.slice --description=\"Tomoyo\" --property=ExitType=cgroup --setenv=MALLOC_ARENA_MAX=2 --working-directory=\"${{HOME:-/}}\" -- \"{exe}\" --updated >/dev/null 2>&1); then\n\
             MALLOC_ARENA_MAX=2 nohup \"{exe}\" --updated </dev/null >/dev/null 2>&1 &\n\
           fi\n\
           log \"Relaunch dispatched cleanly\"\n\
         else\n\
           log \"ERROR: target executable not found at {exe}\"\n\
         fi\n",
        log = log_file.display(),
        inst = install_dir.display(),
        data = data_dir.display(),
        stage = stage_dir.display(),
        arch = archive_path.display(),
        exe = target_exe.display(),
    )
}

pub(super) fn launch_linux_update(installer_path: &Path) -> Result<bool, String> {
    let lower = installer_path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        let stage_dir = std::env::temp_dir().join("tomoyo-update").join("linux_stage");
        let _ = std::fs::remove_dir_all(&stage_dir);
        std::fs::create_dir_all(&stage_dir)
            .map_err(|e| format!("create update stage dir: {e}"))?;

        append_linux_update_log(&format!("Extracting Linux archive {}", installer_path.display()));
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
            append_linux_update_log("Archive extraction failed (tar returned non-zero)");
            return Err("更新包解压失败（tar 返回非零状态）".into());
        }
        if !stage_dir.join("tomoyo").is_file() {
            append_linux_update_log("Archive missing top-level tomoyo binary; aborted");
            return Err("更新包中未找到 tomoyo 主程序，已取消覆盖".into());
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

        let data_dir = crate::data_dir();
        let log_file = logs_dir().join("update.log");
        let script = std::env::temp_dir().join("tomoyo-update").join("apply_linux_update.sh");
        let body = build_linux_apply_script(
            &install_dir,
            &data_dir,
            &stage_dir,
            installer_path,
            &target_exe,
            &log_file,
        );
        std::fs::write(&script, body).map_err(|e| format!("write update script: {e}"))?;
        let script_str = script.to_string_lossy().into_owned();
        append_linux_update_log(&format!(
            "Spawning isolated updater script {} -> {}",
            script.display(),
            install_dir.display()
        ));
        if !crate::linux_launch::spawn_isolated("sh", &[script_str], None, Some("tomoyo-updater")) {
            append_linux_update_log("Failed to spawn isolated updater script");
            return Err("拉起更新脚本失败".into());
        }
        return Ok(true);
    }

    if let Some(parent) = installer_path.parent() {
        let _ = crate::linux_launch::spawn_isolated(
            "xdg-open",
            &[parent.to_string_lossy().into_owned()],
            None,
            Some("xdg-open"),
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_apply_script_syncs_all_binaries_plugins_and_restarts() {
        let script = build_linux_apply_script(
            Path::new("/home/u/.local/bin"),
            Path::new("/home/u/.local/share/tomoyo"),
            Path::new("/tmp/tomoyo-update/linux_stage"),
            Path::new("/tmp/tomoyo-update/Tomoyo-v3.2.0-linux-x86_64.tar.gz"),
            Path::new("/home/u/.local/bin/tomoyo"),
            Path::new("/home/u/.local/share/tomoyo/logs/update.log"),
        );
        assert!(script.contains("tomoyo-(webview-host|plugin-"));
        assert!(script.contains("for u in app-tomoyo@launcher.service"));
        assert!(script.contains("official-*"));
        assert!(script.contains("update.log"));
        assert!(script.contains("--updated"));
        assert!(script.contains("systemd-run --user"));
    }
}
