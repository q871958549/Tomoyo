//! Opt-in zero-thread Win32 `WH_KEYBOARD_LL` override for conflicted global hotkeys.
//!
//! Mounted ONLY when `config.override_conflict_hotkeys == true` AND at least one Tomoyo
//! shortcut failed standard `RegisterHotKey` (e.g. held by a silent background service).
//! Runs directly on `tomoyo.exe`'s existing UI message loop (0 extra threads, 0 KB idle cost).

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, WH_KEYBOARD_LL, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const MAX_OVERRIDE_SLOTS: usize = 4;

static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);
static SLOT_COUNT: AtomicUsize = AtomicUsize::new(0);
static SLOTS: [AtomicU64; MAX_OVERRIDE_SLOTS] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static SWALLOW_UP_VK: AtomicU32 = AtomicU32::new(0);

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
    let raw = HOOK_HANDLE.swap(0, Ordering::SeqCst);
    if raw != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(raw as *mut c_void));
        }
        tracing::info!("Uninstalled low-level conflict override hook");
    }
}

#[inline]
fn current_mod_mask() -> u32 {
    let down = |vk: u16| unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
    let mut m = 0u32;
    if down(VK_MENU.0) || down(VK_LMENU.0) || down(VK_RMENU.0) { m |= 0x0001; }
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
    if (kb.flags.0 & LLKHF_INJECTED.0) != 0 {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }

    let msg = wparam.0 as u32;
    let vk = kb.vkCode;

    if matches!(msg, WM_KEYUP | WM_SYSKEYUP) {
        if SWALLOW_UP_VK.load(Ordering::Relaxed) == vk {
            SWALLOW_UP_VK.store(0, Ordering::Relaxed);
            return LRESULT(1);
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
            let cur_mods = *mod_mask.get_or_insert_with(current_mod_mask);
            let slot_mods = ((packed >> 16) & 0x000F) as u32;
            if cur_mods == slot_mods {
                let hotkey_id = ((packed >> 32) & 0xFFFF) as usize;
                // MOD_NOREPEAT behavior: only trigger once until key is released
                if SWALLOW_UP_VK.swap(vk, Ordering::Relaxed) != vk {
                    let raw_hwnd = TARGET_HWND.load(Ordering::Relaxed);
                    if raw_hwnd != 0 {
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(raw_hwnd as *mut c_void)),
                                WM_HOTKEY,
                                WPARAM(hotkey_id),
                                LPARAM(0),
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
