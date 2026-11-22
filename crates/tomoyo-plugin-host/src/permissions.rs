//! Runtime plugin capability authorization, interactive prompt & revocation enforcement.

use std::path::Path;
use tomoyo_core::{permission_key, permission_label_zh, PermissionStore};
use tomoyo_protocol::{Permission, PluginManifest};

pub(crate) fn is_bundled_plugin_dir(dir: &Path) -> bool {
    let dir_str = dir.to_string_lossy().to_ascii_lowercase();
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent() {
            let bundled = parent.join("plugins").to_string_lossy().to_ascii_lowercase();
            if dir_str.starts_with(&bundled) {
                return true;
            }
        }
    if let Ok(cwd) = std::env::current_dir() {
        let local_plugins = cwd.join("plugins").to_string_lossy().to_ascii_lowercase();
        if dir_str.starts_with(&local_plugins) {
            return true;
        }
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
