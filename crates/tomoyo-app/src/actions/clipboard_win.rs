//! Win32 native clipboard read/write with valid owner `HWND` and single-lock atomic snapshots.
//!
//! Never holds `OpenClipboard` across heavy image decoding and collapses multi-format reads into
//! a single `< 0.3 ms` lock so third-party multi-step copiers (OneCommander, Explorer, Excel, Word)
//! never encounter `CLIPBRD_E_CANT_OPEN`.

pub use super::clipboard_win_img::{
    copy_image_as_base64_uri, decode_and_cache_dib_bytes, set_clipboard_image_from_png,
};

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

static CLIPBOARD_OWNER_HWND: AtomicIsize = AtomicIsize::new(0);

/// Registers Tomoyo's main window handle as the Win32 clipboard owner for `OpenClipboard` + `EmptyClipboard`.
pub fn set_clipboard_owner_hwnd(hwnd_raw: isize) {
    CLIPBOARD_OWNER_HWND.store(hwnd_raw, Ordering::Release);
}

#[inline]
fn owner_hwnd_opt() -> Option<HWND> {
    let raw = CLIPBOARD_OWNER_HWND.load(Ordering::Acquire);
    (raw != 0).then_some(HWND(raw as *mut c_void))
}

/// Opens the Windows clipboard using Tomoyo's registered owner `HWND` with gentle retry backoff.
pub fn open_clipboard_owned(max_attempts: usize) -> bool {
    let owner = owner_hwnd_opt();
    let tries = max_attempts.max(1);
    for attempt in 0..tries {
        if unsafe { OpenClipboard(owner).is_ok() } {
            return true;
        }
        if attempt + 1 < tries {
            std::thread::sleep(Duration::from_millis(12));
        }
    }
    false
}

/// Result of a single `< 0.3 ms` `OpenClipboard` snapshot for clipboard history monitoring.
pub enum ClipboardCaptureSnapshot {
    Confidential,
    Files(Vec<String>),
    Text(String),
    ImageDib(Vec<u8>),
    Empty,
}

/// Captures confidentiality state + active clipboard payload in **at most ONE** `OpenClipboard` lock.
/// Image decoding is deferred outside the lock so OS clipboard contention stays near zero.
pub fn read_clipboard_snapshot_once(want_text_or_files: bool, want_image: bool) -> ClipboardCaptureSnapshot {
    use windows::core::w;
    unsafe {
        let excl = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        if excl != 0 && IsClipboardFormatAvailable(excl).is_ok() {
            return ClipboardCaptureSnapshot::Confidential;
        }
        let has_files = want_text_or_files && IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok();
        let has_text = IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok();
        let has_dib = want_image && IsClipboardFormatAvailable(CF_DIB.0 as u32).is_ok();
        if !has_files && !has_text && !has_dib {
            return ClipboardCaptureSnapshot::Empty;
        }
        let hist_fmt = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        let check_hist = hist_fmt != 0 && IsClipboardFormatAvailable(hist_fmt).is_ok();

        if !open_clipboard_owned(1) {
            return ClipboardCaptureSnapshot::Empty;
        }
        let result = (|| {
            if check_hist && read_u32_format_locked(hist_fmt) == Some(0) {
                return ClipboardCaptureSnapshot::Confidential;
            }
            if has_files && let Some(files) = read_hdrop_locked() {
                return ClipboardCaptureSnapshot::Files(files);
            }
            if has_text && let Some(txt) = read_unicode_text_locked() {
                return ClipboardCaptureSnapshot::Text(txt);
            }
            if has_dib && let Some(raw) = read_dib_bytes_locked() {
                return ClipboardCaptureSnapshot::ImageDib(raw);
            }
            ClipboardCaptureSnapshot::Empty
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Reads either copied file paths (`CF_HDROP`) or Unicode text (`CF_UNICODETEXT`) in ONE single
/// short `OpenClipboard` lock (used by Super Panel selection probing).
pub fn read_clipboard_files_or_text_once() -> (Option<Vec<String>>, Option<String>) {
    unsafe {
        let has_files = IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok();
        let has_text = IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok();
        if !has_files && !has_text {
            return (None, None);
        }
        if !open_clipboard_owned(2) {
            return (None, None);
        }
        let out = if has_files && let Some(files) = read_hdrop_locked() {
            (Some(files), None)
        } else if has_text {
            (None, read_unicode_text_locked())
        } else {
            (None, None)
        };
        let _ = CloseClipboard();
        out
    }
}

pub fn set_clipboard_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * std::mem::size_of::<u16>();
    unsafe {
        if !open_clipboard_owned(4) {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = match GlobalAlloc(GMEM_MOVEABLE, byte_len) {
            Ok(hmem) => {
                let ptr = GlobalLock(hmem) as *mut u16;
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                    let _ = GlobalUnlock(hmem);
                    SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0))).is_ok()
                } else {
                    false
                }
            }
            Err(_) => false,
        };
        let _ = CloseClipboard();
        ok
    }
}

pub fn read_clipboard_text() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() || !open_clipboard_owned(2) {
            return None;
        }
        let out = read_unicode_text_locked();
        let _ = CloseClipboard();
        out
    }
}

/// Checks whether the Windows clipboard currently holds an image (`CF_DIB`) or file list (`CF_HDROP`).
pub fn clipboard_has_rich_attachment() -> bool {
    unsafe {
        IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok()
            || IsClipboardFormatAvailable(CF_DIB.0 as u32).is_ok()
    }
}

/// Reads copied file paths (`CF_HDROP`) from the Windows clipboard (capped at 32 paths).
pub fn read_clipboard_files() -> Option<Vec<String>> {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_err() || !open_clipboard_owned(2) {
            return None;
        }
        let out = read_hdrop_locked();
        let _ = CloseClipboard();
        out
    }
}

/// Captures a `CF_DIB` screenshot/image to a content-addressed PNG on disk (decodes outside lock).
pub fn read_clipboard_image_png() -> Option<(String, u32, u32, String)> {
    let raw_dib = unsafe {
        if IsClipboardFormatAvailable(CF_DIB.0 as u32).is_err() || !open_clipboard_owned(2) {
            return None;
        }
        let bytes = read_dib_bytes_locked();
        let _ = CloseClipboard();
        bytes
    }?;
    decode_and_cache_dib_bytes(&raw_dib)
}

unsafe fn read_u32_format_locked(fmt: u32) -> Option<u32> {
    let h = unsafe { GetClipboardData(fmt).ok()? };
    let hm = HGLOBAL(h.0);
    if unsafe { GlobalSize(hm) } < 4 {
        return None;
    }
    let ptr = unsafe { GlobalLock(hm) as *const u32 };
    if ptr.is_null() {
        return None;
    }
    let val = unsafe { *ptr };
    let _ = unsafe { GlobalUnlock(hm) };
    Some(val)
}

unsafe fn read_unicode_text_locked() -> Option<String> {
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32).ok()? };
    let hmem = HGLOBAL(handle.0);
    let max_bytes = unsafe { GlobalSize(hmem) }.min(64 * 1024);
    let ptr = unsafe { GlobalLock(hmem) as *const u16 };
    if ptr.is_null() || max_bytes < 2 {
        return None;
    }
    let max_u16 = max_bytes / 2;
    let slice = unsafe { std::slice::from_raw_parts(ptr, max_u16) };
    let len = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    let text = String::from_utf16_lossy(&slice[..len]);
    let _ = unsafe { GlobalUnlock(hmem) };
    Some(text)
}

unsafe fn read_hdrop_locked() -> Option<Vec<String>> {
    let handle = unsafe { GetClipboardData(CF_HDROP.0 as u32).ok()? };
    let hdrop = HDROP(handle.0);
    let count = unsafe { DragQueryFileW(hdrop, 0xFFFF_FFFF, None) }.min(32);
    let mut files = Vec::new();
    for i in 0..count {
        let len = unsafe { DragQueryFileW(hdrop, i, None) };
        if len > 0 && len < 4096 {
            let mut buf = vec![0u16; (len + 1) as usize];
            let copied = unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };
            if copied > 0 {
                files.push(String::from_utf16_lossy(&buf[..copied as usize]));
            }
        }
    }
    (!files.is_empty()).then_some(files)
}

unsafe fn read_dib_bytes_locked() -> Option<Vec<u8>> {
    let handle = unsafe { GetClipboardData(CF_DIB.0 as u32).ok()? };
    let hmem = HGLOBAL(handle.0);
    let total_size = unsafe { GlobalSize(hmem) };
    if !(40..=64 * 1024 * 1024).contains(&total_size) {
        return None;
    }
    let ptr = unsafe { GlobalLock(hmem) as *const u8 };
    if ptr.is_null() {
        return None;
    }
    let copy = unsafe { std::slice::from_raw_parts(ptr, total_size).to_vec() };
    let _ = unsafe { GlobalUnlock(hmem) };
    Some(copy)
}

/// Checks Windows confidential clipboard markers (`ExcludeClipboardContentFromMonitorProcessing`
/// and `CanIncludeInClipboardHistory == 0`) used by password managers.
#[allow(dead_code)]
pub fn is_clipboard_confidential() -> bool {
    use windows::core::w;
    unsafe {
        let excl = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        if excl != 0 && IsClipboardFormatAvailable(excl).is_ok() {
            return true;
        }
        let hist = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if hist != 0 && IsClipboardFormatAvailable(hist).is_ok() && open_clipboard_owned(1) {
            let disallowed = read_u32_format_locked(hist) == Some(0);
            let _ = CloseClipboard();
            return disallowed;
        }
        false
    }
}

/// Returns the lowercase `.exe` filename of the clipboard owner or foreground window.
pub fn clipboard_source_app_name() -> String {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let mut hwnd = GetClipboardOwner().unwrap_or_default();
        if hwnd.is_invalid() {
            hwnd = GetForegroundWindow();
        }
        if hwnd.is_invalid() {
            return String::new();
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(handle);
        if !ok || len == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&full)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default()
    }
}
