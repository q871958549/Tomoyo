//! Zero-thread Win32 `WH_MOUSE_LL` trigger engine for Tomoyo Super Panel.
//!
//! Supports stationary right-button hold (default 320 ms) and middle-button hold/click
//! directly on `tomoyo.exe`'s existing UI message loop (0 extra threads, < 5 us callback).
//! Automatically disarms on drag (> 8 px) so gestures/scrolling stay untouched, swallows
//! only the matching `WM_RBUTTONUP` / `WM_MBUTTONUP` when triggered to suppress duplicate
//! OS context menus, and honors fullscreen/remote-desktop/custom avoidance lists.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::Instant;
use tomoyo_core::SuperPanelConfig;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_MBUTTONDOWN,
    WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
};

pub const BTN_NONE: u8 = 0;
pub const BTN_RIGHT: u8 = 1;
pub const BTN_MIDDLE: u8 = 2;

const MIDDLE_OFF: u8 = 0;
const MIDDLE_HOLD: u8 = 1;
const MIDDLE_CLICK: u8 = 2;

const DRAG_CANCEL_PX: i32 = 8;
const MIDDLE_CLICK_MAX_MS: u32 = 420;
const SWALLOW_TTL_MS: u32 = 2000;
const LLMHF_INJECTED_BIT: u32 = 0x0000_0001;

static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);
static MSG_ARM: AtomicU32 = AtomicU32::new(0);
static MSG_FIRE: AtomicU32 = AtomicU32::new(0);

static RIGHT_HOLD_ON: AtomicBool = AtomicBool::new(false);
static MIDDLE_MODE: AtomicU8 = AtomicU8::new(MIDDLE_OFF);
static PANEL_OPEN: AtomicBool = AtomicBool::new(false);

static ARM_BUTTON: AtomicU8 = AtomicU8::new(BTN_NONE);
static ARM_GEN: AtomicU32 = AtomicU32::new(0);
static DOWN_X: AtomicI32 = AtomicI32::new(0);
static DOWN_Y: AtomicI32 = AtomicI32::new(0);
static DOWN_TIME: AtomicU32 = AtomicU32::new(0);

static SWALLOW_UP_BUTTON: AtomicU8 = AtomicU8::new(BTN_NONE);
static SWALLOW_UP_AT: AtomicU32 = AtomicU32::new(0);
static EPOCH: OnceLock<Instant> = OnceLock::new();

#[inline]
fn now_ms() -> u32 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u32
}

#[inline]
pub fn set_panel_open(open: bool) {
    PANEL_OPEN.store(open, Ordering::Release);
    if open {
        ARM_BUTTON.store(BTN_NONE, Ordering::Release);
    }
}

#[inline]
pub fn is_panel_open() -> bool {
    PANEL_OPEN.load(Ordering::Acquire)
}

#[inline]
fn armed_screen_point() -> POINT {
    POINT {
        x: DOWN_X.load(Ordering::Acquire),
        y: DOWN_Y.load(Ordering::Acquire),
    }
}

/// Synchronizes the zero-thread `WH_MOUSE_LL` hook with `SuperPanelConfig`.
/// Unhooks cleanly when disabled or when both right & middle triggers are turned off.
pub fn sync_trigger_hook(hwnd: HWND, cfg: &SuperPanelConfig, msg_arm: u32, msg_fire: u32) {
    TARGET_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    MSG_ARM.store(msg_arm, Ordering::SeqCst);
    MSG_FIRE.store(msg_fire, Ordering::SeqCst);

    let mid_mode = match cfg.middle_trigger.as_str() {
        "click" => MIDDLE_CLICK,
        "off" => MIDDLE_OFF,
        _ => MIDDLE_HOLD,
    };
    let active = cfg.enabled && (cfg.trigger_right_hold || mid_mode != MIDDLE_OFF);
    RIGHT_HOLD_ON.store(cfg.enabled && cfg.trigger_right_hold, Ordering::SeqCst);
    MIDDLE_MODE.store(if cfg.enabled { mid_mode } else { MIDDLE_OFF }, Ordering::SeqCst);

    if !active {
        uninstall_trigger_hook();
        return;
    }
    if HOOK_HANDLE.load(Ordering::SeqCst) != 0 {
        return;
    }
    match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_ll_proc), None, 0) } {
        Ok(hhook) => {
            HOOK_HANDLE.store(hhook.0 as isize, Ordering::SeqCst);
            tracing::info!(
                right_hold = cfg.trigger_right_hold,
                middle = %cfg.middle_trigger,
                hold_ms = cfg.hold_ms,
                "Installed low-level mouse trigger hook for Super Panel"
            );
        }
        Err(e) => {
            tracing::warn!("SetWindowsHookExW(WH_MOUSE_LL) failed for Super Panel: {e}");
        }
    }
}

pub fn uninstall_trigger_hook() {
    RIGHT_HOLD_ON.store(false, Ordering::SeqCst);
    MIDDLE_MODE.store(MIDDLE_OFF, Ordering::SeqCst);
    ARM_BUTTON.store(BTN_NONE, Ordering::SeqCst);
    SWALLOW_UP_BUTTON.store(BTN_NONE, Ordering::SeqCst);
    let raw = HOOK_HANDLE.swap(0, Ordering::SeqCst);
    if raw != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(raw as *mut c_void));
        }
        tracing::info!("Uninstalled Super Panel mouse trigger hook");
    }
}

/// Validates that a hold timer expiration still corresponds to a stationary physical press,
/// and arms the matching button-up swallow guard to prevent duplicate OS context menus.
pub fn consume_hold_if_valid(expected_gen: u32, btn: u8) -> Option<POINT> {
    if PANEL_OPEN.load(Ordering::Acquire) {
        return None;
    }
    if ARM_BUTTON.load(Ordering::Acquire) != btn || ARM_GEN.load(Ordering::Acquire) != expected_gen {
        return None;
    }
    let vk = match btn {
        BTN_RIGHT => VK_RBUTTON.0 as i32,
        BTN_MIDDLE => VK_MBUTTON.0 as i32,
        _ => return None,
    };
    let still_down = unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
    if !still_down {
        ARM_BUTTON.store(BTN_NONE, Ordering::Release);
        return None;
    }
    ARM_BUTTON.store(BTN_NONE, Ordering::Release);
    SWALLOW_UP_AT.store(now_ms(), Ordering::Release);
    SWALLOW_UP_BUTTON.store(btn, Ordering::Release);
    Some(armed_screen_point())
}

/// Returns `true` when Super Panel must yield to the foreground window (fullscreen game/video,
/// remote desktop client, custom excluded process, or Tomoyo's own launcher/capture/webview UI).
pub fn should_avoid_super_panel(launcher_hwnd: HWND, extra_excluded: &[String]) -> bool {
    unsafe {
        if !launcher_hwnd.is_invalid() && IsWindowVisible(launcher_hwnd).as_bool() {
            return true;
        }
        if crate::window::hotkey_avoid::should_avoid_hotkey(launcher_hwnd) {
            return true;
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == launcher_hwnd {
            return true;
        }
        let mut fg_pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut fg_pid));
        if fg_pid == 0 || fg_pid == std::process::id() {
            return true;
        }
        if let Some(exe) = crate::window::hotkey_avoid::query_process_basename_lower(fg_pid) {
            if matches!(
                exe.as_str(),
                "tomoyo.exe" | "tomoyo-capture.exe" | "tomoyo-webview-host.exe"
            ) {
                return true;
            }
            if extra_excluded.iter().any(|e| e.eq_ignore_ascii_case(&exe)) {
                tracing::debug!(exe = %exe, "Yielding Super Panel to excluded foreground app");
                return true;
            }
        }
    }
    false
}

#[inline]
fn moved_past_threshold(x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    (x1 - x0).abs() > DRAG_CANCEL_PX || (y1 - y0).abs() > DRAG_CANCEL_PX
}

#[inline]
fn within_swallow_window(now_ms: u32, armed_ms: u32) -> bool {
    now_ms.wrapping_sub(armed_ms) <= SWALLOW_TTL_MS
}

unsafe extern "system" fn mouse_ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && lparam.0 != 0 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if (info.flags & LLMHF_INJECTED_BIT) == 0 && !PANEL_OPEN.load(Ordering::Acquire) {
            let msg = wparam.0 as u32;
            match msg {
                WM_RBUTTONDOWN if RIGHT_HOLD_ON.load(Ordering::Relaxed) => {
                    let seq = ARM_GEN.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
                    DOWN_X.store(info.pt.x, Ordering::Release);
                    DOWN_Y.store(info.pt.y, Ordering::Release);
                    DOWN_TIME.store(info.time, Ordering::Release);
                    ARM_BUTTON.store(BTN_RIGHT, Ordering::Release);
                    post_host(MSG_ARM.load(Ordering::Relaxed), seq as usize, BTN_RIGHT as isize);
                }
                WM_RBUTTONUP => {
                    if ARM_BUTTON.load(Ordering::Acquire) == BTN_RIGHT {
                        ARM_BUTTON.store(BTN_NONE, Ordering::Release);
                    }
                    if SWALLOW_UP_BUTTON.load(Ordering::Acquire) == BTN_RIGHT {
                        SWALLOW_UP_BUTTON.store(BTN_NONE, Ordering::Release);
                        if within_swallow_window(now_ms(), SWALLOW_UP_AT.load(Ordering::Acquire)) {
                            return LRESULT(1);
                        }
                    }
                }
                WM_MBUTTONDOWN => {
                    let mode = MIDDLE_MODE.load(Ordering::Relaxed);
                    if mode == MIDDLE_HOLD || mode == MIDDLE_CLICK {
                        let seq = ARM_GEN.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
                        DOWN_X.store(info.pt.x, Ordering::Release);
                        DOWN_Y.store(info.pt.y, Ordering::Release);
                        DOWN_TIME.store(info.time, Ordering::Release);
                        ARM_BUTTON.store(BTN_MIDDLE, Ordering::Release);
                        if mode == MIDDLE_HOLD {
                            post_host(MSG_ARM.load(Ordering::Relaxed), seq as usize, BTN_MIDDLE as isize);
                        }
                    }
                }
                WM_MBUTTONUP => {
                    let was_armed = ARM_BUTTON.load(Ordering::Acquire) == BTN_MIDDLE;
                    if was_armed {
                        ARM_BUTTON.store(BTN_NONE, Ordering::Release);
                    }
                    if SWALLOW_UP_BUTTON.load(Ordering::Acquire) == BTN_MIDDLE {
                        SWALLOW_UP_BUTTON.store(BTN_NONE, Ordering::Release);
                        if within_swallow_window(now_ms(), SWALLOW_UP_AT.load(Ordering::Acquire)) {
                            return LRESULT(1);
                        }
                    }
                    if was_armed && MIDDLE_MODE.load(Ordering::Relaxed) == MIDDLE_CLICK {
                        let elapsed = info.time.wrapping_sub(DOWN_TIME.load(Ordering::Acquire));
                        let stationary = !moved_past_threshold(
                            DOWN_X.load(Ordering::Acquire),
                            DOWN_Y.load(Ordering::Acquire),
                            info.pt.x,
                            info.pt.y,
                        );
                        if stationary && elapsed <= MIDDLE_CLICK_MAX_MS {
                            let host_hwnd = HWND(TARGET_HWND.load(Ordering::Acquire) as *mut c_void);
                            if !should_avoid_super_panel(host_hwnd, &[]) {
                                post_host(MSG_FIRE.load(Ordering::Relaxed), BTN_MIDDLE as usize, 0);
                                return LRESULT(1);
                            }
                        }
                    }
                }
                WM_MOUSEMOVE => {
                    if ARM_BUTTON.load(Ordering::Relaxed) != BTN_NONE
                        && moved_past_threshold(
                            DOWN_X.load(Ordering::Relaxed),
                            DOWN_Y.load(Ordering::Relaxed),
                            info.pt.x,
                            info.pt.y,
                        )
                    {
                        ARM_BUTTON.store(BTN_NONE, Ordering::Release);
                    }
                }
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[inline]
fn post_host(msg: u32, wparam: usize, lparam: isize) {
    let raw = TARGET_HWND.load(Ordering::Acquire);
    if raw != 0 && msg != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw as *mut c_void)),
                msg,
                WPARAM(wparam),
                LPARAM(lparam),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_and_swallow_window_guards_work() {
        assert!(!moved_past_threshold(100, 200, 108, 208));
        assert!(moved_past_threshold(100, 200, 109, 200));
        assert!(moved_past_threshold(100, 200, 100, 191));
        assert!(within_swallow_window(5000, 3500));
        assert!(!within_swallow_window(7000, 3500));
        // Wrap-around tick safety
        assert!(within_swallow_window(500, u32::MAX - 500));
    }
}
