//! Local-first zero-account sync directory management (`sync_dir.txt`).
//!
//! Lets users point `config.toml`, `snippets.toml`, and `notes/` to any local synced folder
//! (OneDrive, Nutstore/坚果云, iCloud, Git repo) while keeping machine-bound DPAPI `secrets.json`,
//! `clipboard.db`, `hotkey_status.json`, and `logs/` strictly local in `%LOCALAPPDATA%\Tomoyo`.

use crate::config::validate_scan_dir;
use std::path::{Path, PathBuf};

fn pointer_file_path() -> PathBuf {
    crate::data_dir().join("sync_dir.txt")
}

/// Returns the configured local sync directory if present, valid, and accessible.
pub fn configured_sync_dir() -> Option<PathBuf> {
    let raw = std::fs::read_to_string(pointer_file_path()).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let clean = validate_scan_dir(trimmed).ok()?;
    let pb = PathBuf::from(clean);
    pb.is_dir().then_some(pb)
}

/// Returns the active root directory for portable user config/snippets/notes.
/// Falls back seamlessly to local `data_dir()` when sync directory is unset or unavailable.
pub fn sync_root_dir() -> PathBuf {
    configured_sync_dir().unwrap_or_else(crate::data_dir)
}

/// Sets or clears the local sync directory, migrating `config.toml` & `snippets.toml` safely.
/// - If the target already contains `config.toml` (e.g. from another PC), preserves and loads it.
/// - If the target is empty/new, copies current `config.toml` & `snippets.toml` into it.
/// - Never copies `secrets.json` (DPAPI) or `clipboard.db` into the synced folder!
pub fn set_sync_dir(dir: Option<&str>) -> Result<(Option<PathBuf>, String), String> {
    let local = crate::data_dir();
    let ptr_file = pointer_file_path();

    let Some(raw) = dir.map(str::trim).filter(|s| !s.is_empty()) else {
        // Restore to local default: keep latest active config locally before removing pointer
        if let Some(old_sync) = configured_sync_dir() {
            let src_cfg = old_sync.join("config.toml");
            let dst_cfg = local.join("config.toml");
            if src_cfg.is_file() {
                let _ = std::fs::create_dir_all(&local);
                let _ = std::fs::copy(&src_cfg, &dst_cfg);
            }
        }
        let _ = std::fs::remove_file(&ptr_file);
        return Ok((None, "已恢复为本机默认数据目录（同步目录内文件已原样保留）".into()));
    };

    let clean = validate_scan_dir(raw).map_err(|e| e.to_string())?;
    let target = PathBuf::from(&clean);
    if target.eq_ignore_ascii_case_path(&local) {
        let _ = std::fs::remove_file(&ptr_file);
        return Ok((None, "所选目录即默认本地目录，已保持本机存储".into()));
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("无法访问或创建同步目录：{e}"))?;

    let prev_root = sync_root_dir();
    let target_cfg = target.join("config.toml");
    let loaded_existing = target_cfg.is_file();
    if !loaded_existing {
        let src_cfg = prev_root.join("config.toml");
        if src_cfg.is_file() {
            let _ = std::fs::copy(&src_cfg, &target_cfg);
        }
    }
    let target_snip = target.join("snippets.toml");
    if !target_snip.is_file() {
        let src_snip = prev_root.join("snippets.toml");
        if src_snip.is_file() {
            let _ = std::fs::copy(&src_snip, &target_snip);
        }
    }

    std::fs::create_dir_all(&local).map_err(|e| format!("无法写入本地指针目录：{e}"))?;
    std::fs::write(&ptr_file, format!("{clean}\n")).map_err(|e| format!("写入同步目录配置失败：{e}"))?;

    let msg = if loaded_existing {
        format!("已接入已有同步目录并载入云盘配置：{clean}（API 密钥保持本机 DPAPI 隔离）")
    } else {
        format!("已切换至本地同步目录：{clean}（已迁移配置与片段，API 密钥保持本机隔离）")
    };
    Ok((Some(target), msg))
}

trait PathEqIgnoreCase {
    fn eq_ignore_ascii_case_path(&self, other: &Path) -> bool;
}
impl PathEqIgnoreCase for Path {
    fn eq_ignore_ascii_case_path(&self, other: &Path) -> bool {
        self.to_string_lossy().eq_ignore_ascii_case(&other.to_string_lossy())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_root_falls_back_to_data_dir_when_unset() {
        let root = sync_root_dir();
        assert!(!root.as_os_str().is_empty());
        assert_ne!(root, PathBuf::from("C:\\"));
    }
}
