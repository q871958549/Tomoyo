//! IME isolation, foreground focus acquisition, and `VK_PROCESSKEY` hardware scancode
//! recovery for capture overlay and pin windows so Chinese IMEs never swallow `Shift+C` or
//! other single/modified shortcuts while typing inside `InlineTextEditor` keeps full native IME.

#![cfg(windows)]

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::Ime::{
    ImmAssociateContextEx, ImmGetVirtualKey, HIMC, IACE_DEFAULT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SetFocus, MAPVK_VSC_TO_VK_EX, VK_PROCESSKEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

/// Detaches Win32 IME/TSF composition from shortcut-only overlay / pin windows.
pub(crate) fn disable_ime_for_window(hwnd: HWND) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        let _ = ImmAssociateContextEx(hwnd, HIMC::default(), 0);
    }
}

/// Re-attaches default Win32 IME context on child text annotation editors so Chinese typing works.
pub(crate) fn enable_ime_for_window(hwnd: HWND) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        let _ = ImmAssociateContextEx(hwnd, HIMC::default(), IACE_DEFAULT);
    }
}

/// Reliably acquires foreground keyboard focus for the newly created capture overlay window.
pub(crate) fn activate_overlay_window(hwnd: HWND) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        let fg = GetForegroundWindow();
        let cur_tid = GetCurrentThreadId();
        let fg_tid = if !fg.is_invalid() && fg != hwnd {
            GetWindowThreadProcessId(fg, None)
        } else {
            0
        };
        let attached = fg_tid != 0 && fg_tid != cur_tid && AttachThreadInput(cur_tid, fg_tid, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        if attached {
            let _ = AttachThreadInput(cur_tid, fg_tid, false);
        }
    }
}

/// Recovers the true physical virtual key when a Chinese IME intercepts `WM_KEYDOWN` as
/// `VK_PROCESSKEY (0xE5)` by decoding the hardware scan code in `lParam[16..24]` (or `ImmGetVirtualKey`).
pub(crate) fn resolve_physical_vk(hwnd: HWND, raw_vk: u16, lparam: LPARAM) -> u16 {
    if raw_vk != VK_PROCESSKEY.0 {
        return raw_vk;
    }
    let scancode = ((lparam.0 as usize >> 16) & 0xFF) as u32;
    if scancode != 0 {
        let mapped = unsafe { MapVirtualKeyW(scancode, MAPVK_VSC_TO_VK_EX) } as u16;
        if mapped != 0 {
            return mapped;
        }
    }
    if !hwnd.is_invalid() {
        let imm_vk = unsafe { ImmGetVirtualKey(hwnd) } as u16;
        if imm_vk != 0 && imm_vk != VK_PROCESSKEY.0 {
            return imm_vk;
        }
    }
    raw_vk
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomoyo_core::CaptureToolHotkeys;

    #[test]
    fn vk_processkey_recovers_letter_c_from_scancode_lparam() {
        // US/QWERTY hardware scan code 0x2E corresponds to virtual key 'C'.
        let vk = resolve_physical_vk(HWND::default(), VK_PROCESSKEY.0, LPARAM(0x2E << 16));
        assert_eq!(vk, b'C' as u16);
        assert!(CaptureToolHotkeys::matches_combo("Shift+C", vk as u8, false, false, true));
    }

    #[test]
    fn normal_vk_passes_through_unchanged() {
        assert_eq!(resolve_physical_vk(HWND::default(), b'O' as u16, LPARAM(0)), b'O' as u16);
    }
}
