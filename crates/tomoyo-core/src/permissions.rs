//! Persistent capability permission store for third-party plugins (`permissions.json`).
//!
//! Sensitive capabilities (`FilesystemRead`, `FilesystemWrite`, `Network`, `Clipboard`,
//! `ShellExec`) require explicit approval before execution unless already bundled/trusted,
//! and users can grant or revoke any capability live in Settings Center.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tomoyo_protocol::Permission;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginPermissionRecord {
    #[serde(default)]
    pub approved: BTreeSet<String>,
    #[serde(default)]
    pub denied: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionStore {
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginPermissionRecord>,
}

pub fn permission_requires_approval(p: Permission) -> bool {
    matches!(
        p,
        Permission::FilesystemRead
            | Permission::FilesystemWrite
            | Permission::Network
            | Permission::Clipboard
            | Permission::ShellExec
    )
}

pub fn permission_key(p: Permission) -> &'static str {
    match p {
        Permission::FilesystemRead => "filesystem_read",
        Permission::FilesystemWrite => "filesystem_write",
        Permission::Network => "network",
        Permission::Clipboard => "clipboard",
        Permission::ShellExec => "shell_exec",
        Permission::SystemInfo => "system_info",
    }
}

pub fn permission_from_key(k: &str) -> Option<Permission> {
    match k.trim() {
        "filesystem_read" => Some(Permission::FilesystemRead),
        "filesystem_write" => Some(Permission::FilesystemWrite),
        "network" => Some(Permission::Network),
        "clipboard" => Some(Permission::Clipboard),
        "shell_exec" => Some(Permission::ShellExec),
        "system_info" => Some(Permission::SystemInfo),
        _ => None,
    }
}

pub fn permission_label_zh(p: Permission) -> &'static str {
    match p {
        Permission::FilesystemRead => "读取本地文件 (FilesystemRead)",
        Permission::FilesystemWrite => "写入/修改本地文件 (FilesystemWrite)",
        Permission::Network => "访问网络请求 (Network)",
        Permission::Clipboard => "读写系统剪贴板 (Clipboard)",
        Permission::ShellExec => "执行系统命令/子进程 (ShellExec)",
        Permission::SystemInfo => "读取系统基础信息 (SystemInfo)",
    }
}

impl PermissionStore {
    pub fn store_path() -> PathBuf {
        crate::data_dir().join("permissions.json")
    }

    pub fn load_default() -> Self {
        Self::load_from(&Self::store_path())
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save_default(&self) -> Result<(), String> {
        self.save_to(&Self::store_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Evaluates which declared permissions still require prompt, and which are currently granted.
    pub fn pending_approval(
        &self,
        plugin_id: &str,
        declared: &[Permission],
        bundled_trusted: bool,
    ) -> Vec<Permission> {
        let rec = self.plugins.get(plugin_id);
        let mut pending = Vec::new();
        for &perm in declared {
            if !permission_requires_approval(perm) {
                continue;
            }
            let key = permission_key(perm);
            let explicitly_approved = rec.is_some_and(|r| r.approved.contains(key));
            let explicitly_denied = rec.is_some_and(|r| r.denied.contains(key));
            if !explicitly_approved && !explicitly_denied && !bundled_trusted {
                pending.push(perm);
            }
        }
        pending
    }

    pub fn effective_granted(
        &self,
        plugin_id: &str,
        declared: &[Permission],
        bundled_trusted: bool,
    ) -> Vec<Permission> {
        let rec = self.plugins.get(plugin_id);
        let mut out = Vec::new();
        for &perm in declared {
            if !permission_requires_approval(perm) {
                out.push(perm);
                continue;
            }
            let key = permission_key(perm);
            let explicitly_denied = rec.is_some_and(|r| r.denied.contains(key));
            let explicitly_approved = rec.is_some_and(|r| r.approved.contains(key));
            if !explicitly_denied && (bundled_trusted || explicitly_approved) {
                out.push(perm);
            }
        }
        out
    }

    pub fn set_permission(&mut self, plugin_id: &str, perm: Permission, allow: bool) {
        let rec = self.plugins.entry(plugin_id.to_string()).or_default();
        let key = permission_key(perm).to_string();
        if allow {
            rec.denied.remove(&key);
            rec.approved.insert(key);
        } else {
            rec.approved.remove(&key);
            rec.denied.insert(key);
        }
    }

    pub fn is_permission_allowed(
        &self,
        plugin_id: &str,
        perm: Permission,
        bundled_trusted: bool,
    ) -> bool {
        self.effective_granted(plugin_id, &[perm], bundled_trusted)
            .contains(&perm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_approval_and_revocation_state() {
        let mut store = PermissionStore::default();
        let declared = [Permission::FilesystemRead, Permission::Network, Permission::SystemInfo];
        assert_eq!(
            store.pending_approval("com.thirdparty.tool", &declared, false),
            vec![Permission::FilesystemRead, Permission::Network]
        );
        assert_eq!(
            store.effective_granted("com.thirdparty.tool", &declared, false),
            vec![Permission::SystemInfo]
        );

        store.set_permission("com.thirdparty.tool", Permission::FilesystemRead, true);
        assert!(store.is_permission_allowed("com.thirdparty.tool", Permission::FilesystemRead, false));

        // Revoking even on a bundled plugin blocks that permission deterministically.
        store.set_permission("dev.tomoyo.example-wasm", Permission::FilesystemRead, false);
        assert!(!store.is_permission_allowed("dev.tomoyo.example-wasm", Permission::FilesystemRead, true));
    }
}
