//! Lossless foreground selection & file-list context probe for Tomoyo Super Panel.
//!
//! Snapshots active clipboard formats (`CF_UNICODETEXT`, `CF_HDROP`, `CF_DIB`), suppresses
//! Tomoyo's own clipboard history listener, injects clean `Ctrl+C` while the target app still
//! owns focus, classifies the captured text/files, and restores the original clipboard cleanly.

use crate::actions::{read_clipboard_files, read_clipboard_text};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_CONTROL, VK_MENU, VK_SHIFT,
};

const MAX_SNAPSHOT_BYTES: usize = 24 * 1024 * 1024;
const MAX_SELECTED_CHARS: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Plain,
    Url,
    Json,
    Color,
    Path,
    Code,
}

impl TextKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "text",
            Self::Url => "url",
            Self::Json => "json",
            Self::Color => "color",
            Self::Path => "path",
            Self::Code => "code",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuperContext {
    Text { raw: String, kind: TextKind },
    Files { paths: Vec<String>, all_images: bool },
    Empty,
}

impl SuperContext {
    pub fn selected_text(&self) -> Option<&str> {
        match self {
            Self::Text { raw, .. } => Some(raw.as_str()),
            _ => None,
        }
    }
}

struct ClipboardBackup {
    captured_ok: bool,
    entries: Vec<(u32, Vec<u8>)>,
}

unsafe fn open_clipboard_retry() -> bool {
    for attempt in 0..4 {
        if unsafe { OpenClipboard(None).is_ok() } {
            return true;
        }
        if attempt < 3 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    false
}

impl ClipboardBackup {
    fn capture() -> Self {
        let formats = [CF_UNICODETEXT.0 as u32, CF_HDROP.0 as u32, CF_DIB.0 as u32];
        let mut entries = Vec::new();
        let captured_ok = unsafe {
            if !open_clipboard_retry() {
                return Self { captured_ok: false, entries };
            }
            for fmt in formats {
                if IsClipboardFormatAvailable(fmt).is_ok()
                    && let Ok(handle) = GetClipboardData(fmt)
                {
                    let hmem = HGLOBAL(handle.0);
                    let len = GlobalSize(hmem);
                    if len > 0 && len <= MAX_SNAPSHOT_BYTES {
                        let ptr = GlobalLock(hmem) as *const u8;
                        if !ptr.is_null() {
                            let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
                            let _ = GlobalUnlock(hmem);
                            entries.push((fmt, bytes));
                        }
                    }
                }
            }
            let _ = CloseClipboard();
            true
        };
        Self { captured_ok, entries }
    }

    fn restore(self) {
        if !self.captured_ok {
            return;
        }
        unsafe {
            if !open_clipboard_retry() {
                return;
            }
            let _ = EmptyClipboard();
            for (fmt, bytes) in self.entries {
                if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) {
                    let ptr = GlobalLock(hmem) as *mut u8;
                    if !ptr.is_null() {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
                        let _ = GlobalUnlock(hmem);
                        let _ = SetClipboardData(fmt, Some(HANDLE(hmem.0)));
                    }
                }
            }
            let _ = CloseClipboard();
        }
    }
}

/// Captures the foreground window's active text or file selection without disturbing the
/// user's clipboard contents or triggering Tomoyo's clipboard history plugin.
pub fn probe_foreground_selection(suppress_flag: &Arc<AtomicBool>) -> SuperContext {
    suppress_flag.store(true, Ordering::Release);
    let backup = ClipboardBackup::capture();
    let seq_before = unsafe { GetClipboardSequenceNumber() };

    send_ctrl_c_cleanly();

    let mut changed = false;
    let mut ctx = SuperContext::Empty;
    for _ in 0..12 {
        std::thread::sleep(Duration::from_millis(15));
        if unsafe { GetClipboardSequenceNumber() } != seq_before {
            changed = true;
            if let Some(files) = read_clipboard_files() {
                let all_images = !files.is_empty() && files.iter().all(|p| is_image_path(p));
                ctx = SuperContext::Files { paths: files, all_images };
            } else if let Some(txt) = read_clipboard_text() {
                let trimmed: String = txt.trim().chars().take(MAX_SELECTED_CHARS).collect();
                if !trimmed.is_empty() {
                    let kind = classify_text_kind(&trimmed);
                    ctx = SuperContext::Text { raw: trimmed, kind };
                }
            }
            break;
        }
    }

    if changed {
        backup.restore();
    }
    std::thread::sleep(Duration::from_millis(55));
    suppress_flag.store(false, Ordering::Release);
    ctx
}

fn send_ctrl_c_cleanly() {
    let make = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                ..Default::default()
            },
        },
    };
    let inputs = [
        make(VK_MENU, true),
        make(VK_SHIFT, true),
        make(VK_CONTROL, false),
        make(VIRTUAL_KEY(b'C' as u16), false),
        make(VIRTUAL_KEY(b'C' as u16), true),
        make(VK_CONTROL, true),
    ];
    unsafe {
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn is_image_path(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "ico")
}

/// Classifies selected text into a semantic kind for Super Panel contextual actions.
pub fn classify_text_kind(text: &str) -> TextKind {
    let t = text.trim();
    if is_http_like_url(t) {
        return TextKind::Url;
    }
    if is_color_literal(t) {
        return TextKind::Color;
    }
    if ((t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']')))
        && serde_json::from_str::<serde_json::Value>(t).is_ok()
    {
        return TextKind::Json;
    }
    if looks_like_windows_path(t) {
        return TextKind::Path;
    }
    if looks_like_code(t) {
        return TextKind::Code;
    }
    TextKind::Plain
}

pub fn normalize_url(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return None;
    }
    let low = t.to_ascii_lowercase();
    if low.starts_with("https://") || low.starts_with("http://") {
        tomoyo_core::is_safe_http_url(t).then(|| t.to_string())
    } else if low.starts_with("www.") {
        let full = format!("https://{t}");
        tomoyo_core::is_safe_http_url(&full).then_some(full)
    } else {
        None
    }
}

fn is_http_like_url(t: &str) -> bool {
    normalize_url(t).is_some()
}

fn is_color_literal(t: &str) -> bool {
    if let Some(hex) = t.strip_prefix('#') {
        return matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let low = t.to_ascii_lowercase();
    (low.starts_with("rgb(") || low.starts_with("rgba(")) && low.ends_with(')') && low.len() <= 36
}

fn looks_like_windows_path(t: &str) -> bool {
    if t.contains('\n') || t.len() < 3 || t.len() > 260 {
        return false;
    }
    let unquoted = t.trim_matches('"');
    let bytes = unquoted.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
        && Path::new(unquoted).exists()
}

fn looks_like_code(t: &str) -> bool {
    if t.lines().count() >= 3 && (t.contains('{') || t.contains("fn ") || t.contains("def ") || t.contains("const ") || t.contains("import ")) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_selected_text_kinds_accurately() {
        assert_eq!(classify_text_kind("https://github.com/q871958549/Tomoyo"), TextKind::Url);
        assert_eq!(classify_text_kind("www.rust-lang.org"), TextKind::Url);
        assert_eq!(classify_text_kind("#7C6AF7"), TextKind::Color);
        assert_eq!(classify_text_kind("rgb(16, 185, 129)"), TextKind::Color);
        assert_eq!(classify_text_kind(r#"{"ok":true,"n":42}"#), TextKind::Json);
        assert_eq!(classify_text_kind("serendipity"), TextKind::Plain);
        assert_eq!(classify_text_kind("选中文本快捷翻译"), TextKind::Plain);
        assert_eq!(
            classify_text_kind("fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}"),
            TextKind::Code
        );
        assert_eq!(normalize_url("www.example.com/a").as_deref(), Some("https://www.example.com/a"));
        assert!(normalize_url("javascript:alert(1)").is_none());
    }
}
