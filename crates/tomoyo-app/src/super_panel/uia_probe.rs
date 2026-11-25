//! Zero-clipboard Win32 UI Automation selection reader & terminal SIGINT guard.
//!
//! Reads active text selections directly from the focused accessibility element via
//! `IUIAutomationTextPattern` / `IUIAutomationTextPattern2` without touching the OS clipboard
//! or injecting synthetic `Ctrl+C`. Also protects interactive shells from accidental `SIGINT`.

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextPattern2,
    IUIAutomationTextRangeArray, UIA_TextPattern2Id, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

const MAX_UIA_RANGES: i32 = 4;

/// Attempts to capture the focused window's active text selection purely through Windows
/// UI Automation (`TextPattern2` → `TextPattern`) with **zero** clipboard access and
/// **zero** synthetic keyboard input.
pub fn try_read_focused_uia_selection(max_chars: usize) -> Option<String> {
    if max_chars == 0 {
        return None;
    }
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let should_uninit = hr.is_ok();
        if hr.is_err() && hr != RPC_E_CHANGED_MODE {
            tracing::debug!(?hr, "UIA COM initialization failed; falling back to clipboard probe");
            return None;
        }
        let result = read_focused_uia_selection_inner(max_chars);
        if should_uninit {
            CoUninitialize();
        }
        result
    }
}

unsafe fn read_focused_uia_selection_inner(max_chars: usize) -> Option<String> {
    let uia: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()? };
    let focused = unsafe { uia.GetFocusedElement().ok()? };

    let ranges = unsafe {
        focused
            .GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
            .ok()
            .and_then(|p| p.GetSelection().ok())
            .or_else(|| {
                focused
                    .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                    .ok()
                    .and_then(|p| p.GetSelection().ok())
            })
    }?;

    unsafe { extract_text_from_ranges(&ranges, max_chars) }
}

unsafe fn extract_text_from_ranges(
    ranges: &IUIAutomationTextRangeArray,
    max_chars: usize,
) -> Option<String> {
    let count = unsafe { ranges.Length().ok()? }.clamp(0, MAX_UIA_RANGES);
    if count <= 0 {
        return None;
    }
    let mut parts: Vec<String> = Vec::with_capacity(count as usize);
    let mut remaining = max_chars;
    for i in 0..count {
        if remaining == 0 {
            break;
        }
        let Ok(range) = (unsafe { ranges.GetElement(i) }) else {
            continue;
        };
        let limit = remaining.min(i32::MAX as usize) as i32;
        let Ok(bstr) = (unsafe { range.GetText(limit) }) else {
            continue;
        };
        let raw = bstr.to_string();
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let taken: String = trimmed.chars().take(remaining).collect();
            let used = taken.chars().count();
            remaining = remaining.saturating_sub(used);
            parts.push(taken);
        }
    }
    join_uia_ranges(&parts, max_chars)
}

fn join_uia_ranges(parts: &[String], max_chars: usize) -> Option<String> {
    if parts.is_empty() || max_chars == 0 {
        return None;
    }
    let joined: String = parts.join("\n").trim().chars().take(max_chars).collect();
    (!joined.is_empty()).then_some(joined)
}

/// Returns `true` when the active foreground window belongs to a terminal or SSH client
/// where sending a fallback `Ctrl+C` without an active selection could emit `SIGINT` and
/// terminate a running command.
pub fn is_terminal_foreground_window() -> bool {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return false;
    }
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(fg, Some(&mut pid));
    }
    if pid == 0 {
        return false;
    }
    crate::window::hotkey_avoid::query_process_basename_lower(pid)
        .as_deref()
        .is_some_and(is_terminal_exe_name)
}

fn is_terminal_exe_name(exe_lower: &str) -> bool {
    matches!(
        exe_lower,
        "windowsterminal.exe"
            | "wt.exe"
            | "cmd.exe"
            | "pwsh.exe"
            | "powershell.exe"
            | "conhost.exe"
            | "wezterm-gui.exe"
            | "alacritty.exe"
            | "mintty.exe"
            | "kitty.exe"
            | "tabby.exe"
            | "hyper.exe"
            | "xshell.exe"
            | "securecrt.exe"
            | "mobaxterm.exe"
            | "putty.exe"
            | "finalshell.exe"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_terminal_executables_and_joins_ranges() {
        assert!(is_terminal_exe_name("windowsterminal.exe"));
        assert!(is_terminal_exe_name("pwsh.exe"));
        assert!(is_terminal_exe_name("xshell.exe"));
        assert!(!is_terminal_exe_name("code.exe"));
        assert!(!is_terminal_exe_name("onecommander.exe"));

        assert_eq!(join_uia_ranges(&[], 100), None);
        assert_eq!(
            join_uia_ranges(&["hello".into(), "world".into()], 100).as_deref(),
            Some("hello\nworld")
        );
        assert_eq!(
            join_uia_ranges(&["abcdef".into()], 4).as_deref(),
            Some("abcd")
        );
    }
}
