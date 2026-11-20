//! Dispatches incoming WebView2 host messages safely to native OS helpers, DB, and Plugin Store.

use crate::db::PluginDb;
use crate::store::{install_plugin_from_source, uninstall_user_plugin};
use serde::Deserialize;
use serde_json::Value;

pub const WM_APP_CONFIG_SAVED: u32 = 0x8000 + 20;

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum HostMessage {
    CopyText { text: String },
    CopyAndPaste { text: String },
    OpenExternal { url: String },
    OpenPath { path: String },
    DbPut { id: String, doc: Value },
    DbRemove { id: String },
    SaveSettings { config: tomoyo_core::TomoyoConfig, autostart: bool },
    SyncTheme { theme: String },
    TogglePinTopmost,
    CheckUpdates { repo: String },
    InstallUpdate { release: tomoyo_core::ReleaseInfo },
    FetchStore { sources: Vec<String> },
    InstallPlugin {
        plugin_id: String,
        source: String,
        sha256: String,
    },
    UninstallPlugin { plugin_id: String },
    SetPluginPermission {
        plugin_id: String,
        permission: String,
        allowed: bool,
    },
    Close,
}

#[derive(Debug, Default)]
pub struct HostAction {
    pub should_close: bool,
    pub apply_theme: Option<String>,
    pub toggle_topmost: bool,
    pub reply_json: Option<String>,
    pub async_check_updates: Option<String>,
    pub async_install_update: Option<tomoyo_core::ReleaseInfo>,
    pub async_fetch_store: Option<Vec<String>>,
}

pub fn handle_host_message(raw: &str, db: &mut PluginDb, notify_hwnd: Option<isize>) -> HostAction {
    let Ok(msg) = serde_json::from_str::<HostMessage>(raw) else {
        return HostAction::default();
    };
    match msg {
        HostMessage::CopyText { text } => {
            set_clipboard_text(&text);
            HostAction::default()
        }
        HostMessage::CopyAndPaste { text } => {
            set_clipboard_text(&text);
            HostAction {
                should_close: true,
                ..Default::default()
            }
        }
        HostMessage::OpenExternal { url } => {
            shell_open(&url);
            HostAction::default()
        }
        HostMessage::OpenPath { path } => {
            shell_open(&path);
            HostAction::default()
        }
        HostMessage::DbPut { id, doc } => {
            db.put(id, doc);
            HostAction::default()
        }
        HostMessage::DbRemove { id } => {
            db.remove(&id);
            HostAction::default()
        }
        HostMessage::SaveSettings { config, autostart } => {
            let theme = config.theme.clone();
            let _ = config.save();
            let _ = tomoyo_core::set_autostart_enabled(autostart);
            if let Some(target) = notify_hwnd {
                notify_parent_reload(target);
            }
            HostAction {
                apply_theme: Some(theme),
                ..Default::default()
            }
        }
        HostMessage::SyncTheme { theme } => HostAction {
            apply_theme: Some(theme),
            ..Default::default()
        },
        HostMessage::TogglePinTopmost => HostAction {
            toggle_topmost: true,
            ..Default::default()
        },
        HostMessage::CheckUpdates { repo } => HostAction {
            async_check_updates: Some(repo),
            ..Default::default()
        },
        HostMessage::InstallUpdate { release } => HostAction {
            async_install_update: Some(release),
            ..Default::default()
        },
        HostMessage::FetchStore { sources } => HostAction {
            async_fetch_store: Some(sources),
            ..Default::default()
        },
        HostMessage::InstallPlugin {
            plugin_id,
            source,
            sha256,
        } => {
            let res = install_plugin_from_source(&plugin_id, &source, &sha256);
            if res.is_ok() {
                if let Some(target) = notify_hwnd {
                    notify_parent_reload(target);
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
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::UninstallPlugin { plugin_id } => {
            let res = uninstall_user_plugin(&plugin_id);
            if res.is_ok() {
                if let Some(target) = notify_hwnd {
                    notify_parent_reload(target);
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
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::SetPluginPermission {
            plugin_id,
            permission,
            allowed,
        } => {
            let mut msg = "权限参数无效".to_string();
            let mut ok = false;
            if let Some(perm) = tomoyo_core::permission_from_key(&permission) {
                let mut pstore = tomoyo_core::PermissionStore::load_default();
                pstore.set_permission(&plugin_id, perm, allowed);
                if let Err(e) = pstore.save_default() {
                    msg = format!("保存权限失败: {e}");
                } else {
                    if let Some(target) = notify_hwnd {
                        notify_parent_reload(target);
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
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::Close => HostAction {
            should_close: true,
            ..Default::default()
        },
    }
}

#[cfg(windows)]
fn notify_parent_reload(raw_hwnd: isize) {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    if raw_hwnd != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw_hwnd as *mut c_void)),
                WM_APP_CONFIG_SAVED,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

#[cfg(not(windows))]
fn notify_parent_reload(_raw_hwnd: isize) {}

#[cfg(windows)]
fn shell_open(target: &str) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(not(windows))]
fn shell_open(_target: &str) {}

#[cfg(windows)]
fn set_clipboard_text(text: &str) {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * 2;
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, byte_len) {
            let ptr = GlobalLock(hmem) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(hmem);
                let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

#[cfg(not(windows))]
fn set_clipboard_text(_text: &str) {}

#[cfg(test)]
mod tests {
    use crate::shim_js::build_init_script;

    #[test]
    fn generates_tomoyo_and_utools_bridge() {
        let script = build_init_script("dev.test.json", "{\"a\":1}", true, true, "{}");
        assert!(script.contains("window.tomoyo ="));
        assert!(script.contains("window.utools ="));
        assert!(script.contains("onPluginEnter"));
        assert!(script.contains("setSubInput"));
        assert!(script.contains("hideMainWindowPasteText"));
        assert!(script.contains("dbStorage"));
    }
}
