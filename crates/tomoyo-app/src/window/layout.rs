//! Window geometry, DPI/multi-monitor positioning, edit control layout, divider, filter & row hit-testing.

use super::bootstrap::create_edit_font;
use super::AppShell;
use crate::actions::shell_open;
use crate::renderer::{
    compute_filter_bar_layout, resolve_face_and_weight, FilterHit, DEFAULT_PREVIEW_WIDTH_DIP,
    DEFAULT_WINDOW_WIDTH_DIP, FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP, MAX_WINDOW_WIDTH_DIP,
    MIN_WINDOW_WIDTH_DIP,
};
use tomoyo_core::{ExternalPluginBridge, Lang, TomoyoConfig};
use tomoyo_protocol::{AttachedContext, SearchFilter};
use windows::Win32::Foundation::{LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTONULL,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetCursorPos, GetWindowRect, MoveWindow, SendMessageW, SetWindowPos,
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, WM_SETFONT,
};

impl AppShell {
    pub fn reset_window_geometry(&mut self) {
        self.geom.list_width_dip = DEFAULT_WINDOW_WIDTH_DIP;
        self.geom.max_visible_rows = 8;
        self.geom.preview_width_dip = DEFAULT_PREVIEW_WIDTH_DIP;
        self.geom.custom_pos = None;
        if let Ok(mut cfg) = self.config.write() {
            cfg.window = Default::default();
            let _ = cfg.save();
        }
        self.position_on_active_monitor();
        self.show_toast(self.lang.pick(
            "已重置窗口位置与默认尺寸",
            "Window position & size reset to defaults",
        ));
    }

    pub(super) fn persist_window_geometry(&mut self) {
        let mut rect = RECT::default();
        unsafe {
            if GetWindowRect(self.hwnd, &mut rect).is_err() {
                return;
            }
        }
        self.geom.custom_pos = Some((rect.left, rect.top));
        if let Ok(mut cfg) = self.config.write() {
            cfg.window.width_dip = self.geom.list_width_dip;
            cfg.window.max_visible_rows = self.geom.max_visible_rows;
            cfg.window.preview_width_dip = self.geom.preview_width_dip;
            cfg.window.custom_x = Some(rect.left);
            cfg.window.custom_y = Some(rect.top);
            let _ = cfg.save();
        }
    }

    pub(super) fn refresh_edit_font(&mut self) {
        let (face, _) = resolve_face_and_weight(
            &self.typography.font_family,
            400,
            self.font_regular,
            self.font_medium,
        );
        let new_font = create_edit_font(self.dpi_scale, face);
        unsafe { let _ = SendMessageW(self.edit_hwnd, WM_SETFONT, Some(WPARAM(new_font.0 as usize)), Some(LPARAM(1)));
            if !self.edit_font.is_invalid() { let _ = DeleteObject(self.edit_font.into()); } }
        self.edit_font = new_font;
    }

    pub(super) fn sync_dpi_scale(&mut self, new_scale: f32) {
        let clamped = new_scale.clamp(1.0, 4.0);
        if (clamped - self.dpi_scale).abs() < 0.01 {
            return;
        }
        self.dpi_scale = clamped;
        self.refresh_edit_font();
    }

    pub(super) fn handle_dpi_changed(&mut self, new_dpi: u32, suggested: Option<&RECT>) {
        if new_dpi > 0 {
            self.sync_dpi_scale(new_dpi as f32 / 96.0);
        }
        if let Some(rc) = suggested {
            let w = (rc.right - rc.left).max(120);
            let h = (rc.bottom - rc.top).max(64);
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    rc.left,
                    rc.top,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            self.layout_edit_control(w);
        }
        self.sync_inline_dock_bounds();
        self.invalidate();
    }

    pub(super) fn position_on_active_monitor(&mut self) {
        unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let cursor_mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);

            // Pick target monitor first so mixed-DPI monitors compute exact frame-1 dimensions.
            let target_mon = if let Some((cx, cy)) = self.geom.custom_pos {
                let m = MonitorFromPoint(POINT { x: cx + 160, y: cy + 30 }, MONITOR_DEFAULTTONULL);
                if !m.is_invalid() { m } else { cursor_mon }
            } else {
                cursor_mon
            };

            let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
            let dpi = if GetDpiForMonitor(target_mon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() && dpi_x > 0 {
                dpi_x
            } else {
                GetDpiForWindow(self.hwnd)
            };
            if dpi > 0 {
                self.sync_dpi_scale(dpi as f32 / 96.0);
            }

            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(target_mon, &mut mi);
            let work = mi.rcWork;
            let width = (self.desired_width_dip() as f32 * self.dpi_scale) as i32;
            let height = (self.desired_height_dip() as f32 * self.dpi_scale) as i32;

            let (x, y) = if let Some((cx, cy)) = self.geom.custom_pos {
                let valid = !MonitorFromPoint(POINT { x: cx + 160, y: cy + 30 }, MONITOR_DEFAULTTONULL).is_invalid();
                if valid {
                    (
                        cx.clamp(work.left, (work.right - width).max(work.left)),
                        cy.clamp(work.top, (work.bottom - height).max(work.top)),
                    )
                } else {
                    (work.left + ((work.right - work.left) - width) / 2, work.top + ((work.bottom - work.top) / 5).max(40))
                }
            } else {
                (work.left + ((work.right - work.left) - width) / 2, work.top + ((work.bottom - work.top) / 5).max(40))
            };
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, width, height, SWP_NOACTIVATE);
            self.layout_edit_control(width);
        }
    }

    pub(super) fn resize_to_fit_results(&mut self) {
        if self.inter.in_size_move || self.inter.dragging_divider {
            return;
        }
        let mut rect = RECT::default();
        unsafe {
            if GetWindowRect(self.hwnd, &mut rect).is_err() {
                return;
            }
        }
        let width = (self.desired_width_dip() as f32 * self.dpi_scale) as i32;
        let height = (self.desired_height_dip() as f32 * self.dpi_scale) as i32;
        let modal = super::is_modal_overlay_active();
        let z_flag = if modal { SWP_NOZORDER } else { Default::default() };
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                (!modal).then_some(HWND_TOPMOST),
                rect.left,
                rect.top,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | z_flag,
            );
        }
        self.layout_edit_control(width);
    }

    pub(super) fn layout_edit_control(&self, window_width_px: i32) {
        let left = (62.0 * self.dpi_scale) as i32;
        let top = (18.0 * self.dpi_scale) as i32;
        let mut right_pad_dip = 24.0f32;
        if self.active_mode_badge().is_some() {
            right_pad_dip += 132.0;
        }
        if self.attached.is_some() {
            right_pad_dip += 196.0;
        }
        let right_pad = (right_pad_dip * self.dpi_scale) as i32;
        let w = (window_width_px - left - right_pad).max(120);
        let h = (28.0 * self.dpi_scale) as i32;
        unsafe {
            let _ = MoveWindow(self.edit_hwnd, left, top, w, h, true);
        }
    }

    pub(super) fn is_preview_visible(&self) -> bool {
        (self.cached_preview.is_some() || self.inter.inline_dock_pid.is_some()) && !self.results.is_empty()
    }

    pub(super) fn is_filter_bar_visible(&self) -> bool {
        if !self.show_filter_bar || self.attached.is_some() {
            return false;
        }
        let t = self.current_query.trim();
        if t.starts_with(':') {
            return false;
        }
        let low = t.to_lowercase();
        let is_non_file_kw = low == "cb"
            || low.starts_with("cb ")
            || low == "w"
            || low.starts_with("w ")
            || low == "kill"
            || low.starts_with("kill ")
            || low == "ps"
            || low.starts_with("ps ")
            || low.starts_with("= ")
            || low.starts_with("b64 ")
            || low.starts_with("b64d ");
        if is_non_file_kw {
            return false;
        }
        !t.is_empty() || self.active_filter != SearchFilter::All
    }

    pub(super) fn desired_width_dip(&self) -> i32 {
        if self.is_preview_visible() {
            (self.geom.list_width_dip + self.geom.preview_width_dip).min(MAX_WINDOW_WIDTH_DIP)
        } else {
            self.geom.list_width_dip
        }
    }

    pub(super) fn filter_bar_height_dip(&self) -> i32 {
        if !self.is_filter_bar_visible() {
            return 0;
        }
        let tabs = self.visible_filter_tabs();
        compute_filter_bar_layout(
            self.geom.list_width_dip as f32,
            HEADER_HEIGHT_DIP as f32,
            &tabs,
            self.active_filter,
            self.inter.filter_counts.as_ref(),
            self.filter_bar_wrap,
        )
        .height_dip
    }

    pub(super) fn desired_height_dip(&self) -> i32 {
        let filter_h = self.filter_bar_height_dip();
        let rows = self.results.len().min(self.geom.max_visible_rows) as i32;
        if rows == 0 {
            HEADER_HEIGHT_DIP + filter_h + FOOTER_HEIGHT_DIP
        } else {
            let min_preview_rows = if self.inter.inline_dock_pid.is_some() { 7 } else if self.cached_preview.is_some() { 5 } else { 0 };
            let eff_rows = rows.max(min_preview_rows);
            HEADER_HEIGHT_DIP + filter_h + 8 + eff_rows * self.typography.row_height_dip() + FOOTER_HEIGHT_DIP
        }
    }

    pub(super) fn ensure_selected_visible(&mut self) {
        let cap = self.geom.max_visible_rows.max(1);
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + cap {
            self.scroll_offset = self.selected + 1 - cap;
        }
    }

    fn current_list_width_dip(&self) -> f32 {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let scale = self.dpi_scale.max(1.0);
        let w_dip = (rc.right - rc.left) as f32 / scale;
        if self.is_preview_visible() && w_dip >= 680.0 {
            let pw = (self.geom.preview_width_dip as f32).clamp(220.0, (w_dip * 0.48).max(220.0));
            (w_dip - pw).max(380.0)
        } else {
            w_dip
        }
    }

    pub(super) fn hit_test_logo_icon(&self, x_px: i32, y_px: i32) -> bool {
        let scale = self.dpi_scale.max(1.0);
        let (xd, yd) = (x_px as f32 / scale, y_px as f32 / scale);
        (10.0..=54.0).contains(&xd) && (9.0..=53.0).contains(&yd)
    }

    pub(super) fn hit_test_update_badge(&self, x_px: i32, y_px: i32) -> bool {
        let Some(rel) = self.updater.available_release() else {
            return false;
        };
        let mut rc = RECT::default();
        unsafe { let _ = GetClientRect(self.hwnd, &mut rc); }
        let scale = self.dpi_scale.max(1.0);
        let w_dip = (rc.right - rc.left) as f32 / scale;
        let fy0 = ((rc.bottom - rc.top) as f32 / scale) - FOOTER_HEIGHT_DIP as f32;
        let ver = format!("↑ v{}", rel.version);
        let uw = (ver.chars().count() as f32 * 7.2 + 18.0).clamp(64.0, 120.0);
        let ux0 = w_dip - 190.0 - uw;
        let (xd, yd) = (x_px as f32 / scale, y_px as f32 / scale);
        ux0 > 220.0 && xd >= ux0 && xd <= w_dip - 190.0 && yd >= fy0 + 5.0 && yd <= fy0 + 25.0
    }

    pub(super) fn is_over_preview_divider(&self, x_px: i32, y_px: i32) -> bool {
        if !self.is_preview_visible() {
            return false;
        }
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let scale = self.dpi_scale.max(1.0);
        let w_dip = (rc.right - rc.left) as f32 / scale;
        let h_dip = (rc.bottom - rc.top) as f32 / scale;
        if w_dip < 680.0 {
            return false;
        }
        let y_dip = y_px as f32 / scale;
        if y_dip < HEADER_HEIGHT_DIP as f32 || y_dip > h_dip - FOOTER_HEIGHT_DIP as f32 {
            return false;
        }
        let pw = (self.geom.preview_width_dip as f32).clamp(220.0, (w_dip * 0.48).max(220.0));
        let div_x_dip = (w_dip - pw).max(380.0);
        (x_px as f32 / scale - div_x_dip).abs() <= 5.0
    }

    pub(super) fn hit_test_filter_target(&self, x_px: i32, y_px: i32) -> Option<FilterHit> {
        if !self.is_filter_bar_visible() {
            return None;
        }
        let scale = self.dpi_scale.max(1.0);
        let tabs = self.visible_filter_tabs();
        compute_filter_bar_layout(
            self.current_list_width_dip(),
            HEADER_HEIGHT_DIP as f32,
            &tabs,
            self.active_filter,
            self.inter.filter_counts.as_ref(),
            self.filter_bar_wrap,
        )
        .hit_test(x_px as f32 / scale, y_px as f32 / scale)
    }

    pub(super) fn hit_test_row(&self, x_px: i32, y_px: i32) -> Option<usize> {
        let scale = self.dpi_scale.max(1.0);
        let x_dip = x_px as f32 / scale;
        let y_dip = y_px as f32 / scale;
        if x_dip > self.current_list_width_dip() {
            return None;
        }
        let list_top = HEADER_HEIGHT_DIP as f32 + self.filter_bar_height_dip() as f32 + 4.0;
        if y_dip < list_top {
            return None;
        }
        let slot = ((y_dip - list_top) / self.typography.row_height_dip() as f32) as usize;
        if slot >= self.geom.max_visible_rows.max(1) {
            return None;
        }
        let idx = self.scroll_offset + slot;
        (idx < self.results.len()).then_some(idx)
    }

    pub(super) fn active_mode_badge(&self) -> Option<String> {
        let t = self.current_query.trim();
        let l = self.lang;
        if t.starts_with(':') { return Some(l.pick("命令", "COMMAND").into()); }
        let low = t.to_lowercase();
        if low.starts_with("f ") || low.starts_with("ev ") { return Some(l.pick("文件搜索", "EVERYTHING").into()); }
        if low == "cb" || low.starts_with("cb ") { return Some(l.pick("剪贴板", "CLIPBOARD").into()); }
        if low == "w" || low.starts_with("w ") { return Some(l.pick("窗口切换", "WINDOWS").into()); }
        if low == "kill" || low.starts_with("kill ") || low == "ps" || low.starts_with("ps ") {
            return Some(l.pick("进程管理", "PROCESS").into());
        }
        if low.starts_with("= ") || low.starts_with("b64 ") || low.starts_with("b64d ") {
            return Some(l.pick("计算", "CALC").into());
        }
        if low == "snip" || low.starts_with("snip ") { return Some(l.pick("片段", "SNIPPET").into()); }
        if let Some(first) = low.split_whitespace().next() {
            if self.plugin_host.matches_keyword(first) { return Some(l.pick("插件", "PLUGIN").into()); }
            if self.config.read().map(|c| c.web_searches.iter().any(|w| w.keyword.eq_ignore_ascii_case(first))).unwrap_or(false) && low.contains(' ') {
                return Some(l.pick("网页搜索", "WEB").into());
            }
        }
        if self.active_filter != SearchFilter::All {
            if let SearchFilter::Custom(idx) = self.active_filter
                && let Some(lbl) = self.config.read().ok().and_then(|c| c.custom_filters.get(idx as usize).map(|t| t.label.clone())) {
                    return Some(lbl);
                }
            return Some(self.active_filter.label(l == Lang::Zh).into());
        }
        None
    }

    pub(super) fn attached_capsule_label(&self) -> Option<String> {
        let l = self.lang;
        match self.attached.as_ref()? {
            AttachedContext::Image { width, height, .. } => Some(match l {
                Lang::Zh => format!("📎 图片 {width}×{height}"),
                Lang::En => format!("📎 Img {width}×{height}"),
            }),
            AttachedContext::Files { paths } => Some(match l {
                Lang::Zh => format!("📎 {} 个文件", paths.len()),
                Lang::En => format!("📎 {} file(s)", paths.len()),
            }),
        }
    }

    pub(super) fn open_config_file_in_editor(&mut self) {
        let path = TomoyoConfig::config_path().to_string_lossy().into_owned();
        let _ = shell_open(&path, None, None);
    }

    pub(super) fn handle_window_resize(&mut self, new_w: i32, new_h: i32) {
        self.layout_edit_control(new_w);
        if self.inter.in_size_move {
            let scale = self.dpi_scale.max(1.0);
            let w_dip = (new_w as f32 / scale).round() as i32;
            let h_dip = (new_h as f32 / scale).round() as i32;
            self.geom.list_width_dip = if self.is_preview_visible() {
                (w_dip - self.geom.preview_width_dip).clamp(MIN_WINDOW_WIDTH_DIP, 1400)
            } else {
                w_dip.clamp(MIN_WINDOW_WIDTH_DIP, 1400)
            };
            if !self.results.is_empty() {
                let avail_h = (h_dip - HEADER_HEIGHT_DIP - self.filter_bar_height_dip() - FOOTER_HEIGHT_DIP - 8).max(0);
                self.geom.max_visible_rows = (avail_h / self.typography.row_height_dip()).clamp(4, 18) as usize;
            }
        }
        self.sync_inline_dock_bounds();
        self.invalidate();
    }
}
