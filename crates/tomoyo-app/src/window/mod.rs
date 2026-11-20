//! Main Win32 launcher shell: orchestrates lifecycle, themes, i18n, hibernation & async query loop.

mod bootstrap;
mod commands;
mod input;
mod layout;
mod menus;
mod wndproc;

use crate::actions::spawn_settings_window;
use crate::preview_loader::AsyncPreviewLoader;
use crate::renderer::{FluentRenderer, RenderState, ThemePalette};
use crate::tray::TrayIcon;
use bootstrap::detect_everything_label;
use layout::{apply_cue_banner, apply_dwm_attributes, read_edit_text};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, RwLock};
use tomoyo_core::{Lang, QueryRouter, ResolvedPreview, TomoyoConfig, UpdateManager};
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{AttachedContext, ClipboardSnapshot, ResultItem, SearchFilter};
use tomoyo_providers::ClipboardProvider;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, InvalidateRect, HBRUSH, HDC, HFONT};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetForegroundWindow, GetMessageW, IsWindowVisible, KillTimer, SendMessageW,
    SetForegroundWindow, SetTimer, SetWindowTextW, ShowWindow, TranslateMessage, MSG, SW_HIDE,
    SW_SHOW, WM_APP,
};

pub(super) const HOTKEY_ID: i32 = 1;
pub(super) const EDIT_CTRL_ID: usize = 101;
pub(super) const EDIT_SUBCLASS_ID: usize = 1;
pub(super) const WM_APP_RESULTS_READY: u32 = WM_APP + 1;
pub(super) const WM_APP_EDIT_KEY: u32 = WM_APP + 2;
pub(super) const WM_APP_PASTE_INTERCEPT: u32 = WM_APP + 3;
pub(super) const WM_APP_UPDATE_READY: u32 = WM_APP + 4;
pub(super) const WM_APP_CONFIG_SAVED: u32 = WM_APP + 20;
pub const WM_APP_WAKE_EXISTING: u32 = WM_APP + 21;
pub(super) const TIMER_HIBERNATE: usize = 1001;
pub(super) const TIMER_CLEAR_TOAST: usize = 1002;
pub(super) const EM_SETCUEBANNER: u32 = 0x1501;
pub(super) const EM_GETSEL: u32 = 0x00B0;
pub(super) const EM_SETSEL: u32 = 0x00B1;

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
    pub mouse_tracked: bool,
    pub in_size_move: bool,
    pub dragging_divider: bool,
    pub drag_origin: Option<(i32, i32, String)>,
    pub prev_foreground_hwnd: isize,
    pub suppress_next_deactivate: bool,
    pub suppress_next_clip_img: bool,
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
    pub(super) clipboard: Arc<ClipboardProvider>,
    pub(super) updater: Arc<UpdateManager>,
    pub(super) query_tx: Sender<QueryJob>,
    pub(super) result_rx: Receiver<QueryOutcome>,
    pub(super) current_query: String,
    pub(super) attached: Option<AttachedContext>,
    pub(super) active_filter: SearchFilter,
    pub(super) show_filter_bar: bool,
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
    pub(super) dpi_scale: f32,
    pub(super) taskbar_created_msg: u32,
    pub(super) shutting_down: bool,
}

impl AppShell {
    pub fn hotkey_label(&self) -> &str {
        &self.hotkey_label
    }

    pub fn apply_theme(&mut self, theme_name: &str, persist: bool) {
        let new_theme = ThemePalette::from_name(theme_name);
        self.theme = new_theme;
        self.theme_name = theme_name.to_string();
        apply_dwm_attributes(self.hwnd, new_theme.is_dark);
        unsafe {
            if !self.edit_bg_brush.is_invalid() {
                let _ = DeleteObject(self.edit_bg_brush.into());
            }
            self.edit_bg_brush = CreateSolidBrush(new_theme.bg.to_colorref());
            let _ = InvalidateRect(Some(self.edit_hwnd), None, true);
        }
        if persist {
            if let Ok(mut cfg) = self.config.write() {
                cfg.theme = theme_name.to_string();
                let _ = cfg.save();
            }
        }
        self.invalidate();
    }

    pub fn apply_language(&mut self, lang_code: &str, persist: bool) {
        let new_lang = Lang::from_code(lang_code);
        self.lang = new_lang;
        unsafe { apply_cue_banner(self.edit_hwnd, new_lang); }
        if persist {
            if let Ok(mut cfg) = self.config.write() {
                cfg.language = new_lang.code().to_string();
                let _ = cfg.save();
            }
        }
        self.dispatch_query();
        self.invalidate();
    }

    pub fn open_settings_center(&mut self) {
        match spawn_settings_window(self.hwnd.0 as isize, &self.theme_name) {
            Ok(()) => self.hide_launcher(),
            Err(e) => self.show_toast(&format!("设置中心启动失败: {e}")),
        }
    }

    pub fn show_launcher(&mut self) {
        unsafe {
            let fg = GetForegroundWindow();
            if !fg.is_invalid() && fg != self.hwnd {
                self.inter.prev_foreground_hwnd = fg.0 as isize;
            }
            let _ = KillTimer(Some(self.hwnd), TIMER_HIBERNATE);
        }
        self.everything_status = detect_everything_label();
        self.action_menu_open = false;
        self.selected_action = 0;
        self.inter.hovered = None;
        self.inter.hovered_filter = None;
        self.position_on_active_monitor();
        self.dispatch_query();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.edit_hwnd));
            SendMessageW(self.edit_hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn hide_launcher(&mut self) {
        self.action_menu_open = false;
        self.selected_action = 0;
        self.inter.hovered = None;
        self.inter.hovered_filter = None;
        self.toast = None;
        self.inter.drag_origin = None;
        self.inter.dragging_divider = false;
        self.preview_loader.cancel_all();
        let delay_ms = self
            .config
            .read()
            .map(|c| c.hibernate_delay_secs.clamp(5, 300) * 1000)
            .unwrap_or(15_000);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = SetWindowTextW(self.edit_hwnd, w!(""));
            let _ = KillTimer(Some(self.hwnd), TIMER_HIBERNATE);
            let _ = SetTimer(Some(self.hwnd), TIMER_HIBERNATE, delay_ms, None);
        }
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

    pub fn run_hibernate(&mut self) -> u64 {
        unsafe { let _ = KillTimer(Some(self.hwnd), TIMER_HIBERNATE); }
        self.preview_loader.cancel_all();
        self.renderer.hibernate_release();
        self.results.clear();
        self.results.shrink_to_fit();
        self.cached_preview_id = None;
        self.cached_preview = None;
        let cycles = self.router.hibernate_trim();
        unsafe { mi_collect(true); }
        cycles
    }

    pub(super) fn shutdown_clean(&mut self) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        self.plugin_host.shutdown_all();
        self.clipboard.shutdown_checkpoint();
        self.preview_loader.cancel_all();
        self.tray = None;
    }

    pub fn set_query_text(&mut self, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = SetWindowTextW(self.edit_hwnd, PCWSTR(wide.as_ptr()));
            let len = text.encode_utf16().count() as isize;
            SendMessageW(self.edit_hwnd, EM_SETSEL, Some(WPARAM(len as usize)), Some(LPARAM(len)));
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

    pub(super) fn dispatch_query(&mut self) {
        let generation = self.router.next_generation();
        let _ = self.query_tx.send(QueryJob {
            generation,
            query: self.current_query.clone(),
            clipboard: self.clipboard.latest_snapshot(),
            attached: self.attached.clone(),
            filter: self.active_filter,
        });
    }

    pub(super) fn on_results_ready(&mut self) {
        let mut latest = None;
        while let Ok(out) = self.result_rx.try_recv() {
            latest = Some(out);
        }
        let Some(out) = latest else { return };
        if self.router.is_stale(out.generation) {
            return;
        }
        let prev_sel_id = if self.last_result_gen == out.generation && !out.is_partial {
            self.results.get(self.selected).map(|i| i.id.clone())
        } else {
            None
        };
        self.last_result_gen = out.generation;
        self.results = out.items;
        self.selected = prev_sel_id
            .and_then(|id| self.results.iter().position(|r| r.id == id))
            .unwrap_or(0);
        self.inter.hovered = None;
        self.scroll_offset = 0;
        self.action_menu_open = false;
        self.selected_action = 0;
        self.cached_preview_id = None;
        self.refresh_selected_preview();
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.resize_to_fit_results();
            self.invalidate();
        }
    }

    pub(super) fn on_icons_ready(&mut self) {
        if self.renderer.drain_ready_icons() && unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.invalidate();
        }
    }

    pub(super) fn on_preview_ready(&mut self) {
        let Some(out) = self.preview_loader.drain_latest() else {
            return;
        };
        if self.results.get(self.selected).map(|i| i.id.as_str()) != Some(out.item_id.as_str()) {
            return;
        }
        self.cached_preview_id = Some(out.item_id);
        self.cached_preview = Some(out.preview);
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.resize_to_fit_results();
            self.invalidate();
        }
    }

    pub(super) fn on_edit_changed(&mut self) {
        self.current_query = read_edit_text(self.edit_hwnd);
        self.dispatch_query();
    }

    pub(super) fn show_toast(&mut self, msg: &str) {
        self.toast = Some(msg.to_string());
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CLEAR_TOAST);
            let _ = SetTimer(Some(self.hwnd), TIMER_CLEAR_TOAST, 3000, None);
        }
        self.invalidate();
    }

    pub(super) fn invalidate(&self) {
        unsafe { let _ = InvalidateRect(Some(self.hwnd), None, false); }
    }

    pub(super) fn render_into_hdc(&mut self, hdc: HDC, width: i32, height: i32) {
        let mode = self.active_mode_badge();
        let attached_label = self.attached_capsule_label();
        let update_label = self.updater.available_release().map(|r| format!("↑ v{}", r.version));
        let filter_vis = self.is_filter_bar_visible();
        let state = RenderState {
            lang: self.lang,
            theme: self.theme,
            mode_badge: mode,
            attached_capsule: attached_label.as_deref(),
            update_badge: update_label.as_deref(),
            filter_bar_visible: filter_vis,
            active_filter: self.active_filter,
            hovered_filter: self.inter.hovered_filter,
            items: &self.results,
            selected: self.selected,
            hovered: self.inter.hovered,
            scroll_offset: self.scroll_offset,
            max_visible_rows: self.geom.max_visible_rows,
            preview_width_dip: self.geom.preview_width_dip,
            action_menu_open: self.action_menu_open,
            selected_action: self.selected_action,
            preview: self.cached_preview.as_ref(),
            everything_status: self.everything_status,
            toast: self.toast.as_deref(),
            dpi_scale: self.dpi_scale,
        };
        self.renderer.paint(hdc, width, height, &state);
    }
}

impl Drop for AppShell {
    fn drop(&mut self) {
        self.plugin_host.shutdown_all();
        self.preview_loader.cancel_all();
        self.renderer.hibernate_release();
        self.tray = None;
        unsafe {
            if !self.edit_font.is_invalid() {
                let _ = DeleteObject(self.edit_font.into());
            }
            if !self.edit_bg_brush.is_invalid() {
                let _ = DeleteObject(self.edit_bg_brush.into());
            }
        }
    }
}
