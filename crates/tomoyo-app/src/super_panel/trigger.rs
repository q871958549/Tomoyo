//! Zero-thread Win32 `WH_MOUSE_LL` trigger & click-outside dismiss hook for Tomoyo Super Panel.
//!
//! Uses strictly balanced **Swallow-and-Replay** (`(0 DOWN, 0 UP)` when Super Panel triggers,
//! `(1 DOWN, 1 UP)` for normal short clicks, drags, or chords) so `win32k` NEVER desynchronizes
//! button state or locks implicit mouse capture onto background windows / Taskbar.

pub use super::trigger_guard::should_avoid_super_panel;
use super::trigger_guard::{
    replay_mouse_button, set_excluded_apps, should_bypass_on_down, REPLAY_EXTRA_INFO,
};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::Instant;
use tomoyo_core::SuperPanelConfig;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, MSLLHOOKSTRUCT,
    WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_XBUTTONDOWN,
};

pub const BTN_NONE: u8 = 0;
pub const BTN_RIGHT: u8 = 1;
pub const BTN_MIDDLE: u8 = 2;

const MODE_OFF: u8 = 0;
const MODE_RIGHT_HOLD: u8 = 1;
const MODE_MIDDLE_HOLD: u8 = 2;
const MODE_MIDDLE_CLICK: u8 = 3;

const DRAG_CANCEL_PX: i32 = 8;
const MIDDLE_CLICK_MAX_MS: u32 = 420;
const HOLD_MAX_AGE_MS: u32 = 2500;
const DISMISS_GRACE_MS: u32 = 80;
const LLMHF_INJECTED_BIT: u32 = 0x0000_0001;

static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);
static MSG_ARM: AtomicU32 = AtomicU32::new(0);
static MSG_FIRE: AtomicU32 = AtomicU32::new(0);
static TRIGGER_MODE: AtomicU8 = AtomicU8::new(MODE_OFF);

static PANEL_OPEN: AtomicBool = AtomicBool::new(false);
static PANEL_HWND: AtomicIsize = AtomicIsize::new(0);
static PANEL_DISMISS_MSG: AtomicU32 = AtomicU32::new(0);
static PANEL_OPENED_AT: AtomicU32 = AtomicU32::new(0);
static PANEL_L: AtomicI32 = AtomicI32::new(0);
static PANEL_T: AtomicI32 = AtomicI32::new(0);
static PANEL_R: AtomicI32 = AtomicI32::new(0);
static PANEL_B: AtomicI32 = AtomicI32::new(0);

static ARM_BUTTON: AtomicU8 = AtomicU8::new(BTN_NONE);
static ARM_GEN: AtomicU32 = AtomicU32::new(0);
static DOWN_X: AtomicI32 = AtomicI32::new(0);
static DOWN_Y: AtomicI32 = AtomicI32::new(0);
static DOWN_TIME: AtomicU32 = AtomicU32::new(0);
static DOWN_MS: AtomicU32 = AtomicU32::new(0);

static SWALLOWED_DOWN: AtomicU8 = AtomicU8::new(BTN_NONE);
static SWALLOW_UP_BUTTON: AtomicU8 = AtomicU8::new(BTN_NONE);
static EPOCH: OnceLock<Instant> = OnceLock::new();

#[inline]
fn now_ms() -> u32 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u32
}

/// Registers the active Super Panel screen bounds so outside clicks anywhere on Windows
/// dismiss it immediately and deterministically (even over taskbar or elevated apps).
pub fn set_panel_bounds(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, dismiss_msg: u32, fresh_open: bool) {
    PANEL_L.store(x, Ordering::Release);
    PANEL_T.store(y, Ordering::Release);
    PANEL_R.store(x + w, Ordering::Release);
    PANEL_B.store(y + h, Ordering::Release);
    PANEL_HWND.store(hwnd.0 as isize, Ordering::Release);
    PANEL_DISMISS_MSG.store(dismiss_msg, Ordering::Release);
    if fresh_open {
        PANEL_OPENED_AT.store(now_ms(), Ordering::Release);
    }
    ARM_BUTTON.store(BTN_NONE, Ordering::Release);
    PANEL_OPEN.store(true, Ordering::Release);
}

#[inline]
pub fn clear_panel_open() {
    PANEL_OPEN.store(false, Ordering::Release);
    PANEL_HWND.store(0, Ordering::Release);
}

#[inline]
pub fn is_panel_open() -> bool {
    PANEL_OPEN.load(Ordering::Acquire)
}

#[inline]
fn armed_screen_point() -> POINT {
    POINT { x: DOWN_X.load(Ordering::Acquire), y: DOWN_Y.load(Ordering::Acquire) }
}

/// Synchronizes the zero-thread `WH_MOUSE_LL` hook with `SuperPanelConfig`.
pub fn sync_trigger_hook(hwnd: HWND, cfg: &SuperPanelConfig, msg_arm: u32, msg_fire: u32) {
    TARGET_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    MSG_ARM.store(msg_arm, Ordering::SeqCst);
    MSG_FIRE.store(msg_fire, Ordering::SeqCst);
    set_excluded_apps(&cfg.excluded_apps);

    let mode = if !cfg.enabled {
        MODE_OFF
    } else {
        match cfg.trigger_mode.as_str() {
            "middle_hold" => MODE_MIDDLE_HOLD,
            "middle_click" => MODE_MIDDLE_CLICK,
            _ => MODE_RIGHT_HOLD,
        }
    };
    TRIGGER_MODE.store(mode, Ordering::SeqCst);

    if mode == MODE_OFF {
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
                mode = %cfg.trigger_mode,
                hold_ms = cfg.hold_ms,
                "Installed balanced low-level mouse trigger hook for Super Panel"
            );
        }
        Err(e) => tracing::warn!("SetWindowsHookExW(WH_MOUSE_LL) failed for Super Panel: {e}"),
    }
}

pub fn uninstall_trigger_hook() {
    TRIGGER_MODE.store(MODE_OFF, Ordering::SeqCst);
    ARM_BUTTON.store(BTN_NONE, Ordering::SeqCst);
    SWALLOWED_DOWN.store(BTN_NONE, Ordering::SeqCst);
    SWALLOW_UP_BUTTON.store(BTN_NONE, Ordering::SeqCst);
    clear_panel_open();
    let raw = HOOK_HANDLE.swap(0, Ordering::SeqCst);
    if raw != 0 {
        unsafe { let _ = UnhookWindowsHookEx(HHOOK(raw as *mut c_void)); }
        tracing::info!("Uninstalled Super Panel mouse trigger hook");
    }
}

/// Validates that a hold timer expiration still matches a stationary swallowed physical press,
/// and marks its upcoming button-up to be silently dropped (yielding 0 DOWN + 0 UP to `win32k`).
pub fn consume_hold_if_valid(expected_gen: u32, btn: u8) -> Option<POINT> {
    if PANEL_OPEN.load(Ordering::Acquire)
        || ARM_BUTTON.load(Ordering::Acquire) != btn
        || ARM_GEN.load(Ordering::Acquire) != expected_gen
        || SWALLOWED_DOWN.load(Ordering::Acquire) != btn
        || !within_window(now_ms(), DOWN_MS.load(Ordering::Acquire), HOLD_MAX_AGE_MS)
    {
        return None;
    }
    ARM_BUTTON.store(BTN_NONE, Ordering::Release);
    SWALLOW_UP_BUTTON.store(btn, Ordering::Release);
    Some(armed_screen_point())
}

/// If a hold timer is rejected while the button is still swallowed, replays the synthetic
/// button-down so the user's real physical button-up pairs cleanly with `win32k`.
pub fn abort_armed_hold(expected_gen: u32, btn: u8) {
    if ARM_GEN.load(Ordering::Acquire) == expected_gen
        && SWALLOWED_DOWN.compare_exchange(btn, BTN_NONE, Ordering::AcqRel, Ordering::Relaxed).is_ok()
    {
        ARM_BUTTON.store(BTN_NONE, Ordering::Release);
        SWALLOW_UP_BUTTON.store(BTN_NONE, Ordering::Release);
        replay_mouse_button(btn, true, false);
    }
}

#[inline]
fn release_swallowed_into_real_down() {
    let btn = SWALLOWED_DOWN.swap(BTN_NONE, Ordering::AcqRel);
    ARM_BUTTON.store(BTN_NONE, Ordering::Release);
    SWALLOW_UP_BUTTON.store(BTN_NONE, Ordering::Release);
    if btn != BTN_NONE {
        replay_mouse_button(btn, true, false);
    }
}

#[inline]
fn moved_past_threshold(x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    (x1 - x0).abs() > DRAG_CANCEL_PX || (y1 - y0).abs() > DRAG_CANCEL_PX
}

#[inline]
fn within_window(now: u32, since: u32, limit: u32) -> bool {
    now.wrapping_sub(since) <= limit
}

#[inline]
fn point_in_rect(x: i32, y: i32, l: i32, t: i32, r: i32, b: i32) -> bool {
    x >= l && x < r && y >= t && y < b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpResolution {
    PassThrough,
    SwallowSilent,
    FireMiddleAndSwallow,
    ReplayClickAndSwallow,
}

fn resolve_button_up(
    btn: u8,
    swallowed_down: u8,
    was_armed: bool,
    was_fired: bool,
    mode: u8,
    stationary: bool,
    elapsed_ms: u32,
) -> UpResolution {
    if swallowed_down != btn {
        return UpResolution::PassThrough;
    }
    if was_fired {
        return UpResolution::SwallowSilent;
    }
    if mode == MODE_MIDDLE_CLICK
        && btn == BTN_MIDDLE
        && was_armed
        && stationary
        && elapsed_ms <= MIDDLE_CLICK_MAX_MS
    {
        return UpResolution::FireMiddleAndSwallow;
    }
    UpResolution::ReplayClickAndSwallow
}

fn arm_button(btn: u8, info: &MSLLHOOKSTRUCT, post_arm_timer: bool) {
    let seq = ARM_GEN.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    DOWN_X.store(info.pt.x, Ordering::Release);
    DOWN_Y.store(info.pt.y, Ordering::Release);
    DOWN_TIME.store(info.time, Ordering::Release);
    DOWN_MS.store(now_ms(), Ordering::Release);
    SWALLOW_UP_BUTTON.store(BTN_NONE, Ordering::Release);
    SWALLOWED_DOWN.store(btn, Ordering::Release);
    ARM_BUTTON.store(btn, Ordering::Release);
    if post_arm_timer {
        post_host(MSG_ARM.load(Ordering::Relaxed), seq as usize, btn as isize);
    }
}

unsafe extern "system" fn mouse_ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && lparam.0 != 0 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if (info.flags & LLMHF_INJECTED_BIT) == 0 && info.dwExtraInfo != REPLAY_EXTRA_INFO {
            let msg = wparam.0 as u32;
            let mode = TRIGGER_MODE.load(Ordering::Relaxed);

            // 1. Resolve matched physical button-up for any swallowed DOWN first.
            if matches!(msg, WM_RBUTTONUP | WM_MBUTTONUP) {
                let btn = if msg == WM_RBUTTONUP { BTN_RIGHT } else { BTN_MIDDLE };
                let sw = SWALLOWED_DOWN.load(Ordering::Acquire);
                if sw == btn {
                    SWALLOWED_DOWN.store(BTN_NONE, Ordering::Release);
                    let was_armed = ARM_BUTTON.swap(BTN_NONE, Ordering::AcqRel) == btn;
                    let was_fired = SWALLOW_UP_BUTTON.swap(BTN_NONE, Ordering::AcqRel) == btn;
                    let elapsed = info.time.wrapping_sub(DOWN_TIME.load(Ordering::Acquire));
                    let stationary = !moved_past_threshold(
                        DOWN_X.load(Ordering::Acquire),
                        DOWN_Y.load(Ordering::Acquire),
                        info.pt.x,
                        info.pt.y,
                    );
                    match resolve_button_up(btn, sw, was_armed, was_fired, mode, stationary, elapsed) {
                        UpResolution::SwallowSilent => return LRESULT(1),
                        UpResolution::FireMiddleAndSwallow => {
                            post_host(MSG_FIRE.load(Ordering::Relaxed), BTN_MIDDLE as usize, 0);
                            return LRESULT(1);
                        }
                        UpResolution::ReplayClickAndSwallow => {
                            replay_mouse_button(btn, true, true);
                            return LRESULT(1);
                        }
                        UpResolution::PassThrough => {}
                    }
                }
            }

            // 2. Outside-click dismiss while Super Panel is open (passes click straight through).
            if PANEL_OPEN.load(Ordering::Acquire) {
                if matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN)
                    && !within_window(now_ms(), PANEL_OPENED_AT.load(Ordering::Acquire), DISMISS_GRACE_MS)
                    && !point_in_rect(
                        info.pt.x, info.pt.y,
                        PANEL_L.load(Ordering::Acquire), PANEL_T.load(Ordering::Acquire),
                        PANEL_R.load(Ordering::Acquire), PANEL_B.load(Ordering::Acquire),
                    )
                {
                    let p_hwnd = PANEL_HWND.load(Ordering::Acquire);
                    let p_msg = PANEL_DISMISS_MSG.load(Ordering::Acquire);
                    if p_hwnd != 0 && p_msg != 0 {
                        unsafe {
                            let _ = PostMessageW(Some(HWND(p_hwnd as *mut c_void)), p_msg, WPARAM(0), LPARAM(0));
                        }
                    }
                }
                return unsafe { CallNextHookEx(None, code, wparam, lparam) };
            }

            // 3. Arm balanced Swallow-and-Replay gestures when panel is closed.
            let host_hwnd = HWND(TARGET_HWND.load(Ordering::Acquire) as *mut c_void);
            match msg {
                WM_RBUTTONDOWN if mode == MODE_RIGHT_HOLD => {
                    if SWALLOWED_DOWN.load(Ordering::Acquire) == BTN_NONE
                        && !should_bypass_on_down(info.pt, host_hwnd)
                    {
                        arm_button(BTN_RIGHT, info, true);
                        return LRESULT(1);
                    }
                }
                WM_MBUTTONDOWN if mode == MODE_MIDDLE_HOLD || mode == MODE_MIDDLE_CLICK => {
                    if SWALLOWED_DOWN.load(Ordering::Acquire) == BTN_NONE
                        && !should_bypass_on_down(info.pt, host_hwnd)
                    {
                        arm_button(BTN_MIDDLE, info, mode == MODE_MIDDLE_HOLD);
                        return LRESULT(1);
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
                        release_swallowed_into_real_down();
                    }
                }
                WM_LBUTTONDOWN | WM_XBUTTONDOWN => {
                    if ARM_BUTTON.load(Ordering::Relaxed) != BTN_NONE {
                        release_swallowed_into_real_down();
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
            let _ = PostMessageW(Some(HWND(raw as *mut c_void)), msg, WPARAM(wparam), LPARAM(lparam));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::super_panel::trigger_guard::is_protected_shell_class;

    #[test]
    fn movement_bounds_and_balanced_swallow_replay_transitions_work() {
        assert!(!moved_past_threshold(100, 200, 108, 208));
        assert!(moved_past_threshold(100, 200, 109, 200));
        assert!(point_in_rect(120, 140, 100, 100, 300, 300));
        assert!(!point_in_rect(300, 140, 100, 100, 300, 300));
        assert!(is_protected_shell_class("Shell_TrayWnd"));
        assert!(is_protected_shell_class("#32768"));
        assert!(!is_protected_shell_class("Chrome_WidgetWin_1"));
        assert_eq!(
            resolve_button_up(BTN_RIGHT, BTN_RIGHT, false, true, MODE_RIGHT_HOLD, true, 500),
            UpResolution::SwallowSilent
        );
        assert_eq!(
            resolve_button_up(BTN_RIGHT, BTN_RIGHT, true, false, MODE_RIGHT_HOLD, true, 120),
            UpResolution::ReplayClickAndSwallow
        );
        assert_eq!(
            resolve_button_up(BTN_MIDDLE, BTN_MIDDLE, true, false, MODE_MIDDLE_CLICK, true, 140),
            UpResolution::FireMiddleAndSwallow
        );
        assert_eq!(
            resolve_button_up(BTN_MIDDLE, BTN_MIDDLE, true, false, MODE_MIDDLE_CLICK, true, 600),
            UpResolution::ReplayClickAndSwallow
        );
        assert_eq!(
            resolve_button_up(BTN_RIGHT, BTN_NONE, false, false, MODE_RIGHT_HOLD, true, 100),
            UpResolution::PassThrough
        );
    }
}
