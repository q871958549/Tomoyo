//! Zero-thread Win32 `WH_KEYBOARD_LL` conflict rescue & bulletproof foreground activation.
//!
//! Mounted ONLY when `config.override_conflict_hotkeys == true` AND at least one Tomoyo
//! shortcut failed standard `RegisterHotKey` (e.g. held by a silent background service).
//! Runs directly on `tomoyo.exe`'s existing UI message loop (0 extra threads, 0 KB idle cost).

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, CallNextHookEx, GetForegroundWindow,
    GetWindowThreadProcessId, PostMessageW, SetForegroundWindow, SetWindowPos, SetWindowsHookExW,
    UnhookWindowsHookEx, ASFW_ANY, HHOOK, HWND_TOPMOST, KBDLLHOOKSTRUCT, KBDLLHOOKSTRUCT_FLAGS,
    LLKHF_ALTDOWN, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WH_KEYBOARD_LL, WM_HOTKEY, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const MAX_OVERRIDE_SLOTS: usize = 12;
/// Microsoft PowerToys / AutoHotkey `#MenuMaskKey` unassigned VK (`0xE8`) that cancels
/// bare-`Alt` (`SC_KEYMENU`) and bare-`Win` Start Menu activation when swallowing a chord.
const VK_MENU_MASK: u16 = 0xE8;
const OVERRIDE_EXTRA_INFO: usize = 0x544D_594B; // "TMYK"

static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);
static SLOT_COUNT: AtomicUsize = AtomicUsize::new(0);
static SLOTS: [AtomicU64; MAX_OVERRIDE_SLOTS] = [const { AtomicU64::new(0) }; MAX_OVERRIDE_SLOTS];
static SWALLOW_UP_VK: AtomicU32 = AtomicU32::new(0);
static SWALLOW_AT_MS: AtomicU32 = AtomicU32::new(0);

#[inline]
fn pack_slot(hotkey_id: i32, mods: u32, vk: u32) -> u64 {
    (((hotkey_id as u16) as u64) << 32) | ((((mods & 0x000F) as u16) as u64) << 16) | ((vk as u16) as u64)
}

/// Synchronizes the active override hook with the exact conflicted `(hotkey_id, mods, vk)` list.
/// Automatically uninstalls the hook when `entries` is empty.
pub(super) fn sync_conflict_override_hook(hwnd: HWND, entries: &[(i32, u32, u32)]) -> bool {
    let count = entries.len().min(MAX_OVERRIDE_SLOTS);
    TARGET_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    SLOT_COUNT.store(0, Ordering::SeqCst);
    for (i, &(id, mods, vk)) in entries.iter().take(count).enumerate() {
        SLOTS[i].store(pack_slot(id, mods, vk), Ordering::SeqCst);
    }
    SLOT_COUNT.store(count, Ordering::SeqCst);

    if count == 0 {
        uninstall_conflict_override_hook();
        return true;
    }

    if HOOK_HANDLE.load(Ordering::SeqCst) != 0 {
        return true;
    }
    match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(override_ll_proc), None, 0) } {
        Ok(hhook) => {
            HOOK_HANDLE.store(hhook.0 as isize, Ordering::SeqCst);
            tracing::info!(slots = count, "Installed low-level conflict override hook");
            true
        }
        Err(e) => {
            SLOT_COUNT.store(0, Ordering::SeqCst);
            tracing::warn!("SetWindowsHookExW(WH_KEYBOARD_LL) override failed: {e}");
            false
        }
    }
}

pub(super) fn uninstall_conflict_override_hook() {
    SLOT_COUNT.store(0, Ordering::SeqCst);
    SWALLOW_UP_VK.store(0, Ordering::SeqCst);
    SWALLOW_AT_MS.store(0, Ordering::SeqCst);
    let raw = HOOK_HANDLE.swap(0, Ordering::SeqCst);
    if raw != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(raw as *mut c_void));
        }
        tracing::info!("Uninstalled low-level conflict override hook");
    }
}

/// Sends a harmless synthetic `0xE8` key-down/up to prevent bare-`Alt` menu bar / Start Menu theft.
#[inline]
fn inject_menu_mask_key() {
    let mk = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(VK_MENU_MASK),
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                dwExtraInfo: OVERRIDE_EXTRA_INFO,
                ..Default::default()
            },
        },
    };
    let inputs = [mk(false), mk(true)];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// 3-stage Win32 foreground activator that reliably breaks Windows Foreground Lock even when
/// summoned via `WH_KEYBOARD_LL` override or from elevated/UWP foreground windows.
pub(crate) fn force_foreground_window(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            return;
        }
        let fg = GetForegroundWindow();
        let fg_tid = if !fg.is_invalid() { GetWindowThreadProcessId(fg, None) } else { 0 };
        let my_tid = GetCurrentThreadId();
        if fg_tid != 0 && fg_tid != my_tid && AttachThreadInput(my_tid, fg_tid, true).as_bool() {
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(my_tid, fg_tid, false);
        }
        if GetForegroundWindow() == hwnd {
            return;
        }
        // Final fallback: harmless synthetic unassigned key-up resets Foreground Lock cleanly.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let up = [INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(VK_MENU_MASK),
                    dwFlags: KEYEVENTF_KEYUP,
                    dwExtraInfo: OVERRIDE_EXTRA_INFO,
                    ..Default::default()
                },
            },
        }];
        SendInput(&up, std::mem::size_of::<INPUT>() as i32);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
}

#[inline]
fn is_modifier_vk(vk: u32) -> bool {
    matches!(
        vk as u16,
        x if x == VK_MENU.0
            || x == VK_LMENU.0
            || x == VK_RMENU.0
            || x == VK_CONTROL.0
            || x == VK_LCONTROL.0
            || x == VK_RCONTROL.0
            || x == VK_SHIFT.0
            || x == VK_LSHIFT.0
            || x == VK_RSHIFT.0
            || x == VK_LWIN.0
            || x == VK_RWIN.0
    )
}

#[inline]
fn current_mod_mask(flags: KBDLLHOOKSTRUCT_FLAGS) -> u32 {
    let down = |vk: u16| unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
    let mut m = 0u32;
    if (flags.0 & LLKHF_ALTDOWN.0) != 0 || down(VK_MENU.0) || down(VK_LMENU.0) || down(VK_RMENU.0) {
        m |= 0x0001;
    }
    if down(VK_CONTROL.0) || down(VK_LCONTROL.0) || down(VK_RCONTROL.0) { m |= 0x0002; }
    if down(VK_SHIFT.0) || down(VK_LSHIFT.0) || down(VK_RSHIFT.0) { m |= 0x0004; }
    if down(VK_LWIN.0) || down(VK_RWIN.0) { m |= 0x0008; }
    m
}

unsafe extern "system" fn override_ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }
    let count = SLOT_COUNT.load(Ordering::Relaxed);
    if count == 0 {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }
    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    if kb.dwExtraInfo == OVERRIDE_EXTRA_INFO {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }

    let msg = wparam.0 as u32;
    let vk = kb.vkCode;

    if matches!(msg, WM_KEYUP | WM_SYSKEYUP) {
        if SWALLOW_UP_VK.load(Ordering::Relaxed) == vk {
            SWALLOW_UP_VK.store(0, Ordering::Relaxed);
            SWALLOW_AT_MS.store(0, Ordering::Relaxed);
            return LRESULT(1);
        }
        // Releasing any modifier resets anti-repeat state so a lost key-up can never deadlock.
        if is_modifier_vk(vk) {
            SWALLOW_UP_VK.store(0, Ordering::Relaxed);
            SWALLOW_AT_MS.store(0, Ordering::Relaxed);
        }
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }

    if matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let mut mod_mask: Option<u32> = None;
        for slot in SLOTS.iter().take(count.min(MAX_OVERRIDE_SLOTS)) {
            let packed = slot.load(Ordering::Relaxed);
            let slot_vk = (packed & 0xFFFF) as u32;
            if slot_vk != vk {
                continue;
            }
            let cur_mods = *mod_mask.get_or_insert_with(|| current_mod_mask(kb.flags));
            let slot_mods = ((packed >> 16) & 0x000F) as u32;
            if cur_mods == slot_mods {
                let raw_hwnd = TARGET_HWND.load(Ordering::Relaxed);
                let self_hwnd = HWND(raw_hwnd as *mut c_void);
                if super::hotkey_avoid::should_avoid_hotkey(self_hwnd) {
                    return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
                }
                let hotkey_id = ((packed >> 32) & 0xFFFF) as usize;
                let now_ms = kb.time;
                let prev_vk = SWALLOW_UP_VK.load(Ordering::Relaxed);
                let prev_ms = SWALLOW_AT_MS.load(Ordering::Relaxed);
                let is_fresh = prev_vk != vk || now_ms.wrapping_sub(prev_ms) > 1800;
                SWALLOW_UP_VK.store(vk, Ordering::Relaxed);
                SWALLOW_AT_MS.store(now_ms, Ordering::Relaxed);
                if is_fresh {
                    if (slot_mods & (0x0001 | 0x0008)) != 0 {
                        inject_menu_mask_key();
                    }
                    if raw_hwnd != 0 {
                        unsafe {
                            let _ = PostMessageW(
                                Some(self_hwnd),
                                WM_HOTKEY,
                                WPARAM(hotkey_id),
                                LPARAM(1),
                            );
                        }
                    }
                }
                return LRESULT(1);
            }
        }
    }

    unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) }
}
