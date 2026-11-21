//! Temporary zero-idle-overhead Win32 `WH_KEYBOARD_LL` exclusive hotkey recorder.
//!
//! Installed ONLY while the user actively clicks a `HotkeyRecorder` field in Settings Center,
//! and uninstalled immediately on capture, `Esc`, blur, window close, or safety timeout.
//! Because `WH_KEYBOARD_LL` fires BEFORE Win32 kernel `RegisterHotKey` dispatch, swallowing
//! the key prevents both Tomoyo and third-party apps (WeChat, PixPin, Pot, CopyQ, etc.) from
//! being accidentally triggered while recording a shortcut.

#![cfg(windows)]

use crate::bridge_os::notify_parent_mode;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Mutex};
use tomoyo_core::{format_hotkey, is_standalone_allowed_vk, vk_to_key_name};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

struct ActiveCapture {
    tx: mpsc::Sender<String>,
    host_hwnd: usize,
    notify_msg: u32,
    hook_tid: u32,
}

static ACTIVE: Mutex<Option<ActiveCapture>> = Mutex::new(None);
static SWALLOW_UP_VK: AtomicU32 = AtomicU32::new(0);
static CAPTURE_DONE: AtomicBool = AtomicBool::new(false);

pub struct HotkeyCaptureController {
    hook_tid: Option<u32>,
    parent_hwnd: Option<isize>,
}

impl HotkeyCaptureController {
    pub fn new(parent_hwnd: Option<isize>) -> Self {
        Self { hook_tid: None, parent_hwnd }
    }

    pub fn start(&mut self, host_hwnd: usize, tx: mpsc::Sender<String>, notify_msg: u32) {
        self.stop();
        if let Some(p) = self.parent_hwnd {
            notify_parent_mode(p, 3); // Suspend Tomoyo hotkeys while recording
        }
        CAPTURE_DONE.store(false, Ordering::SeqCst);
        SWALLOW_UP_VK.store(0, Ordering::SeqCst);

        let (tid_tx, tid_rx) = mpsc::channel::<u32>();
        let _ = std::thread::Builder::new()
            .name("tomoyo-hotkey-capture".into())
            .spawn(move || unsafe {
                let tid = GetCurrentThreadId();
                {
                    let mut guard = ACTIVE.lock().unwrap();
                    *guard = Some(ActiveCapture { tx, host_hwnd, notify_msg, hook_tid: tid });
                }
                let _ = tid_tx.send(tid);
                let Ok(hook) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), None, 0)
                else {
                    tracing::warn!("SetWindowsHookExW(WH_KEYBOARD_LL) failed for hotkey capture");
                    let mut guard = ACTIVE.lock().unwrap();
                    *guard = None;
                    return;
                };
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).into() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = UnhookWindowsHookEx(hook);
                let mut guard = ACTIVE.lock().unwrap();
                if guard.as_ref().is_some_and(|a| a.hook_tid == tid) {
                    *guard = None;
                }
            });

        if let Ok(tid) = tid_rx.recv_timeout(std::time::Duration::from_millis(250)) {
            self.hook_tid = Some(tid);
        }
    }

    pub fn stop(&mut self) {
        if let Some(tid) = self.hook_tid.take() {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            if let Some(p) = self.parent_hwnd {
                notify_parent_mode(p, 4); // Resume Tomoyo hotkeys
            }
        }
    }
}

impl Drop for HotkeyCaptureController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn is_modifier_vk(vk: u32) -> bool {
    matches!(
        vk as u16,
        x if x == VK_SHIFT.0
            || x == VK_LSHIFT.0
            || x == VK_RSHIFT.0
            || x == VK_CONTROL.0
            || x == VK_LCONTROL.0
            || x == VK_RCONTROL.0
            || x == VK_MENU.0
            || x == VK_LMENU.0
            || x == VK_RMENU.0
            || x == VK_LWIN.0
            || x == VK_RWIN.0
    )
}

fn current_modifiers() -> u32 {
    let down = |vk: u16| unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
    let mut m = 0u32;
    if down(VK_MENU.0) || down(VK_LMENU.0) || down(VK_RMENU.0) { m |= 0x0001; }
    if down(VK_CONTROL.0) || down(VK_LCONTROL.0) || down(VK_RCONTROL.0) { m |= 0x0002; }
    if down(VK_SHIFT.0) || down(VK_LSHIFT.0) || down(VK_RSHIFT.0) { m |= 0x0004; }
    if down(VK_LWIN.0) || down(VK_RWIN.0) { m |= 0x0008; }
    m
}

fn emit_event(json: String, finish_hook: bool) {
    let guard = ACTIVE.lock().unwrap();
    if let Some(act) = guard.as_ref() {
        if !json.is_empty() && act.tx.send(json).is_ok() {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(act.host_hwnd as *mut c_void)),
                    act.notify_msg,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
        if finish_hook {
            let tid = act.hook_tid;
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

unsafe extern "system" fn ll_keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }
    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    if (kb.flags.0 & LLKHF_INJECTED.0) != 0 {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }
    let msg = wparam.0 as u32;
    let vk = kb.vkCode;

    if matches!(msg, WM_KEYUP | WM_SYSKEYUP) {
        if SWALLOW_UP_VK.load(Ordering::SeqCst) == vk {
            SWALLOW_UP_VK.store(0, Ordering::SeqCst);
            if CAPTURE_DONE.load(Ordering::SeqCst) {
                emit_event(String::new(), true);
            }
            return LRESULT(1);
        }
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }

    if !matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN) || is_modifier_vk(vk) {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }

    if vk == VK_ESCAPE.0 as u32 {
        SWALLOW_UP_VK.store(vk, Ordering::SeqCst);
        CAPTURE_DONE.store(true, Ordering::SeqCst);
        emit_event(serde_json::json!({ "event": "hotkey_captured", "cancelled": true }).to_string(), true);
        return LRESULT(1);
    }

    let mods = current_modifiers();
    if let Some(combo) = format_hotkey(mods, vk) {
        SWALLOW_UP_VK.store(vk, Ordering::SeqCst);
        CAPTURE_DONE.store(true, Ordering::SeqCst);
        emit_event(
            serde_json::json!({ "event": "hotkey_captured", "combo": combo }).to_string(),
            false,
        );
        return LRESULT(1);
    }

    if vk_to_key_name(vk).is_some() && !is_standalone_allowed_vk(vk) {
        SWALLOW_UP_VK.store(vk, Ordering::SeqCst);
        emit_event(
            serde_json::json!({ "event": "hotkey_captured", "need_mod": true }).to_string(),
            false,
        );
        return LRESULT(1);
    }

    unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) }
}
