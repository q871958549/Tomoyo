//! Fast target-window avoidance & balanced synthetic mouse-button replay for Super Panel.

use super::trigger::{BTN_MIDDLE, BTN_RIGHT};
use std::sync::{OnceLock, RwLock};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
    WindowFromPoint, GA_ROOT,
};

pub const REPLAY_EXTRA_INFO: usize = 0x544D_5953; // "TMYS" marker

static EXCLUDED_APPS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();

fn excluded_store() -> &'static RwLock<Vec<String>> {
    EXCLUDED_APPS.get_or_init(|| RwLock::new(Vec::new()))
}

pub fn set_excluded_apps(apps: &[String]) {
    if let Ok(mut guard) = excluded_store().write() {
        *guard = apps.to_vec();
    }
}

/// Returns `true` when Super Panel must yield to fullscreen games/videos, remote desktop,
/// custom excluded apps, taskbar/tray/context menus, or Tomoyo's own UI windows.
pub fn should_avoid_super_panel(launcher_hwnd: HWND, extra_excluded: &[String]) -> bool {
    unsafe {
        if !launcher_hwnd.is_invalid() && IsWindowVisible(launcher_hwnd).as_bool() {
            return true;
        }
        if crate::window::hotkey_avoid::should_avoid_hotkey(launcher_hwnd) {
            return true;
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == launcher_hwnd {
            return true;
        }
        let mut fg_pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut fg_pid));
        if is_excluded_pid(fg_pid, extra_excluded) {
            return true;
        }
    }
    false
}

pub fn is_excluded_pid(pid: u32, extra_excluded: &[String]) -> bool {
    if pid == 0 || pid == std::process::id() {
        return true;
    }
    if let Some(exe) = crate::window::hotkey_avoid::query_process_basename_lower(pid) {
        if matches!(exe.as_str(), "tomoyo.exe" | "tomoyo-capture.exe" | "tomoyo-webview-host.exe") {
            return true;
        }
        if extra_excluded.iter().any(|e| e.eq_ignore_ascii_case(&exe)) {
            return true;
        }
    }
    false
}

pub fn is_protected_shell_class(cls: &str) -> bool {
    matches!(
        cls,
        "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
            | "TopLevelOverflowWindow"
            | "TrayNotifyWnd"
            | "TaskListThumbnailWnd"
            | "MultitaskingViewFrame"
            | "#32768"
    )
}

/// Fast `< 100 us` check on `WM_*BUTTONDOWN` so Taskbar, Tray, popup menus, fullscreen games,
/// excluded apps, and Tomoyo's own windows NEVER have their clicks intercepted or delayed.
pub fn should_bypass_on_down(pt: POINT, launcher_hwnd: HWND) -> bool {
    unsafe {
        if !launcher_hwnd.is_invalid() && IsWindowVisible(launcher_hwnd).as_bool() {
            return true;
        }
        let under = WindowFromPoint(pt);
        if !under.is_invalid() {
            let root = GetAncestor(under, GA_ROOT);
            let target = if !root.is_invalid() { root } else { under };
            let mut cls_buf = [0u16; 64];
            let len = GetClassNameW(target, &mut cls_buf) as usize;
            if len > 0 {
                let cls = String::from_utf16_lossy(&cls_buf[..len]);
                if is_protected_shell_class(&cls) {
                    return true;
                }
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(target, Some(&mut pid));
            let excl_guard = excluded_store().try_read().ok();
            let excl_slice = excl_guard.as_deref().map_or(&[][..], Vec::as_slice);
            if is_excluded_pid(pid, excl_slice) {
                return true;
            }
            return crate::window::hotkey_avoid::should_avoid_hotkey(launcher_hwnd);
        }
    }
    let excl_guard = excluded_store().try_read().ok();
    should_avoid_super_panel(launcher_hwnd, excl_guard.as_deref().map_or(&[][..], Vec::as_slice))
}

pub fn replay_mouse_button(btn: u8, send_down: bool, send_up: bool) {
    let (down_flag, up_flag) = match btn {
        BTN_RIGHT => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        BTN_MIDDLE => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
        _ => return,
    };
    let mk = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                dwExtraInfo: REPLAY_EXTRA_INFO,
                ..Default::default()
            },
        },
    };
    let mut buf = [mk(down_flag), mk(up_flag)];
    let slice = match (send_down, send_up) {
        (true, true) => &buf[..2],
        (true, false) => &buf[..1],
        (false, true) => {
            buf[0] = mk(up_flag);
            &buf[..1]
        }
        (false, false) => return,
    };
    unsafe {
        let _ = SendInput(slice, std::mem::size_of::<INPUT>() as i32);
    }
}
