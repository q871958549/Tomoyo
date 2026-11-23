//! Extracted WebView host command handlers for Store, Permissions, Diagnostics, and Clipboard Workbench.

use crate::bridge::{notify_parent_reload, HostAction};
use crate::bridge_os::{notify_parent_mode, set_clipboard_text, shell_open};
use crate::store::{install_plugin_from_source, uninstall_user_plugin};

pub(crate) fn handle_install_plugin(
    plugin_id: String,
    source: String,
    sha256: String,
    notify_hwnd: Option<isize>,
) -> HostAction {
    let res = install_plugin_from_source(&plugin_id, &source, &sha256);
    if res.is_ok() {
        crate::store_index::invalidate_catalog_cache();
        if let Some(target) = notify_hwnd {
            notify_parent_reload(target, true);
        }
    }
    let evt = match res {
        Ok(dest) => serde_json::json!({
            "event": "store_action_result",
            "ok": true,
            "action": "install",
            "plugin_id": plugin_id,
            "message": format!("SHA-256 校验通过，已安装到 {dest}")
        }),
        Err(e) => serde_json::json!({
            "event": "store_action_result",
            "ok": false,
            "action": "install",
            "plugin_id": plugin_id,
            "message": e
        }),
    };
    HostAction { reply_json: Some(evt.to_string()), ..Default::default() }
}

pub(crate) fn handle_uninstall_plugin(plugin_id: String, notify_hwnd: Option<isize>) -> HostAction {
    let res = uninstall_user_plugin(&plugin_id);
    if res.is_ok() {
        crate::store_index::invalidate_catalog_cache();
        if let Some(target) = notify_hwnd {
            notify_parent_reload(target, true);
        }
    }
    let evt = match res {
        Ok(()) => serde_json::json!({
            "event": "store_action_result",
            "ok": true,
            "action": "uninstall",
            "plugin_id": plugin_id,
            "message": "已卸载并热重载主窗口插件列表"
        }),
        Err(e) => serde_json::json!({
            "event": "store_action_result",
            "ok": false,
            "action": "uninstall",
            "plugin_id": plugin_id,
            "message": e
        }),
    };
    HostAction { reply_json: Some(evt.to_string()), ..Default::default() }
}

pub(crate) fn handle_set_plugin_permission(
    plugin_id: String,
    permission: String,
    allowed: bool,
    notify_hwnd: Option<isize>,
) -> HostAction {
    let mut msg = "权限参数无效".to_string();
    let mut ok = false;
    if let Some(perm) = tomoyo_core::permission_from_key(&permission) {
        let mut pstore = tomoyo_core::PermissionStore::load_default();
        pstore.set_permission(&plugin_id, perm, allowed);
        if let Err(e) = pstore.save_default() {
            msg = format!("保存权限失败: {e}");
        } else {
            if let Some(target) = notify_hwnd {
                notify_parent_reload(target, true);
            }
            ok = true;
            msg = if allowed {
                format!("已授予 {plugin_id} 权限 `{permission}`")
            } else {
                format!("已撤销 {plugin_id} 权限 `{permission}`")
            };
        }
    }
    let evt = serde_json::json!({
        "event": "perm_action_result",
        "ok": ok,
        "message": msg,
    });
    HostAction { reply_json: Some(evt.to_string()), ..Default::default() }
}

pub(crate) fn handle_read_logs(source: String, errors_only: bool) -> HostAction {
    build_logs_action(&source, errors_only, false)
}

pub(crate) fn handle_clear_logs(source: String, errors_only: bool) -> HostAction {
    let archived = tomoyo_core::archive_diagnostic_issues().is_ok();
    build_logs_action(&source, errors_only, archived)
}

fn build_logs_action(source: &str, errors_only: bool, cleared: bool) -> HostAction {
    let safe_file = match source {
        "webview.log" => "webview.log",
        "update.log" => "update.log",
        "inno_setup.log" => "inno_setup.log",
        "last_crash.txt" => "last_crash.txt",
        _ => "tomoyo.log",
    };
    let content = tomoyo_core::read_log_tail(safe_file, 260, errors_only);
    let (warnings, errors, has_crash_report) = tomoyo_core::count_recent_log_issues();
    let evt = serde_json::json!({
        "event": "logs_loaded",
        "source": safe_file,
        "errors_only": errors_only,
        "cleared": cleared,
        "content": content,
        "issues": {
            "warnings": warnings,
            "errors": errors,
            "has_crash_report": has_crash_report,
        },
        "logs_dir": tomoyo_core::logs_dir().to_string_lossy(),
    });
    HostAction { reply_json: Some(evt.to_string()), ..Default::default() }
}

pub(crate) fn handle_copy_diag_bundle() -> HostAction {
    let bundle = tomoyo_core::build_diagnostic_bundle();
    set_clipboard_text(&bundle);
    let evt = serde_json::json!({
        "event": "diag_bundle_copied",
        "ok": true,
        "bytes": bundle.len(),
    });
    HostAction { reply_json: Some(evt.to_string()), ..Default::default() }
}

pub(crate) fn handle_open_logs_dir() -> HostAction {
    let dir = tomoyo_core::logs_dir();
    let _ = std::fs::create_dir_all(&dir);
    shell_open(&dir.to_string_lossy());
    HostAction::default()
}

pub(crate) fn handle_open_clipboard_manager(notify_hwnd: Option<isize>) -> HostAction {
    if let Ok(exe) = std::env::current_exe() {
        let theme = tomoyo_core::TomoyoConfig::load_or_create().theme;
        let mut cmd = std::process::Command::new(exe);
        cmd.args([
            "--plugin-id", "builtin.clipboard",
            "--title", "Tomoyo 剪贴板工作台",
            "--entry", crate::clip_view::CLIP_MANAGER_ENTRY,
            "--theme", &theme,
            "--width", "1060", "--height", "680",
        ]);
        if let Some(h) = notify_hwnd {
            cmd.args(["--notify-hwnd", &h.to_string()]);
        }
        let _ = cmd.spawn();
    }
    HostAction::default()
}

pub(crate) fn finish_clip_mutation(
    reply: String,
    mutated: bool,
    notify_hwnd: Option<isize>,
) -> HostAction {
    if mutated
        && let Some(t) = notify_hwnd
    {
        notify_parent_mode(t, 2);
    }
    HostAction { reply_json: Some(reply), ..Default::default() }
}
