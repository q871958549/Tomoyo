//! Lifecycle (theme/language/hibernation/shutdown), self-test, and hot-reload synchronization.

use super::bootstrap::{apply_cue_banner, apply_dwm_attributes};
use super::input::{register_all_hotkeys, unregister_all_hotkeys};
use super::{mi_collect, AppShell, TIMER_HIBERNATE};
use crate::actions::spawn_settings_window;
use crate::renderer::{compute_filter_counts, ThemePalette, MIN_WINDOW_WIDTH_DIP};
use tomoyo_core::{Lang, TomoyoConfig};
use tomoyo_protocol::{AttachedContext, SearchFilter};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, InvalidateRect};
use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, KillTimer};

impl AppShell {
    pub fn apply_theme(&mut self, theme_name: &str, persist: bool) {
        let new_theme = ThemePalette::from_name(theme_name);
        self.theme = new_theme;
        self.theme_name = theme_name.to_string();
        apply_dwm_attributes(self.hwnd, new_theme.is_dark);
        unsafe {
            if !self.edit_bg_brush.is_invalid() { let _ = DeleteObject(self.edit_bg_brush.into()); }
            self.edit_bg_brush = CreateSolidBrush(new_theme.bg.to_colorref());
            let _ = InvalidateRect(Some(self.edit_hwnd), None, true);
        }
        if persist {
            if let Ok(mut cfg) = self.config.write() { cfg.theme = theme_name.to_string(); let _ = cfg.save(); }
        }
        self.invalidate();
    }

    pub fn apply_language(&mut self, lang_code: &str, persist: bool) {
        let new_lang = Lang::from_code(lang_code);
        self.lang = new_lang;
        unsafe { apply_cue_banner(self.edit_hwnd, new_lang); }
        if persist {
            if let Ok(mut cfg) = self.config.write() { cfg.language = new_lang.code().to_string(); let _ = cfg.save(); }
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

    pub fn run_hibernate(&mut self) -> u64 {
        unsafe { let _ = KillTimer(Some(self.hwnd), TIMER_HIBERNATE); }
        self.preview_loader.purge_worker();
        self.renderer.hibernate_release();
        self.results.clear();
        self.results.shrink_to_fit();
        self.cached_preview_id = None;
        self.cached_preview = None;
        let cycles = self.router.hibernate_trim();
        unsafe {
            windows::Win32::System::Com::CoFreeUnusedLibraries();
            mi_collect(true);
        }
        cycles
    }

    pub(super) fn shutdown_clean(&mut self) {
        if self.shutting_down { return; }
        self.shutting_down = true;
        self.plugin_host.shutdown_all();
        self.clipboard.shutdown_checkpoint();
        self.preview_loader.cancel_all();
        self.tray = None;
    }

    pub fn is_renderer_hibernated(&self) -> bool {
        self.renderer.is_hibernated()
    }

    pub fn wait_background_idle_for_test(&mut self) {
        self.renderer.wait_icons_idle_for_test();
        self.preview_loader.wait_idle_and_drain();
    }

    pub fn self_test_paint_and_hibernate(&mut self, query: &str) -> usize {
        self.current_query = query.to_string();
        let generation = self.router.next_generation();
        self.results = self.router.search_with_filter(
            query,
            generation,
            20,
            self.clipboard.latest_snapshot(),
            self.attached.clone(),
            self.active_filter,
        );
        self.selected = 0;
        let tabs = self.visible_filter_tabs();
        self.inter.filter_counts = (self.active_filter == SearchFilter::All)
            .then(|| compute_filter_counts(&self.results, &tabs));
        self.inter.hovered = if self.results.len() > 1 { Some(1) } else { None };
        self.scroll_offset = 0;
        self.cached_preview_id = None;
        self.refresh_selected_preview();
        let count = self.results.len();

        let hdc = unsafe { windows::Win32::Graphics::Gdi::GetDC(Some(self.hwnd)) };
        let w = (self.desired_width_dip() as f32 * self.dpi_scale) as i32;
        let h = (self.desired_height_dip() as f32 * self.dpi_scale) as i32;
        self.render_into_hdc(hdc, w, h);
        unsafe {
            windows::Win32::Graphics::Gdi::ReleaseDC(Some(self.hwnd), hdc);
        }
        count
    }

    pub fn self_test_attached_paint(&mut self, ctx: AttachedContext) -> usize {
        self.attached = Some(ctx);
        let hits = self.self_test_paint_and_hibernate("");
        self.attached = None;
        hits
    }

    pub(super) fn reload_config_and_all(&mut self) {
        self.reload_config_internal(true);
    }

    pub(super) fn reload_config_from_settings(&mut self) {
        self.reload_config_internal(false);
    }

    fn reload_config_internal(&mut self, force_provider_reload: bool) {
        let fresh = TomoyoConfig::load_or_create();
        let new_lang = fresh.lang();
        let new_theme = fresh.theme.clone();
        self.preview_enabled = fresh.auto_preview;
        self.show_filter_bar = fresh.show_filter_bar;
        self.geom.list_width_dip = fresh.window.width_dip.clamp(MIN_WINDOW_WIDTH_DIP, 1400);
        self.geom.max_visible_rows = fresh.window.max_visible_rows.clamp(4, 18);
        self.geom.preview_width_dip = fresh.window.preview_width_dip.clamp(220, 620);
        self.geom.custom_pos = fresh.window.custom_x.zip(fresh.window.custom_y);
        let (apps_changed, plugins_changed, clip_was_on) = self
            .config
            .read()
            .map(|old| {
                (
                    old.apps.extra_dirs != fresh.apps.extra_dirs
                        || old.apps.auto_detect_desktop_organizers != fresh.apps.auto_detect_desktop_organizers
                        || old.apps.extra_scan_depth != fresh.apps.extra_scan_depth,
                    old.disabled_plugins != fresh.disabled_plugins,
                    old.is_plugin_enabled(tomoyo_core::PLUGIN_CLIPBOARD),
                )
            })
            .unwrap_or((true, true, true));
        let clip_now_on = fresh.is_plugin_enabled(tomoyo_core::PLUGIN_CLIPBOARD);
        let cfg_for_hk = fresh.clone();
        if let Ok(mut cfg) = self.config.write() {
            *cfg = fresh;
        }
        self.lang = new_lang;
        unsafe { apply_cue_banner(self.edit_hwnd, new_lang); }
        self.apply_theme(&new_theme, false);
        if self.tray.is_some() {
            if clip_was_on != clip_now_on {
                unsafe {
                    if clip_now_on {
                        let _ = windows::Win32::System::DataExchange::AddClipboardFormatListener(self.hwnd);
                    } else {
                        let _ = windows::Win32::System::DataExchange::RemoveClipboardFormatListener(self.hwnd);
                    }
                }
            }
            unregister_all_hotkeys(self.hwnd);
            let (active, conflict) = register_all_hotkeys(self.hwnd, &cfg_for_hk);
            self.hotkey_label = active;
            self.hotkey_conflict = conflict;
            if let Some(tray) = &mut self.tray {
                tray.update_tooltip(&self.hotkey_label);
            }
        }
        if force_provider_reload || apps_changed || plugins_changed {
            self.router.reload_all();
        }
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.position_on_active_monitor();
            self.dispatch_query();
            self.invalidate();
        }
    }
}
