//! Self-test, soak diagnostics, and live hot-reload synchronization for `AppShell`.

use super::input::{register_configured_hotkey, register_optional_hotkey};
use super::{
    bootstrap::apply_cue_banner, AppShell, HOTKEY_CLIPBOARD_ID, HOTKEY_ID, HOTKEY_TRANSLATE_ID,
};
use crate::renderer::{compute_filter_counts, MIN_WINDOW_WIDTH_DIP};
use tomoyo_core::TomoyoConfig;
use tomoyo_protocol::{AttachedContext, SearchFilter};
use windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

impl AppShell {
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
        self.inter.filter_counts = (self.active_filter == SearchFilter::All)
            .then(|| compute_filter_counts(&self.results));
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
        let new_hotkey = fresh.hotkey.clone();
        let new_clip_hotkey = fresh.clipboard.hotkey.clone();
        let new_tr_hotkey = fresh.translate.hotkey.clone();
        self.preview_enabled = fresh.auto_preview;
        self.show_filter_bar = fresh.show_filter_bar;
        self.geom.list_width_dip = fresh.window.width_dip.clamp(MIN_WINDOW_WIDTH_DIP, 1400);
        self.geom.max_visible_rows = fresh.window.max_visible_rows.clamp(4, 18);
        self.geom.preview_width_dip = fresh.window.preview_width_dip.clamp(220, 620);
        self.geom.custom_pos = fresh.window.custom_x.zip(fresh.window.custom_y);
        let apps_changed = self
            .config
            .read()
            .map(|old| {
                old.apps.extra_dirs != fresh.apps.extra_dirs
                    || old.apps.auto_detect_desktop_organizers != fresh.apps.auto_detect_desktop_organizers
                    || old.apps.extra_scan_depth != fresh.apps.extra_scan_depth
            })
            .unwrap_or(true);
        if let Ok(mut cfg) = self.config.write() {
            *cfg = fresh;
        }
        self.lang = new_lang;
        unsafe {
            apply_cue_banner(self.edit_hwnd, new_lang);
        }
        self.apply_theme(&new_theme, false);
        if self.tray.is_some() {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ID);
                let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_CLIPBOARD_ID);
                let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_TRANSLATE_ID);
            }
            let (active, conflict) = register_configured_hotkey(self.hwnd, &new_hotkey);
            self.hotkey_label = active;
            self.hotkey_conflict = conflict;
            let _ = register_optional_hotkey(self.hwnd, HOTKEY_CLIPBOARD_ID, &new_clip_hotkey);
            let _ = register_optional_hotkey(self.hwnd, HOTKEY_TRANSLATE_ID, &new_tr_hotkey);
            if let Some(tray) = &mut self.tray {
                tray.update_tooltip(&self.hotkey_label);
            }
        }
        if force_provider_reload || apps_changed {
            self.router.reload_all();
        }
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.position_on_active_monitor();
            self.dispatch_query();
            self.invalidate();
        }
    }
}
