//! Zero-polling fullscreen & remote-desktop hotkey avoidance guard.
//!
//! Invoked ONLY at the exact millisecond a hotkey fires (`< 0.05 ms`, 0 background threads/timers).
//! Yields Tomoyo hotkeys & override hooks automatically when a fullscreen game/video or excluded
//! remote-desktop client (e.g. `raylink.exe`, `mstsc.exe`, `parsec.exe`) owns the foreground.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    GWL_STYLE, WS_CAPTION,
};

static SUSPEND_FULLSCREEN: AtomicBool = AtomicBool::new(true);
static EXCLUDED_APPS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
static LAST_YIELD_LOG_MS: AtomicU64 = AtomicU64::new(0);

#[inline]
fn is_fullscreen_quns_state(state: QUERY_USER_NOTIFICATION_STATE) -> bool {
    state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE
}

fn log_hotkey_yield(exe_lower: &str, reason: &str) {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let prev = LAST_YIELD_LOG_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(prev) >= 3000
        && LAST_YIELD_LOG_MS
            .compare_exchange(prev, now_ms, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    {
        tracing::info!(exe = %exe_lower, reason = reason, "Yielded global hotkey to foreground window");
    }
}

fn excluded_store() -> &'static RwLock<Vec<String>> {
    EXCLUDED_APPS.get_or_init(|| RwLock::new(tomoyo_core::default_hotkey_excluded_apps()))
}

/// Refreshes the in-memory avoidance settings on startup & config hot-reload.
pub(super) fn sync_hotkey_avoidance(suspend_fullscreen: bool, excluded_apps: &[String]) {
    SUSPEND_FULLSCREEN.store(suspend_fullscreen, Ordering::Relaxed);
    if let Ok(mut guard) = excluded_store().write() {
        *guard = excluded_apps.iter().map(|s| s.to_ascii_lowercase()).collect();
    }
}

/// Returns `true` when Tomoyo should yield a global hotkey to the current foreground window.
pub(crate) fn should_avoid_hotkey(self_hwnd: HWND) -> bool {
    unsafe {
        if !self_hwnd.is_invalid() && IsWindowVisible(self_hwnd).as_bool() {
            return false;
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == self_hwnd {
            return false;
        }
        let mut fg_pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut fg_pid));
        if fg_pid == 0 || fg_pid == std::process::id() {
            return false;
        }

        let exe_lower = query_process_basename_lower(fg_pid).unwrap_or_default();
        if !exe_lower.is_empty()
            && let Ok(guard) = excluded_store().try_read()
                && guard.iter().any(|ex| ex == &exe_lower) {
                    log_hotkey_yield(&exe_lower, "excluded_app");
                    return true;
                }

        if !SUSPEND_FULLSCREEN.load(Ordering::Relaxed) {
            return false;
        }
        // Desktop / shell host (`explorer.exe`) must never be treated as a fullscreen game.
        if exe_lower == "explorer.exe" || exe_lower == "shellexperiencehost.exe" {
            return false;
        }

        if let Ok(state) = SHQueryUserNotificationState()
            && is_fullscreen_quns_state(state) {
                log_hotkey_yield(&exe_lower, "fullscreen_quns");
                return true;
            }

        // Borderless fullscreen game / media player check: covers entire monitor AND has no caption bar
        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        if (style & WS_CAPTION.0) == 0 {
            let mut wr = RECT::default();
            if GetWindowRect(fg, &mut wr).is_ok() {
                let hmon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                    let mr = mi.rcMonitor;
                    if wr.left <= mr.left && wr.top <= mr.top && wr.right >= mr.right && wr.bottom >= mr.bottom {
                        log_hotkey_yield(&exe_lower, "borderless_fullscreen");
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub(crate) fn query_process_basename_lower(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 320];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok || len == 0 {
            return None;
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&full)
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Shell::{QUNS_ACCEPTS_NOTIFICATIONS, QUNS_BUSY};

    #[test]
    fn quns_busy_does_not_suppress_hotkeys() {
        assert!(is_fullscreen_quns_state(QUNS_RUNNING_D3D_FULL_SCREEN));
        assert!(is_fullscreen_quns_state(QUNS_PRESENTATION_MODE));
        assert!(!is_fullscreen_quns_state(QUNS_BUSY));
        assert!(!is_fullscreen_quns_state(QUNS_ACCEPTS_NOTIFICATIONS));
    }
}
