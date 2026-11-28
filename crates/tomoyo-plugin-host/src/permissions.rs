//! Runtime plugin capability authorization, interactive prompt & revocation enforcement.

use std::path::Path;
use tomoyo_core::{permission_key, permission_label_zh, PermissionStore};
use tomoyo_protocol::{Permission, PluginManifest};

const OFFICIAL_BUNDLED_DIRS: &[&str] = &[
    "builtin-ocr",
    "official-notes",
    "official-md-editor",
    "official-translate",
    "official-capture",
    "official-clipboard",
    "official-devtools-wasm",
    "official-regex-wasm",
    "official-winlayout",
    "official-ai",
];

fn normalize_cmp_path(p: &Path) -> String {
    let s = p.to_string_lossy().to_ascii_lowercase();
    s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string()
}

pub(crate) fn is_bundled_plugin_dir(dir: &Path) -> bool {
    let dir_str = normalize_cmp_path(dir);
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let bundled = normalize_cmp_path(&parent.join("plugins"));
        if dir_str.starts_with(&bundled) {
            return true;
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let local_plugins = normalize_cmp_path(&cwd.join("plugins"));
        if dir_str.starts_with(&local_plugins) {
            return true;
        }
    }
    #[cfg(unix)]
    if dir_str.starts_with("/usr/share/tomoyo/plugins")
        || dir_str.starts_with("/usr/local/share/tomoyo/plugins")
    {
        return true;
    }
    // Linux XDG installs place official Suite plugins in `$XDG_DATA_HOME/tomoyo/plugins/official-*`
    // while keeping third-party plugins in the same root untrusted by default.
    let data_plugins = normalize_cmp_path(&tomoyo_core::data_dir().join("plugins"));
    if let (Some(parent), Some(leaf)) = (dir.parent(), dir.file_name().and_then(|s| s.to_str()))
        && normalize_cmp_path(parent) == data_plugins
        && OFFICIAL_BUNDLED_DIRS.contains(&leaf.to_ascii_lowercase().as_str())
    {
        return true;
    }
    false
}

/// Evaluates declared permissions against `permissions.json`. Prompts once interactively
/// when a third-party plugin first invokes sensitive capabilities.
pub(crate) fn authorize_plugin_runtime(
    store: &mut PermissionStore,
    manifest: &PluginManifest,
    dir: &Path,
    interactive_prompt: bool,
) -> Result<Vec<Permission>, String> {
    let bundled = is_bundled_plugin_dir(dir);
    let pending = store.pending_approval(&manifest.id, &manifest.permissions, bundled);

    if !pending.is_empty() && interactive_prompt {
        let granted = prompt_user_for_permissions(manifest, dir, &pending);
        for &perm in &pending {
            store.set_permission(&manifest.id, perm, granted);
        }
        let _ = store.save_default();
    }

    let effective = store.effective_granted(&manifest.id, &manifest.permissions, bundled);
    let has_denied_exec = manifest.permissions.iter().any(|p| {
        matches!(p, Permission::ShellExec | Permission::FilesystemWrite | Permission::Network)
            && !effective.contains(p)
    });
    if has_denied_exec && !matches!(manifest.runtime, tomoyo_protocol::RuntimeConfig::Wasm { .. }) {
        let denied_list: Vec<&str> = manifest
            .permissions
            .iter()
            .filter(|p| !effective.contains(p))
            .map(|p| permission_key(*p))
            .collect();
        return Err(format!(
            "权限未授予（{}），可在设置中心 -> 已安装插件中管理权限",
            denied_list.join(", ")
        ));
    }
    Ok(effective)
}

#[cfg(windows)]
fn prompt_user_for_permissions(manifest: &PluginManifest, dir: &Path, pending: &[Permission]) -> bool {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONQUESTION, MB_TOPMOST, MB_YESNO,
    };

    let list: String = pending
        .iter()
        .map(|p| format!("• {}", permission_label_zh(*p)))
        .collect::<Vec<_>>()
        .join("\n");
    let body = format!(
        "插件「{}」({} v{}) 首次运行请求以下敏感权限：\n\n{}\n\n来源目录：{}\n\n是否授予这些权限并允许继续运行？（之后可随时在设置中心撤销）",
        manifest.name,
        manifest.id,
        manifest.version,
        list,
        dir.display()
    );
    let wide: Vec<u16> = body.encode_utf16().chain(std::iter::once(0)).collect();
    let res = unsafe {
        MessageBoxW(
            None,
            PCWSTR(wide.as_ptr()),
            w!("Tomoyo 插件安全权限审批"),
            MB_YESNO | MB_ICONQUESTION | MB_TOPMOST,
        )
    };
    res == IDYES
}

#[cfg(target_os = "linux")]
fn prompt_user_for_permissions(manifest: &PluginManifest, dir: &Path, pending: &[Permission]) -> bool {
    let list: String = pending
        .iter()
        .map(|p| format!("• {}", permission_label_zh(*p)))
        .collect::<Vec<_>>()
        .join("\n");
    let body = format!(
        "插件「{}」({} v{}) 首次运行请求以下敏感权限：\n\n{}\n\n来源目录：{}\n\n是否授予这些权限并允许继续运行？（之后可随时在设置中心撤销）",
        manifest.name,
        manifest.id,
        manifest.version,
        list,
        dir.display()
    );
    if let Ok(st) = std::process::Command::new("kdialog")
        .args(["--title", "Tomoyo 插件安全权限审批", "--yesno", &body])
        .status()
    {
        return st.success();
    }
    if let Ok(st) = std::process::Command::new("zenity")
        .args(["--question", "--title=Tomoyo 插件安全权限审批", "--text", &body])
        .status()
    {
        return st.success();
    }
    false
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn prompt_user_for_permissions(_manifest: &PluginManifest, _dir: &Path, _pending: &[Permission]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_official_suite_plugins_are_trusted_but_third_party_are_not() {
        let plugins_root = tomoyo_core::data_dir().join("plugins");
        assert!(is_bundled_plugin_dir(&plugins_root.join("official-clipboard")));
        assert!(is_bundled_plugin_dir(&plugins_root.join("official-capture")));
        assert!(is_bundled_plugin_dir(&plugins_root.join("official-ai")));
        assert!(is_bundled_plugin_dir(&plugins_root.join("builtin-ocr")));
        let verbatim = Path::new(r"\\?\C:\Windows\Temp\plugins\official-ai");
        assert_eq!(normalize_cmp_path(verbatim), r"c:\windows\temp\plugins\official-ai");
        assert!(!is_bundled_plugin_dir(&plugins_root.join("com.thirdparty.evil")));
        assert!(!is_bundled_plugin_dir(&plugins_root.join("official-fake-thirdparty")));
    }
}
