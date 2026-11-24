//! Keyboard navigation, filter shortcuts, global hotkey registration, and EDIT subclassing.

use super::hotkey_override::{sync_conflict_override_hook, uninstall_conflict_override_hook};
use super::{AppShell, EM_GETSEL, HOTKEY_CLIPBOARD_ID, HOTKEY_ID};
use crate::actions::set_clipboard_text;
use tomoyo_core::{
    parse_hotkey, save_hotkey_health, HotkeyHealthReport, HotkeySlotStatus, Lang, TomoyoConfig,
    PLUGIN_CAPTURE, PLUGIN_CLIPBOARD,
};
use tomoyo_protocol::SearchFilter;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
    VK_BACK, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::SendMessageW;

pub(super) const HOTKEY_CAPTURE_ID: i32 = 4;
pub(super) const HOTKEY_PIN_ID: i32 = 5;
pub(super) const HOTKEY_OCR_ID: i32 = 6;
pub(super) const HOTKEY_RECORD_ID: i32 = 7;

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
            let tabs = self.visible_filter_tabs();
            if let Some(t) = tabs.get(idx) {
                self.set_active_filter(t.filter);
            }
            return;
        }

        if ctrl_down && vk == VK_TAB.0 {
            let tabs = self.visible_filter_tabs();
            let next = self.active_filter.cycle_visible(&tabs, !shift_down);
            self.set_active_filter(next);
            return;
        }

        if ctrl_down && shift_down && vk == b'P' as u16
            && self.trigger_selected_clipboard_shortcut("pin") {
                return;
            }

        if ctrl_down && vk == b'E' as u16 {
            if !self.trigger_selected_clipboard_shortcut("edit_workbench") {
                self.open_clipboard_workbench(None);
            }
            return;
        }

        if ctrl_down && vk == b'D' as u16
            && self.trigger_selected_clipboard_shortcut("delete") {
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
            if start == end
                && let Some(txt) = self.results.get(self.selected).and_then(|i| i.copy_text.as_deref())
                    && set_clipboard_text(txt) {
                        self.show_toast(self.lang.pick("已复制选中项内容", "Copied selected result"));
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
                if self.inter.inline_dock_pid.is_some() {
                    self.close_inline_dock();
                    self.resize_to_fit_results();
                    unsafe {
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.edit_hwnd));
                    }
                    self.show_toast(self.lang.pick("已关闭内嵌插件，返回搜索框", "Closed inline plugin"));
                    self.invalidate();
                } else if self.action_menu_open {
                    self.action_menu_open = false;
                    self.selected_action = 0;
                    self.invalidate();
                } else if !self.current_query.is_empty() {
                    self.cancel_pending_launch_hide();
                    self.set_query_text("");
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
                } else if ctrl_down && shift_down {
                    if let Some(admin_idx) = self
                        .results
                        .get(self.selected)
                        .and_then(|it| it.actions.iter().position(|a| a.id == "run_admin"))
                    {
                        self.execute_selected(admin_idx);
                    } else {
                        self.execute_selected(0);
                    }
                } else if ctrl_down {
                    self.execute_selected(1);
                } else if shift_down {
                    self.execute_selected(2);
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

pub(super) fn register_configured_hotkey(hwnd: HWND, spec: &str) -> (String, Option<String>) {
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
                return (spec.to_string(), None);
            }
        }
    }
    let fallback_ok = unsafe {
        RegisterHotKey(
            Some(hwnd),
            HOTKEY_ID,
            HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0),
            VK_SPACE.0 as u32,
        )
        .is_ok()
    };
    if fallback_ok {
        ("Ctrl+Shift+Space".to_string(), Some(spec.to_string()))
    } else {
        ("Unbound".to_string(), Some(spec.to_string()))
    }
}

pub(super) fn unregister_all_hotkeys(hwnd: HWND) {
    uninstall_conflict_override_hook();
    super::plugin_hotkeys::unregister_plugin_hotkeys(hwnd);
    unsafe {
        for id in [HOTKEY_ID, HOTKEY_CLIPBOARD_ID, HOTKEY_CAPTURE_ID, HOTKEY_PIN_ID, HOTKEY_OCR_ID, HOTKEY_RECORD_ID] {
            let _ = UnregisterHotKey(Some(hwnd), id);
        }
    }
}

pub(super) fn register_all_hotkeys(
    hwnd: HWND,
    cfg: &TomoyoConfig,
    plugin_host: &tomoyo_plugin_host::PluginHost,
) -> (String, Option<String>, Vec<tomoyo_plugin_host::ActivePluginHotkey>, Vec<String>) {
    super::hotkey_avoid::sync_hotkey_avoidance(cfg.suspend_hotkeys_in_fullscreen, &cfg.hotkey_excluded_apps);
    let lang = cfg.lang();
    let allow_override = cfg.override_conflict_hotkeys;
    let mut override_entries: Vec<(i32, u32, u32)> = Vec::new();
    let mut issues = Vec::new();

    let (mut active, main_conflict) = register_configured_hotkey(hwnd, &cfg.hotkey);
    let mut main_status = if let Some(orig) = main_conflict {
        if allow_override {
            if let Some(p) = parse_hotkey(&orig) {
                unsafe { let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID); }
                override_entries.push((HOTKEY_ID, p.modifiers, p.vk));
                active = orig.clone();
                HotkeySlotStatus {
                    spec: orig.clone(), active: orig, ok: true, disabled: false, overridden: true,
                    reason: lang.pick("已高优先级接管生效", "Overridden via high-priority hook").into(),
                }
            } else {
                let msg = match lang {
                    Lang::Zh => format!("主热键 {orig} 被占用，已改用 {active}"),
                    Lang::En => format!("Main hotkey {orig} busy; using {active}"),
                };
                issues.push(msg.clone());
                HotkeySlotStatus { spec: orig, active: active.clone(), ok: false, disabled: false, overridden: false, reason: msg }
            }
        } else {
            let msg = match lang {
                Lang::Zh => format!("主热键 {orig} 被占用，已改用 {active}"),
                Lang::En => format!("Main hotkey {orig} busy; using {active}"),
            };
            issues.push(msg.clone());
            HotkeySlotStatus { spec: orig, active: active.clone(), ok: false, disabled: false, overridden: false, reason: msg }
        }
    } else {
        HotkeySlotStatus { spec: cfg.hotkey.clone(), active: active.clone(), ok: true, disabled: false, overridden: false, reason: String::new() }
    };

    let mut reg_opt = |id: i32, spec: &str, label: &str| -> HotkeySlotStatus {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return HotkeySlotStatus { spec: String::new(), active: String::new(), ok: true, disabled: true, overridden: false, reason: String::new() };
        }
        if register_optional_hotkey(hwnd, id, trimmed) {
            return HotkeySlotStatus { spec: trimmed.to_string(), active: trimmed.to_string(), ok: true, disabled: false, overridden: false, reason: String::new() };
        }
        if allow_override
            && let Some(p) = parse_hotkey(trimmed) {
                override_entries.push((id, p.modifiers, p.vk));
                tracing::info!(hotkey = %trimmed, id = id, "Hotkey occupied externally; queued for high-priority override");
                return HotkeySlotStatus {
                    spec: trimmed.to_string(), active: trimmed.to_string(), ok: true, disabled: false, overridden: true,
                    reason: lang.pick("已高优先级接管生效", "Overridden via high-priority hook").into(),
                };
            }
        tracing::warn!(hotkey = %trimmed, id = id, "Optional hotkey registration failed (already in use; override disabled)");
        let reason = match lang {
            Lang::Zh => format!("已被系统或其他软件占用（{trimmed}）"),
            Lang::En => format!("Occupied by OS or another app ({trimmed})"),
        };
        issues.push(match lang {
            Lang::Zh => format!("{label}热键 {trimmed} 被其他软件占用"),
            Lang::En => format!("{label} hotkey {trimmed} is busy"),
        });
        HotkeySlotStatus { spec: trimmed.to_string(), active: String::new(), ok: false, disabled: false, overridden: false, reason }
    };

    let cap_on = cfg.is_plugin_enabled(PLUGIN_CAPTURE);
    let clip_spec = if cfg.is_plugin_enabled(PLUGIN_CLIPBOARD) { cfg.clipboard.hotkey.as_str() } else { "" };
    let cap_spec = if cap_on { cfg.capture.hotkey.as_str() } else { "" };
    let pin_spec = if cap_on { cfg.capture.pin_hotkey.as_str() } else { "" };
    let ocr_spec = if cap_on { cfg.capture.ocr_hotkey.as_str() } else { "" };
    let rec_spec = if cap_on { cfg.capture.record_hotkey.as_str() } else { "" };
    let mut clip_status = reg_opt(HOTKEY_CLIPBOARD_ID, clip_spec, lang.pick("剪贴板", "Clipboard"));
    let tr_status = HotkeySlotStatus { disabled: true, ..Default::default() };
    let mut cap_status = reg_opt(HOTKEY_CAPTURE_ID, cap_spec, lang.pick("截图", "Capture"));
    let mut pin_status = reg_opt(HOTKEY_PIN_ID, pin_spec, lang.pick("贴图", "Pin"));
    let mut ocr_status = reg_opt(HOTKEY_OCR_ID, ocr_spec, lang.pick("截图识字", "Screenshot OCR"));
    let mut rec_status = reg_opt(HOTKEY_RECORD_ID, rec_spec, lang.pick("区域录屏", "Screen Record"));
    let plug_hks = super::plugin_hotkeys::register_plugin_hotkeys(hwnd, cfg, plugin_host, &mut override_entries);
    let flow_hks = super::plugin_hotkeys::register_flow_hotkeys(hwnd, cfg, &mut override_entries);

    if !sync_conflict_override_hook(hwnd, &override_entries) {
        tracing::warn!("Failed to install low-level conflict override hook; conflicted hotkeys inactive");
        for slot in [&mut main_status, &mut clip_status, &mut cap_status, &mut pin_status, &mut ocr_status, &mut rec_status] {
            if slot.overridden {
                slot.ok = false; slot.overridden = false;
                slot.reason = lang.pick("底层接管钩子挂载失败", "Override hook installation failed").into();
            }
        }
    }

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    save_hotkey_health(&HotkeyHealthReport {
        main: main_status, clipboard: clip_status, translate: tr_status, capture: cap_status, pin: pin_status, ocr: ocr_status, record: rec_status,
        updated_at_unix: now,
    });

    let summary = if issues.is_empty() { None } else {
        Some(match lang {
            Lang::Zh => format!("⚠ {}（按 Ctrl+, 调整）", issues.join("；")),
            Lang::En => format!("⚠ {} (Ctrl+, to change)", issues.join("; ")),
        })
    };
    (active, summary, plug_hks, flow_hks)
}

pub(super) fn register_optional_hotkey(hwnd: HWND, id: i32, spec: &str) -> bool {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(parsed) = parse_hotkey(trimmed) else {
        return false;
    };
    unsafe {
        RegisterHotKey(Some(hwnd), id, HOT_KEY_MODIFIERS(parsed.modifiers), parsed.vk).is_ok()
    }
}
