//! Win32 single-line EDIT control subclassing: intercepts launcher navigation shortcuts
//! and suppresses `WM_CHAR` / `WM_SYSCHAR` control-character `MessageBeep` sounds.

use super::{EM_GETSEL, EM_SETSEL, WM_APP_EDIT_KEY, WM_APP_PASTE_INTERCEPT, WM_APP_SHOW_CONTEXT_MENU};
use crate::actions::clipboard_has_rich_attachment;
use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_BACK, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, PostMessageW, SendMessageW, WM_CHAR, WM_CONTEXTMENU, WM_KEYDOWN,
    WM_SYSCHAR, WM_SYSKEYDOWN,
};

pub(super) unsafe extern "system" fn edit_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid_subclass: usize,
    ref_data: usize,
) -> LRESULT {
    let parent = HWND(ref_data as *mut c_void);
    match msg {
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            let ctrl = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
            let shift = unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
            let mod_flags = (if ctrl { 1isize } else { 0 }) | (if shift { 4isize } else { 0 });

            let intercept = matches!(
                vk,
                x if x == VK_UP.0
                    || x == VK_DOWN.0
                    || x == VK_RETURN.0
                    || x == VK_ESCAPE.0
                    || x == VK_TAB.0
            );
            if intercept || (ctrl && (b'1' as u16..=b'9' as u16).contains(&vk)) {
                unsafe {
                    let _ = PostMessageW(
                        Some(parent),
                        WM_APP_EDIT_KEY,
                        WPARAM(vk as usize),
                        LPARAM(mod_flags),
                    );
                }
                return LRESULT(0);
            }
            if ctrl && vk == b'V' as u16 && clipboard_has_rich_attachment() {
                unsafe {
                    let _ = PostMessageW(Some(parent), WM_APP_PASTE_INTERCEPT, WPARAM(0), LPARAM(0));
                }
                return LRESULT(0);
            }
            if vk == VK_BACK.0 && unsafe { GetWindowTextLengthW(hwnd) } == 0 {
                unsafe {
                    let _ = PostMessageW(Some(parent), WM_APP_EDIT_KEY, WPARAM(vk as usize), LPARAM(0));
                }
            }
            if ctrl
                && matches!(
                    vk,
                    x if x == b'P' as u16 || x == 0xBC || x == b'E' as u16 || x == b'D' as u16
                )
            {
                unsafe {
                    let _ = PostMessageW(
                        Some(parent),
                        WM_APP_EDIT_KEY,
                        WPARAM(vk as usize),
                        LPARAM(mod_flags),
                    );
                }
                return LRESULT(0);
            }
            if ctrl && vk == b'C' as u16 {
                let (mut start, mut end) = (0u32, 0u32);
                unsafe {
                    SendMessageW(
                        hwnd,
                        EM_GETSEL,
                        Some(WPARAM(&mut start as *mut _ as usize)),
                        Some(LPARAM(&mut end as *mut _ as isize)),
                    );
                }
                if start == end {
                    unsafe {
                        let _ = PostMessageW(
                            Some(parent),
                            WM_APP_EDIT_KEY,
                            WPARAM(vk as usize),
                            LPARAM(1),
                        );
                    }
                    return LRESULT(0);
                }
            }
        }
        WM_CONTEXTMENU => {
            unsafe {
                let _ = PostMessageW(Some(parent), WM_APP_SHOW_CONTEXT_MENU, WPARAM(1), lparam);
            }
            return LRESULT(0);
        }
        WM_CHAR => {
            // Suppress single-line EDIT control MessageBeep on Esc/Enter/Tab/Ctrl-chord chars.
            let ch = wparam.0 as u32;
            if ch == 0x01 {
                unsafe {
                    let _ = SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                }
                return LRESULT(0);
            }
            if matches!(ch, 0x09 | 0x0A | 0x0D | 0x1B | 0x7F | 0x04 | 0x05 | 0x10) {
                return LRESULT(0);
            }
        }
        WM_SYSKEYDOWN => {
            let vk = wparam.0 as u16;
            if (b'1' as u16..=b'8' as u16).contains(&vk) {
                unsafe {
                    let _ = PostMessageW(Some(parent), WM_APP_EDIT_KEY, WPARAM(vk as usize), LPARAM(2));
                }
                return LRESULT(0);
            }
        }
        WM_SYSCHAR => {
            // Prevent DefWindowProc MessageBeep when pressing Alt+1..8 quick-launch chords.
            let ch = wparam.0 as u32;
            if (b'1' as u32..=b'8' as u32).contains(&ch) {
                return LRESULT(0);
            }
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
