//! Native Win32 bridge helpers (parent reload notification, ShellExecuteW, Unicode clipboard write).

pub const WM_APP_CONFIG_SAVED: u32 = 0x8000 + 20;

#[cfg(windows)]
pub fn notify_parent_mode(raw_hwnd: isize, mode: usize) {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    if raw_hwnd != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw_hwnd as *mut c_void)),
                WM_APP_CONFIG_SAVED,
                WPARAM(mode),
                LPARAM(0),
            );
        }
    }
}

#[cfg(not(windows))]
pub fn notify_parent_mode(_raw_hwnd: isize, _mode: usize) {}

#[cfg(windows)]
pub fn shell_open(target: &str) {
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
pub fn shell_open(_target: &str) {}

#[cfg(windows)]
pub fn set_clipboard_text(text: &str) {
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
pub fn set_clipboard_text(_text: &str) {}

pub fn spawn_capture_tool(mode: &str) {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(dir) = exe.parent() else { return };
    let cap_exe = dir.join("tomoyo-capture.exe");
    if !cap_exe.is_file() {
        tracing::warn!("tomoyo-capture.exe not found at {}", cap_exe.display());
        return;
    }
    let flag = match mode {
        "ocr" => "--ocr",
        "translate" => "--translate",
        "pin_clipboard" | "pin" => "--pin-clipboard",
        _ => "--capture",
    };
    let _ = std::process::Command::new(cap_exe).arg(flag).spawn();
}
