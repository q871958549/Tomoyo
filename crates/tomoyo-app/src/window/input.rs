//! Keyboard navigation, filter shortcuts, global hotkey registration, and EDIT subclassing.

use super::{AppShell, EM_GETSEL, HOTKEY_ID, WM_APP_EDIT_KEY, WM_APP_PASTE_INTERCEPT};
use crate::actions::{clipboard_has_rich_attachment, set_clipboard_text};
use std::ffi::c_void;
use tomoyo_core::parse_hotkey;
use tomoyo_protocol::SearchFilter;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, RegisterHotKey, HOT_KEY_MODIFIERS, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_BACK,
    VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, PostMessageW, SendMessageW, WM_KEYDOWN, WM_SYSKEYDOWN,
};

impl AppShell {
    pub(super) fn handle_nav_key(
        &mut self,
        vk: u16,
        ctrl_down: bool,
        alt_down: bool,
        shift_down: bool,
    ) {
        if alt_down && (b'1' as u16..=b'8' as u16).contains(&vk) {
            let idx = self.scroll_offset + (vk - b'1' as u16) as usize;
            if idx < self.results.len() {
                self.selected = idx;
                self.execute_selected(0);
            }
            return;
        }

        if ctrl_down && (b'1' as u16..=b'9' as u16).contains(&vk) {
            let idx = (vk - b'1' as u16) as usize;
            if let Some(f) = SearchFilter::from_index(idx) {
                self.set_active_filter(f);
            }
            return;
        }

        if ctrl_down && vk == VK_TAB.0 {
            let next = self.active_filter.cycle(!shift_down);
            self.set_active_filter(next);
            return;
        }

        if ctrl_down && vk == b'P' as u16 {
            self.preview_enabled = !self.preview_enabled;
            self.cached_preview_id = None;
            self.refresh_selected_preview();
            self.resize_to_fit_results();
            self.show_toast(if self.preview_enabled {
                self.lang.pick("已开启右侧实时预览 (Ctrl+P)", "Live Preview enabled (Ctrl+P)")
            } else {
                self.lang.pick("已关闭右侧实时预览 (Ctrl+P)", "Live Preview hidden (Ctrl+P)")
            });
            return;
        }

        if ctrl_down && vk == 0xBC {
            self.open_settings_center();
            return;
        }

        if ctrl_down && vk == b'C' as u16 {
            let (mut start, mut end) = (0u32, 0u32);
            unsafe {
                SendMessageW(
                    self.edit_hwnd,
                    EM_GETSEL,
                    Some(WPARAM(&mut start as *mut _ as usize)),
                    Some(LPARAM(&mut end as *mut _ as isize)),
                );
            }
            if start == end {
                if let Some(txt) = self.results.get(self.selected).and_then(|i| i.copy_text.as_deref()) {
                    if set_clipboard_text(txt) {
                        self.show_toast(self.lang.pick("已复制选中项内容", "Copied selected result"));
                    }
                }
            }
            return;
        }

        match vk {
            x if x == VK_BACK.0 => {
                if self.attached.is_some() && self.current_query.is_empty() {
                    self.attached = None;
                    self.show_toast(self.lang.pick("已清除粘贴胶囊", "Cleared attached capsule"));
                    self.dispatch_query();
                    self.invalidate();
                }
            }
            x if x == VK_ESCAPE.0 => {
                if self.action_menu_open {
                    self.action_menu_open = false;
                    self.selected_action = 0;
                    self.invalidate();
                } else if self.attached.is_some() {
                    self.attached = None;
                    self.dispatch_query();
                    self.invalidate();
                } else if self.active_filter != SearchFilter::All {
                    self.set_active_filter(SearchFilter::All);
                } else {
                    self.hide_launcher();
                }
            }
            x if x == VK_TAB.0 => {
                if self.results.get(self.selected).is_some_and(|i| !i.actions.is_empty()) {
                    self.action_menu_open = !self.action_menu_open;
                    self.selected_action = 0;
                    self.invalidate();
                }
            }
            x if x == VK_DOWN.0 => self.move_selection(1),
            x if x == VK_UP.0 => self.move_selection(-1),
            x if x == VK_RETURN.0 => {
                if self.action_menu_open {
                    self.execute_selected(self.selected_action);
                } else if ctrl_down {
                    self.execute_selected(1);
                } else {
                    self.execute_selected(0);
                }
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: i32) {
        if self.action_menu_open {
            if let Some(item) = self.results.get(self.selected) {
                let len = item.actions.len();
                if len > 0 {
                    self.selected_action = if delta > 0 {
                        (self.selected_action + 1) % len
                    } else if self.selected_action == 0 {
                        len - 1
                    } else {
                        self.selected_action - 1
                    };
                    self.invalidate();
                }
            }
            return;
        }
        let len = self.results.len();
        if len == 0 {
            return;
        }
        self.selected = if delta > 0 {
            (self.selected + 1) % len
        } else if self.selected == 0 {
            len - 1
        } else {
            self.selected - 1
        };
        self.ensure_selected_visible();
        self.refresh_selected_preview();
        self.resize_to_fit_results();
        self.invalidate();
    }
}

pub(super) fn register_configured_hotkey(hwnd: HWND, spec: &str) -> String {
    if let Some(parsed) = parse_hotkey(spec) {
        unsafe {
            if RegisterHotKey(
                Some(hwnd),
                HOTKEY_ID,
                HOT_KEY_MODIFIERS(parsed.modifiers),
                parsed.vk,
            )
            .is_ok()
            {
                return spec.to_string();
            }
        }
    }
    unsafe {
        let _ = RegisterHotKey(
            Some(hwnd),
            HOTKEY_ID,
            HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0),
            VK_SPACE.0 as u32,
        );
    }
    "Ctrl+Shift+Space".to_string()
}

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
                x if x == VK_UP.0 || x == VK_DOWN.0 || x == VK_RETURN.0 || x == VK_ESCAPE.0 || x == VK_TAB.0
            );
            if intercept || (ctrl && (b'1' as u16..=b'9' as u16).contains(&vk)) {
                unsafe {
                    let _ = PostMessageW(Some(parent), WM_APP_EDIT_KEY, WPARAM(vk as usize), LPARAM(mod_flags));
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
            if ctrl && (vk == b'P' as u16 || vk == 0xBC) {
                unsafe {
                    let _ = PostMessageW(Some(parent), WM_APP_EDIT_KEY, WPARAM(vk as usize), LPARAM(1));
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
                        let _ = PostMessageW(Some(parent), WM_APP_EDIT_KEY, WPARAM(vk as usize), LPARAM(1));
                    }
                    return LRESULT(0);
                }
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
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
