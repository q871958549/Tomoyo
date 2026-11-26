//! Linux window geometry tracking, 250 ms debounced persistence, divider hit-testing & reset.

use crate::linux::shell_state::{with_current_core, AppCore};
use crate::linux::{
    shell_events, tomoyo_kwin_pos_start, tomoyo_kwin_pos_stop, tomoyo_kwin_pos_update_target,
    tomoyo_linux_add_timeout, tomoyo_linux_remove_source, tomoyo_linux_ui_is_wayland,
    tomoyo_linux_ui_resize, tomoyo_linux_ui_screen_info, tomoyo_linux_ui_show,
};
use crate::renderer::{
    DEFAULT_PREVIEW_WIDTH_DIP, DEFAULT_WINDOW_WIDTH_DIP, FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP,
    MAX_WINDOW_WIDTH_DIP, MIN_WINDOW_WIDTH_DIP,
};
use std::ffi::{c_int, c_void};
use std::time::Instant;
use tomoyo_core::TomoyoConfig;

impl AppCore {
    pub(super) fn init_kwin_pos_bridge(&mut self, core_ptr: *mut c_void) {
        let (hp, kx, ky) = match self.custom_pos {
            Some((x, y)) => (1, x, y),
            None => (0, 0, 0),
        };
        self.kwin_pos = unsafe {
            tomoyo_kwin_pos_start(hp, kx, ky, shell_events::on_kwin_pos_cb, core_ptr)
        };
    }

    pub(super) fn stop_window_geom(&mut self) {
        self.flush_geometry_on_hide();
        if !self.kwin_pos.is_null() {
            unsafe { tomoyo_kwin_pos_stop(self.kwin_pos); }
            self.kwin_pos = std::ptr::null_mut();
        }
    }

    pub(super) fn flush_geometry_on_hide(&mut self) {
        if self.geom_save_source != 0 {
            self.persist_window_geometry();
        }
    }

    pub(super) fn compute_show_geometry(&mut self) -> (i32, i32, i32, i32) {
        let (mut sx, mut sy, mut sw, mut sh, mut _sc) = (0, 0, 1920, 1080, 1);
        if !self.ui.is_null() {
            unsafe {
                tomoyo_linux_ui_screen_info(self.ui, &mut sx, &mut sy, &mut sw, &mut sh, &mut _sc);
            }
        }
        let w = self.desired_width_dip();
        let h = self.desired_height_dip();
        self.last_applied_height_dip = h;
        let (x, y) = match self.custom_pos {
            Some((cx, cy)) => (
                cx.clamp(sx, (sx + sw - w).max(sx)),
                cy.clamp(sy, (sy + sh - h).max(sy)),
            ),
            None => (sx + (sw - w) / 2, sy + (sh / 5).max(80)),
        };
        (x, y, w, h)
    }

    pub(super) fn sync_window_geom_from_config(&mut self, fresh: &TomoyoConfig) {
        self.list_width_dip = fresh.window.width_dip.clamp(MIN_WINDOW_WIDTH_DIP, MAX_WINDOW_WIDTH_DIP);
        self.max_visible_rows = fresh.window.max_visible_rows.clamp(4, 18);
        self.preview_width_dip = fresh.window.preview_width_dip.clamp(220, 620);
        self.custom_height_dip = fresh
            .window
            .custom_height_dip
            .map(|v| v.clamp(HEADER_HEIGHT_DIP + FOOTER_HEIGHT_DIP, 1080));
        let new_pos = fresh.window.custom_x.zip(fresh.window.custom_y);
        if new_pos != self.custom_pos {
            self.custom_pos = new_pos;
            let (hp, kx, ky) = match self.custom_pos {
                Some((x, y)) => (1, x, y),
                None => (0, 0, 0),
            };
            unsafe { tomoyo_kwin_pos_update_target(self.kwin_pos, hp, kx, ky); }
        }
    }

    pub(super) fn visible_row_capacity(&self) -> usize {
        let row_h = self.typography.row_height_dip().max(1);
        let max_by_h = ((self.desired_height_dip() - HEADER_HEIGHT_DIP - 4 - FOOTER_HEIGHT_DIP).max(0)
            / row_h) as usize;
        if max_by_h == 0 {
            1
        } else {
            self.max_visible_rows.max(1).min(max_by_h)
        }
    }

    pub(super) fn is_over_preview_divider(&self, x_dip: f32, y_dip: f32) -> bool {
        if !self.is_preview_visible() {
            return false;
        }
        let w_dip = self.desired_width_dip() as f32;
        let h_dip = self.desired_height_dip() as f32;
        let (lx0, lx1) = self.list_bounds_dip(w_dip);
        if (w_dip - lx0) < 540.0
            || y_dip < HEADER_HEIGHT_DIP as f32
            || y_dip > h_dip - FOOTER_HEIGHT_DIP as f32
        {
            return false;
        }
        let dx = x_dip - lx1;
        (-3.5..=5.0).contains(&dx)
    }

    fn apply_resized_dims(&mut self, w: i32, h: i32) {
        let min_h = HEADER_HEIGHT_DIP + FOOTER_HEIGHT_DIP;
        let nw = w.clamp(MIN_WINDOW_WIDTH_DIP, MAX_WINDOW_WIDTH_DIP);
        let nh = h.clamp(min_h, 1080);
        self.list_width_dip = nw;
        if self.last_applied_height_dip > 0 && nh != self.last_applied_height_dip {
            self.custom_height_dip = Some(nh);
            let avail_h = (nh - min_h - 8).max(0);
            self.max_visible_rows =
                (avail_h / self.typography.row_height_dip()).clamp(4, 18) as usize;
        }
        self.last_applied_height_dip = nh;
        let max_off = self.results.len().saturating_sub(self.visible_row_capacity());
        self.scroll_offset = self.scroll_offset.min(max_off);
        self.layout_entry(nw);
        self.invalidate();
    }

    pub(super) fn handle_window_configure(&mut self, x: i32, y: i32, w: i32, h: i32, flags: i32) {
        self.last_geom_change_at = Some(Instant::now());
        if (flags & 1) != 0 {
            self.apply_resized_dims(w, h);
        }
        if (flags & 2) != 0 {
            self.custom_pos = Some((x, y));
        }
        if (flags & 3) != 0 {
            self.schedule_persist_geometry();
        }
    }

    pub(super) fn handle_kwin_pos_report(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.last_geom_change_at = Some(Instant::now());
        self.custom_pos = Some((x, y));
        if w >= MIN_WINDOW_WIDTH_DIP && h >= HEADER_HEIGHT_DIP + FOOTER_HEIGHT_DIP {
            self.apply_resized_dims(w, h);
        }
        self.persist_window_geometry();
    }

    pub(super) fn cancel_geom_save_timer(&mut self) {
        if self.geom_save_source != 0 {
            unsafe { tomoyo_linux_remove_source(self.geom_save_source); }
            self.geom_save_source = 0;
        }
    }

    pub(super) fn schedule_persist_geometry(&mut self) {
        self.cancel_geom_save_timer();
        unsafe extern "C" fn on_save_timer(_d: *mut c_void) -> c_int {
            with_current_core(|st| {
                st.geom_save_source = 0;
                st.persist_window_geometry();
            });
            0
        }
        self.geom_save_source =
            unsafe { tomoyo_linux_add_timeout(250, on_save_timer, std::ptr::null_mut()) };
    }

    pub(super) fn persist_window_geometry(&mut self) {
        self.cancel_geom_save_timer();
        if let Ok(mut cfg) = self.config.write() {
            cfg.window.width_dip = self.list_width_dip;
            cfg.window.max_visible_rows = self.max_visible_rows;
            cfg.window.preview_width_dip = self.preview_width_dip;
            cfg.window.custom_height_dip = self.custom_height_dip;
            cfg.window.custom_x = self.custom_pos.map(|p| p.0);
            cfg.window.custom_y = self.custom_pos.map(|p| p.1);
            let _ = cfg.save();
        }
    }

    pub(super) fn reset_window_geometry(&mut self) {
        self.cancel_geom_save_timer();
        self.list_width_dip = DEFAULT_WINDOW_WIDTH_DIP;
        self.max_visible_rows = 8;
        self.preview_width_dip = DEFAULT_PREVIEW_WIDTH_DIP;
        self.custom_height_dip = None;
        self.custom_pos = None;
        if let Ok(mut cfg) = self.config.write() {
            cfg.window = Default::default();
            let _ = cfg.save();
        }
        unsafe { tomoyo_kwin_pos_update_target(self.kwin_pos, 0, 0, 0); }
        let w = self.desired_width_dip();
        let h = self.desired_height_dip();
        self.last_applied_height_dip = h;
        if !self.ui.is_null() && self.is_visible() {
            if unsafe { tomoyo_linux_ui_is_wayland() } == 0 {
                let (mut sx, mut sy, mut sw, mut sh, mut _sc) = (0, 0, 1920, 1080, 1);
                unsafe {
                    tomoyo_linux_ui_screen_info(self.ui, &mut sx, &mut sy, &mut sw, &mut sh, &mut _sc);
                }
                let x = sx + (sw - w) / 2;
                let y = sy + (sh / 5).max(80);
                unsafe { tomoyo_linux_ui_show(self.ui, x, y, w, h); }
            } else {
                unsafe { tomoyo_linux_ui_resize(self.ui, w, h); }
            }
            self.layout_entry(w);
        }
        self.invalidate();
        self.show_toast(self.lang.pick(
            "已重置窗口位置与默认尺寸",
            "Window position & size reset to defaults",
        ));
    }
}

#[cfg(test)]
mod tests {
    use crate::linux::AppShell;
    use std::sync::{Arc, RwLock};
    use tomoyo_core::{FrecencyStore, PrivateFonts, QueryRouter, TomoyoConfig, UpdateManager};
    use tomoyo_plugin_host::PluginHost;

    fn test_shell(custom_height_dip: Option<i32>) -> Box<AppShell> {
        let cfg = TomoyoConfig {
            window: tomoyo_core::WindowConfig {
                width_dip: 840,
                max_visible_rows: 8,
                preview_width_dip: 310,
                custom_height_dip,
                custom_x: Some(120),
                custom_y: Some(90),
            },
            ..Default::default()
        };
        let cfg_arc = Arc::new(RwLock::new(cfg));
        let host = PluginHost::new(Vec::new());
        let frecency = Arc::new(RwLock::new(FrecencyStore::load_or_default(
            std::env::temp_dir().join("tomoyo_geom_test_frecency.json"),
        )));
        let router = Arc::new(QueryRouter::new(Vec::new(), None, frecency));
        let updater = UpdateManager::with_cached("q871958549/Tomoyo");
        let fonts = PrivateFonts::load_default();
        AppShell::create(cfg_arc, router, host, updater, fonts.regular_family(), fonts.medium_family(), false)
            .expect("headless shell")
    }

    #[test]
    fn width_resize_keeps_auto_height_and_height_resize_persists_custom_height() {
        let sh = test_shell(None);
        {
            let mut c = sh.core.borrow_mut();
            let auto_h = c.desired_height_dip();
            c.last_applied_height_dip = auto_h;
            c.apply_resized_dims(1040, auto_h);
            assert_eq!(c.list_width_dip, 1040);
            assert_eq!(c.custom_height_dip, None);

            c.apply_resized_dims(1040, 620);
            assert_eq!(c.custom_height_dip, Some(620));
            assert_eq!(c.desired_height_dip(), 620);
            assert_eq!(c.visible_row_capacity(), 9);
            c.cancel_geom_save_timer();
        }
    }
}
