//! Main Win32 launcher shell: orchestrates lifecycle, themes, i18n, hibernation & async query loop.

mod bootstrap;
mod clipboard_ctrl;
mod commands;
mod context_card;
mod events;
mod filter_dialog;
mod filter_dialog_ui;
mod flow_exec;
pub(crate) mod hotkey_avoid;
pub(crate) mod hotkey_override;
mod inline_host;
mod input;
mod input_edit;
mod launch_ctrl;
mod launch_probe;
mod layout;
mod menus;
mod plugin_hotkeys;
mod selftest;
mod sidebar_menu;
mod super_panel_ctrl;
mod translate_ctrl;
mod updater_ctrl;
mod wndproc;
mod wndproc_mouse;

use crate::preview_loader::AsyncPreviewLoader;
use crate::renderer::{FilterCounts, FluentRenderer, ScrollbarHit, ThemePalette};
use crate::tray::TrayIcon;
use bootstrap::detect_everything_label;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use tomoyo_core::{Lang, QueryRouter, ResolvedPreview, TomoyoConfig, TypographyConfig, UpdateManager};
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{AttachedContext, ClipboardSnapshot, ResultItem, SearchFilter};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{DeleteObject, InvalidateRect, HBRUSH, HFONT};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetForegroundWindow, GetMessageW, IsWindowVisible, KillTimer, SendMessageW,
    SetTimer, SetWindowTextW, ShowWindow, TranslateMessage, MSG, SW_HIDE, SW_SHOW, WM_APP,
};

pub(super) const HOTKEY_ID: i32 = 1;
pub(super) const EDIT_CTRL_ID: usize = 101;
pub(super) const EDIT_SUBCLASS_ID: usize = 1;
pub(super) const WM_APP_RESULTS_READY: u32 = WM_APP + 1;
pub(super) const WM_APP_EDIT_KEY: u32 = WM_APP + 2;
pub(super) const WM_APP_PASTE_INTERCEPT: u32 = WM_APP + 3;
pub const WM_APP_UPDATE_READY: u32 = WM_APP + 4;
pub(super) const WM_APP_CONFIG_SAVED: u32 = WM_APP + 20;
pub const WM_APP_WAKE_EXISTING: u32 = WM_APP + 21;
pub(super) const WM_APP_UPDATE_INSTALLED: u32 = WM_APP + 22;
pub(super) const WM_APP_FLOW_DONE: u32 = WM_APP + 24;
pub(super) const WM_APP_LAUNCH_DONE: u32 = WM_APP + 25;
pub(super) const WM_APP_SHOW_CONTEXT_MENU: u32 = WM_APP + 26;
pub(super) const WM_APP_PLUGIN_EVENT_DONE: u32 = WM_APP + 27;
pub(super) const TIMER_HIBERNATE: usize = 1001;
pub(super) const TIMER_CLEAR_TOAST: usize = 1002;
pub(super) const TIMER_FOCUS_GUARD: usize = 1003;
pub(super) const TIMER_LAUNCH_HIDE: usize = 1004;
pub(super) const TIMER_SETTINGS_HIDE: usize = 1005;
pub(super) const TIMER_FADE_HIDE: usize = 1006;
pub(super) const TIMER_LAUNCH_TICK: usize = 1007;
pub(super) const EM_SETCUEBANNER: u32 = 0x1501;
pub(super) const EM_GETSEL: u32 = 0x00B0;
pub(super) const EM_SETSEL: u32 = 0x00B1;

static MODAL_OVERLAY_DEPTH: AtomicUsize = AtomicUsize::new(0);
static ASYNC_MODAL_OVERLAY: AtomicBool = AtomicBool::new(false);
static SUPPRESS_EDIT_CHANGE_DEPTH: AtomicUsize = AtomicUsize::new(0);

/// RAII guard for synchronous modal Win32 loops (context menus, OLE drag-drop).
/// Uses process-global atomics rather than a `&mut AppShell` bool so LLVM `noalias`
/// dead-store elimination cannot elide the store before a re-entrant Win32 callback.
pub(super) struct ModalOverlayGuard;

impl Drop for ModalOverlayGuard {
    fn drop(&mut self) {
        MODAL_OVERLAY_DEPTH.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(super) fn enter_modal_overlay() -> ModalOverlayGuard {
    MODAL_OVERLAY_DEPTH.fetch_add(1, Ordering::SeqCst);
    ModalOverlayGuard
}

pub(super) fn set_async_modal_overlay(active: bool) {
    ASYNC_MODAL_OVERLAY.store(active, Ordering::SeqCst);
}

pub(super) fn is_modal_overlay_active() -> bool {
    MODAL_OVERLAY_DEPTH.load(Ordering::SeqCst) > 0 || ASYNC_MODAL_OVERLAY.load(Ordering::SeqCst)
}

/// RAII guard suppressing synchronous `WM_COMMAND(EN_CHANGE)` callbacks during `SetWindowTextW`.
pub(super) struct EditChangeSuppressGuard;

impl Drop for EditChangeSuppressGuard {
    fn drop(&mut self) {
        SUPPRESS_EDIT_CHANGE_DEPTH.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(super) fn suppress_edit_change() -> EditChangeSuppressGuard {
    SUPPRESS_EDIT_CHANGE_DEPTH.fetch_add(1, Ordering::SeqCst);
    EditChangeSuppressGuard
}

pub(super) fn is_edit_change_suppressed() -> bool {
    SUPPRESS_EDIT_CHANGE_DEPTH.load(Ordering::SeqCst) > 0
}

unsafe extern "C" {
    /// Linked via `mimalloc`: forces immediate purge of freed segments back to Windows OS!
    fn mi_collect(force: bool);
}

pub(super) struct QueryJob {
    pub generation: u64,
    pub query: String,
    pub clipboard: Option<ClipboardSnapshot>,
    pub attached: Option<AttachedContext>,
    pub filter: SearchFilter,
}

pub(super) struct QueryOutcome {
    pub generation: u64,
    pub items: Vec<ResultItem>,
    pub is_partial: bool,
}

pub(super) struct WindowGeometry {
    pub list_width_dip: i32,
    pub max_visible_rows: usize,
    pub preview_width_dip: i32,
    pub custom_pos: Option<(i32, i32)>,
}

#[derive(Default)]
pub(super) struct InteractionState {
    pub hovered: Option<usize>,
    pub hovered_filter: Option<SearchFilter>,
    pub hovered_overflow: bool,
    pub hovered_add_tag: bool,
    pub sidebar_scroll: f32,
    pub hovered_logo: bool,
    pub pressed_logo: bool,
    pub settings_pending: bool,
    pub fade_alpha: Option<u8>,
    pub filter_counts: Option<FilterCounts>,
    pub mouse_tracked: bool,
    pub in_size_move: bool,
    pub dragging_divider: bool,
    pub dragging_scrollbar: Option<f32>,
    pub scrollbar_hover: Option<ScrollbarHit>,
    pub drag_origin: Option<(i32, i32, String)>,
    pub last_click: Option<(usize, Instant)>,
    pub prev_foreground_hwnd: isize,
    pub suppress_next_clip_img: bool,
    pub inline_dock_pid: Option<u32>,
    pub last_shown_at: Option<Instant>,
}

pub struct AppShell {
    pub(super) hwnd: HWND,
    pub(super) edit_hwnd: HWND,
    pub(super) edit_font: HFONT,
    pub(super) edit_bg_brush: HBRUSH,
    pub(super) tray: Option<TrayIcon>,
    pub(super) renderer: FluentRenderer,
    pub(super) preview_loader: AsyncPreviewLoader,
    pub(super) font_regular: &'static str,
    pub(super) font_medium: &'static str,
    pub(super) config: Arc<RwLock<TomoyoConfig>>,
    pub(super) lang: Lang,
    pub(super) theme: ThemePalette,
    pub(super) theme_name: String,
    pub(super) router: Arc<QueryRouter>,
    pub(super) plugin_host: Arc<PluginHost>,
    pub(super) last_clip_snapshot: Option<ClipboardSnapshot>,
    pub(super) clip_listener_active: bool,
    pub(super) updater: Arc<UpdateManager>,
    pub(super) query_tx: Sender<QueryJob>,
    pub(super) result_rx: Receiver<QueryOutcome>,
    pub(super) current_query: String,
    pub(super) attached: Option<AttachedContext>,
    pub(super) active_filter: SearchFilter,
    pub(super) show_filter_bar: bool,
    pub(super) filter_bar_wrap: bool,
    pub(super) typography: TypographyConfig,
    pub(super) last_result_gen: u64,
    pub(super) results: Vec<ResultItem>,
    pub(super) selected: usize,
    pub(super) scroll_offset: usize,
    pub(super) action_menu_open: bool,
    pub(super) selected_action: usize,
    pub(super) preview_enabled: bool,
    pub(super) cached_preview_id: Option<String>,
    pub(super) cached_preview: Option<ResolvedPreview>,
    pub(super) geom: WindowGeometry,
    pub(super) inter: InteractionState,
    pub(super) toast: Option<String>,
    pub(super) everything_status: Option<&'static str>,
    pub(super) hotkey_label: String,
    pub(super) hotkey_conflict: Option<String>,
    pub(super) hotkeys_paused: bool,
    pub(super) plugin_hotkeys: Vec<tomoyo_plugin_host::ActivePluginHotkey>,
    pub(super) tray_plugin_actions: Vec<tomoyo_plugin_host::ActivePluginTrayAction>,
    pub(super) flow_hotkeys: Vec<String>,
    pub(super) pending_flow_result: Arc<Mutex<Option<Result<tomoyo_core::FlowRunPlan, String>>>>,
    pub(super) pending_plugin_event: Arc<Mutex<Option<(String, tomoyo_protocol::ExecuteResponse)>>>,
    pub(super) pending_launch_events: Arc<Mutex<std::collections::VecDeque<launch_ctrl::LaunchEvent>>>,
    pub(super) active_launches: Vec<launch_ctrl::ActiveLaunch>,
    pub(super) next_launch_id: u64,
    pub(super) pending_launch_failure: Option<(String, Instant)>,
    pub(super) pending_update_install: Arc<Mutex<updater_ctrl::UpdateInstallSlot>>,
    pub(super) dpi_scale: f32,
    pub(super) taskbar_created_msg: u32,
    pub(super) suppress_clipboard_capture: Arc<AtomicBool>,
    pub(super) super_panel: super_panel_ctrl::SuperPanelBridge,
    pub(super) shutting_down: bool,
}

impl AppShell {
    pub fn hotkey_label(&self) -> &str { &self.hotkey_label }
    pub fn hwnd_raw(&self) -> usize { self.hwnd.0 as usize }

    pub fn show_launcher(&mut self) {
        self.show_launcher_with_query(None);
    }

    fn reset_transient_ui_state(&mut self) {
        self.action_menu_open = false;
        self.selected_action = 0;
        self.inter.hovered = None;
        self.inter.hovered_filter = None;
        self.inter.hovered_overflow = false;
        self.inter.hovered_add_tag = false;
        self.inter.hovered_logo = false;
        self.inter.pressed_logo = false;
        self.inter.dragging_scrollbar = None;
        self.inter.scrollbar_hover = None;
        self.inter.settings_pending = false;
        set_async_modal_overlay(false);
        self.inter.fade_alpha = None;
        selftest::restore_launcher_opacity(self.hwnd);
        self.inter.filter_counts = None;
        unsafe { let _ = KillTimer(Some(self.hwnd), TIMER_SETTINGS_HIDE); let _ = KillTimer(Some(self.hwnd), TIMER_FADE_HIDE); }
    }

    pub fn show_launcher_with_query(&mut self, initial_query: Option<&str>) {
        unsafe {
            let fg = GetForegroundWindow();
            if !fg.is_invalid() && fg != self.hwnd {
                self.inter.prev_foreground_hwnd = fg.0 as isize;
            }
            let _ = KillTimer(Some(self.hwnd), TIMER_HIBERNATE);
        }
        self.everything_status = detect_everything_label();
        self.reset_transient_ui_state();
        if let Some(q) = initial_query {
            self.active_filter = SearchFilter::All;
            self.attached = None;
            let wide: Vec<u16> = q.encode_utf16().chain(std::iter::once(0)).collect();
            {
                let _suppress = suppress_edit_change();
                unsafe { let _ = SetWindowTextW(self.edit_hwnd, PCWSTR(wide.as_ptr())); }
            }
            self.current_query = q.to_string();
        }
        self.position_on_active_monitor();
        self.dispatch_query();
        set_async_modal_overlay(false);
        self.inter.last_shown_at = Some(Instant::now());
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            hotkey_override::force_foreground_window(self.hwnd);
            let _ = SetFocus(Some(self.edit_hwnd));
            if let Some(q) = initial_query {
                let len = q.encode_utf16().count() as isize;
                SendMessageW(self.edit_hwnd, EM_SETSEL, Some(WPARAM(len as usize)), Some(LPARAM(len)));
            } else {
                SendMessageW(self.edit_hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            }
            let _ = SetTimer(Some(self.hwnd), TIMER_FOCUS_GUARD, 160, None);
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        self.restore_launch_ui_on_show();
        if let Some(msg) = self.hotkey_conflict.take() {
            self.show_toast(&msg);
        }
    }

    pub fn hide_launcher(&mut self) {
        self.close_inline_dock();
        self.reset_transient_ui_state();
        self.toast = None;
        self.inter.drag_origin = None;
        self.inter.last_click = None;
        self.inter.dragging_divider = false;
        self.preview_loader.cancel_all();
        let delay_ms = self
            .config
            .read()
            .map(|c| c.hibernate_delay_secs.clamp(5, 300) * 1000)
            .unwrap_or(15_000);
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_FOCUS_GUARD);
            let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_HIDE);
            let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_TICK);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            {
                let _suppress = suppress_edit_change();
                let _ = SetWindowTextW(self.edit_hwnd, w!(""));
            }
            let _ = KillTimer(Some(self.hwnd), TIMER_HIBERNATE);
            let _ = SetTimer(Some(self.hwnd), TIMER_HIBERNATE, delay_ms, None);
        }
        self.inter.last_shown_at = None;
        self.current_query.clear();
        self.attached = None;
        self.active_filter = SearchFilter::All;
        self.last_result_gen = 0;
        self.results.clear();
        self.cached_preview_id = None;
        self.cached_preview = None;
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn toggle_launcher(&mut self) {
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.hide_launcher();
        } else {
            self.show_launcher();
        }
    }

    pub fn set_query_text(&mut self, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        {
            let _suppress = suppress_edit_change();
            unsafe {
                let _ = SetWindowTextW(self.edit_hwnd, PCWSTR(wide.as_ptr()));
                let len = text.encode_utf16().count() as isize;
                SendMessageW(self.edit_hwnd, EM_SETSEL, Some(WPARAM(len as usize)), Some(LPARAM(len)));
            }
        }
        self.current_query = text.to_string();
        self.dispatch_query();
    }

    pub fn set_active_filter(&mut self, filter: SearchFilter) {
        if self.active_filter == filter {
            return;
        }
        self.active_filter = filter;
        self.selected = 0;
        self.scroll_offset = 0;
        self.dispatch_query();
        self.invalidate();
    }

    pub fn run_message_loop(&mut self) {
        let mut msg = MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    pub fn show_toast(&mut self, msg: &str) {
        self.toast = Some(msg.to_string());
        unsafe { let _ = KillTimer(Some(self.hwnd), TIMER_CLEAR_TOAST); let _ = SetTimer(Some(self.hwnd), TIMER_CLEAR_TOAST, 3000, None); }
        self.invalidate();
    }
    pub(super) fn invalidate(&self) { unsafe { let _ = InvalidateRect(Some(self.hwnd), None, false); } }
}

impl Drop for AppShell {
    fn drop(&mut self) {
        self.shutdown_super_panel();
        self.plugin_host.shutdown_all();
        self.preview_loader.cancel_all();
        self.renderer.hibernate_release();
        self.tray = None;
        unsafe {
            if !self.edit_font.is_invalid() { let _ = DeleteObject(self.edit_font.into()); }
            if !self.edit_bg_brush.is_invalid() { let _ = DeleteObject(self.edit_bg_brush.into()); }
        }
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    fn reentrant_atomic_guards_nest_and_reset_cleanly() {
        set_async_modal_overlay(false);
        assert!(!is_modal_overlay_active());
        assert!(!is_edit_change_suppressed());

        {
            let _outer = enter_modal_overlay();
            assert!(is_modal_overlay_active());
            {
                let _inner = enter_modal_overlay();
                assert!(is_modal_overlay_active());
            }
            assert!(is_modal_overlay_active());
        }
        assert!(!is_modal_overlay_active());

        set_async_modal_overlay(true);
        assert!(is_modal_overlay_active());
        {
            let _scoped = enter_modal_overlay();
            assert!(is_modal_overlay_active());
        }
        assert!(is_modal_overlay_active());
        set_async_modal_overlay(false);
        assert!(!is_modal_overlay_active());

        {
            let _e1 = suppress_edit_change();
            assert!(is_edit_change_suppressed());
            {
                let _e2 = suppress_edit_change();
                assert!(is_edit_change_suppressed());
            }
            assert!(is_edit_change_suppressed());
        }
        assert!(!is_edit_change_suppressed());
    }
}
