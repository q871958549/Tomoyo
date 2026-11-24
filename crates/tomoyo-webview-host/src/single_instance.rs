//! Single-instance Mutex & window activation for the Tomoyo Settings Center.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, IsIconic, PostMessageW,
    SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
};

pub const SETTINGS_WINDOW_CLASS: PCWSTR = w!("TomoyoSettingsCenterWindowClass");
pub const CLIPBOARD_WINDOW_CLASS: PCWSTR = w!("TomoyoClipboardManagerWindowClass");
pub const EPHEMERAL_WINDOW_CLASS: PCWSTR = w!("TomoyoEphemeralWebViewHost");
pub const SETTINGS_SINGLETON_MUTEX: PCWSTR = w!("Local\\TomoyoSettingsCenterSingleton");
pub const CLIPBOARD_SINGLETON_MUTEX: PCWSTR = w!("Local\\TomoyoClipboardManagerSingleton");
pub const WM_APP_ACTIVATE_SETTINGS: u32 = 0x8000 + 32;
pub const WM_APP_ACTIVATE_CLIPBOARD: u32 = 0x8000 + 34;

pub struct SettingsSingletonGuard(HANDLE);

impl Drop for SettingsSingletonGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_invalid() {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Acquires the single-instance lock for the Settings Center.
/// If another instance already holds it, raises that window and returns `None`.
pub fn acquire_or_activate_settings() -> Option<SettingsSingletonGuard> {
    acquire_or_activate_named(
        SETTINGS_SINGLETON_MUTEX,
        Some(SETTINGS_WINDOW_CLASS),
        WM_APP_ACTIVATE_SETTINGS,
    )
}

pub fn acquire_or_activate_clipboard() -> Option<SettingsSingletonGuard> {
    acquire_or_activate_named(
        CLIPBOARD_SINGLETON_MUTEX,
        Some(CLIPBOARD_WINDOW_CLASS),
        WM_APP_ACTIVATE_CLIPBOARD,
    )
}

fn acquire_or_activate_named(
    mutex_name: PCWSTR,
    window_class: Option<PCWSTR>,
    activate_msg: u32,
) -> Option<SettingsSingletonGuard> {
    unsafe {
        let handle = CreateMutexW(None, false, mutex_name).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            if let Some(cls) = window_class {
                activate_existing_with_retry(cls, activate_msg);
            }
            return None;
        }
        Some(SettingsSingletonGuard(handle))
    }
}

fn activate_existing_with_retry(window_class: PCWSTR, activate_msg: u32) {
    // Wait briefly in case the first process acquired the mutex moments before CreateWindowExW.
    for attempt in 0..8 {
        unsafe {
            if let Ok(hwnd) = FindWindowW(window_class, PCWSTR::null())
                && !hwnd.is_invalid() {
                    activate_window_with_msg(hwnd, activate_msg);
                    return;
                }
        }
        if attempt < 7 {
            std::thread::sleep(std::time::Duration::from_millis(35));
        }
    }
}

pub fn activate_window_with_msg(hwnd: HWND, activate_msg: u32) {
    unsafe {
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            let _ = AllowSetForegroundWindow(pid);
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = PostMessageW(Some(hwnd), activate_msg, WPARAM(0), LPARAM(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_single_settings_instance_per_mutex() {
        let test_mutex = w!("Local\\TomoyoSettingsCenterSingleton_UnitTest");
        let guard = acquire_or_activate_named(test_mutex, None, WM_APP_ACTIVATE_SETTINGS)
            .expect("first settings guard acquisition must succeed");
        assert!(
            acquire_or_activate_named(test_mutex, None, WM_APP_ACTIVATE_SETTINGS).is_none(),
            "second concurrent settings acquisition must be rejected"
        );
        drop(guard);
        assert!(
            acquire_or_activate_named(test_mutex, None, WM_APP_ACTIVATE_SETTINGS).is_some(),
            "acquisition must succeed again after first settings window closes"
        );
    }
}
