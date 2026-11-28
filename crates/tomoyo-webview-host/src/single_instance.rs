//! Single-instance Mutex & window activation for Tomoyo Settings Center and standalone plugin WebViews.

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, WPARAM,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, FindWindowW, GetWindowThreadProcessId, IsIconic,
    PostMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_NOTOPMOST, HWND_TOPMOST,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, SW_SHOW,
};

pub const SETTINGS_WINDOW_CLASS: PCWSTR = w!("TomoyoSettingsCenterWindowClass");
pub const EPHEMERAL_WINDOW_CLASS: PCWSTR = w!("TomoyoEphemeralWebViewHost");
pub const SETTINGS_SINGLETON_MUTEX: PCWSTR = w!("Local\\TomoyoSettingsCenterSingleton");
pub const WM_APP_ACTIVATE_SETTINGS: u32 = 0x8000 + 32;
pub const WM_APP_ACTIVATE_PLUGIN: u32 = 0x8000 + 34;

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
        8,
    )
}

/// Writes the latest query payload to the per-plugin handoff file atomically.
pub fn write_pending_plugin_query(plugin_id: &str, query: &str) {
    let p = tomoyo_core::pending_plugin_query_path(plugin_id);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = p.with_extension(format!("{}.tmp", std::process::id()));
    if std::fs::write(&tmp, query.as_bytes()).is_ok() {
        if std::fs::rename(&tmp, &p).is_err() {
            let _ = std::fs::write(&p, query.as_bytes());
            let _ = std::fs::remove_file(&tmp);
        }
    } else {
        let _ = std::fs::write(&p, query.as_bytes());
    }
}

/// Acquires the per-plugin single-instance mutex for a standalone plugin WebView window.
/// If an instance for `plugin_id` already exists, forwards `query` to it, activates it, and returns `None`.
pub fn acquire_or_activate_plugin(plugin_id: &str, query: &str) -> Option<SettingsSingletonGuard> {
    let tag = tomoyo_core::sanitize_plugin_instance_tag(plugin_id);
    let mutex_w = HSTRING::from(format!("Local\\TomoyoPluginHostSingleton_{tag}"));
    let cls_w = HSTRING::from(tomoyo_core::plugin_webview_window_class(plugin_id));
    unsafe {
        let handle = CreateMutexW(None, false, PCWSTR(mutex_w.as_ptr())).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            write_pending_plugin_query(plugin_id, query);
            activate_existing_with_retry(PCWSTR(cls_w.as_ptr()), WM_APP_ACTIVATE_PLUGIN, 10);
            return None;
        }
        let _ = std::fs::remove_file(tomoyo_core::pending_plugin_query_path(plugin_id));
        Some(SettingsSingletonGuard(handle))
    }
}

fn acquire_or_activate_named(
    mutex_name: PCWSTR,
    window_class: Option<PCWSTR>,
    activate_msg: u32,
    retries: usize,
) -> Option<SettingsSingletonGuard> {
    unsafe {
        let handle = CreateMutexW(None, false, mutex_name).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            if let Some(cls) = window_class {
                activate_existing_with_retry(cls, activate_msg, retries);
            }
            return None;
        }
        Some(SettingsSingletonGuard(handle))
    }
}

fn activate_existing_with_retry(window_class: PCWSTR, activate_msg: u32, retries: usize) {
    // Wait briefly in case the first process acquired the mutex moments before CreateWindowExW.
    for attempt in 0..retries {
        unsafe {
            if let Ok(hwnd) = FindWindowW(window_class, PCWSTR::null())
                && !hwnd.is_invalid()
            {
                activate_window_with_msg(hwnd, activate_msg);
                return;
            }
        }
        if attempt + 1 < retries {
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

unsafe fn raise_hwnd_to_front(hwnd: HWND, is_topmost: bool) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = BringWindowToTop(hwnd);
        if !SetForegroundWindow(hwnd).as_bool() && !is_topmost {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// Raises the Settings Center window and applies any pending route navigation request.
pub unsafe fn handle_activate_settings_msg(hwnd: HWND, webview: Option<&ICoreWebView2>) {
    unsafe {
        raise_hwnd_to_front(hwnd, false);
        let p = tomoyo_core::logs_dir().join("pending_settings_route.txt");
        if let Ok(raw) = std::fs::read_to_string(&p) {
            let _ = std::fs::remove_file(&p);
            let route = raw.trim();
            if !route.is_empty() && let Some(wv) = webview {
                let r_json = serde_json::to_string(route).unwrap_or_else(|_| "\"\"".into());
                let js = format!("window.__TOMOYO_NAVIGATE_ROUTE__ && window.__TOMOYO_NAVIGATE_ROUTE__({r_json});");
                let jw = HSTRING::from(js);
                let _ = wv.ExecuteScript(PCWSTR(jw.as_ptr()), None);
            }
        }
    }
}

/// Raises a reactivated standalone plugin window and forwards any pending query to its WebView.
pub unsafe fn handle_activate_plugin_msg(
    hwnd: HWND,
    webview: Option<&ICoreWebView2>,
    plugin_id: &str,
    is_topmost: bool,
    pending_out: &mut Option<String>,
) {
    unsafe {
        raise_hwnd_to_front(hwnd, is_topmost);
        let p = tomoyo_core::pending_plugin_query_path(plugin_id);
        if let Ok(query) = std::fs::read_to_string(&p) {
            let _ = std::fs::remove_file(&p);
            if let Some(wv) = webview {
                reactivate_webview_with_query(wv, &query);
            } else {
                *pending_out = Some(query);
            }
        }
    }
}

/// Evaluates `window.tomoyo.__reactivate(query)` inside an already running plugin WebView.
pub unsafe fn reactivate_webview_with_query(wv: &ICoreWebView2, query: &str) {
    let q_json = serde_json::to_string(query).unwrap_or_else(|_| "\"\"".into());
    let js = format!("window.tomoyo && window.tomoyo.__reactivate && window.tomoyo.__reactivate({q_json});");
    let jw = HSTRING::from(js);
    unsafe {
        let _ = wv.ExecuteScript(PCWSTR(jw.as_ptr()), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_single_settings_instance_per_mutex() {
        let test_mutex = w!("Local\\TomoyoSettingsCenterSingleton_UnitTest");
        let guard = acquire_or_activate_named(test_mutex, None, WM_APP_ACTIVATE_SETTINGS, 1)
            .expect("first settings guard acquisition must succeed");
        assert!(
            acquire_or_activate_named(test_mutex, None, WM_APP_ACTIVATE_SETTINGS, 1).is_none(),
            "second concurrent settings acquisition must be rejected"
        );
        drop(guard);
        assert!(
            acquire_or_activate_named(test_mutex, None, WM_APP_ACTIVATE_SETTINGS, 1).is_some(),
            "acquisition must succeed again after first settings window closes"
        );
    }

    #[test]
    fn enforces_per_plugin_singleton_and_handoffs_pending_query() {
        let pid_tr = "dev.tomoyo.unit_test_translate";
        let pid_cb = "dev.tomoyo.unit_test_clipboard";
        let _ = std::fs::remove_file(tomoyo_core::pending_plugin_query_path(pid_tr));

        let g1 = acquire_or_activate_plugin(pid_tr, "first").expect("first plugin lock must succeed");
        let g_other = acquire_or_activate_plugin(pid_cb, "")
            .expect("independent plugin ID must acquire concurrently");

        assert!(
            acquire_or_activate_plugin(pid_tr, "second translated text").is_none(),
            "duplicate launch of same plugin_id must be coalesced"
        );
        let handed = std::fs::read_to_string(tomoyo_core::pending_plugin_query_path(pid_tr))
            .expect("duplicate launch must hand off query to pending file");
        assert_eq!(handed, "second translated text");

        drop(g_other);
        drop(g1);
        let _ = std::fs::remove_file(tomoyo_core::pending_plugin_query_path(pid_tr));
        let g2 = acquire_or_activate_plugin(pid_tr, "reopened")
            .expect("lock must be acquirable again after window closes");
        drop(g2);
    }
}
