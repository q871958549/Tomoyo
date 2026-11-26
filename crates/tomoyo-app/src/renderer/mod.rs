//! Low-memory native renderer orchestrator (SDF 32-bit DIB + ClearType GDI + Async Shell Icons).

mod canvas;
pub(crate) mod filter_bar;
#[cfg(windows)]
pub(crate) mod filter_sidebar;
#[cfg(windows)]
mod gdi;
#[cfg(windows)]
mod gdi_pass;
pub(crate) mod scrollbar;
mod sdf_pass;
mod theme;

pub use canvas::Canvas;
#[cfg(not(windows))]
pub use filter_bar::{compute_filter_bar_layout, FilterHit};
pub use filter_bar::{compute_filter_counts, FilterCounts};
#[cfg(windows)]
pub use filter_sidebar::{
    compute_drag_drop_slot, compute_sidebar_layout, hit_test_sidebar, should_show_filter_sidebar,
    SidebarDragVisual, SidebarHit, SIDEBAR_WIDTH_DIP,
};
pub use scrollbar::ScrollbarVisual;
#[cfg(windows)]
pub use scrollbar::{compute_scrollbar_metrics, ScrollbarHit, ScrollbarMetrics};
#[cfg(not(windows))]
pub use filter_bar::{item_matches_filter, FilterPillAnim};
#[cfg(windows)]
pub use gdi::{
    dip_rect, draw_multiline_gdi, draw_text_gdi, resolve_face_and_weight, DibSurface, FontKey,
    FontPack,
};
pub(crate) use sdf_pass::paint_sdf_pass;
pub use theme::{Rgb, ThemePalette};
#[cfg(not(windows))]
pub use theme::category_rgb;

#[cfg(windows)]
use crate::icons::IconCache;
#[cfg(windows)]
use crate::image_icons::ImageIconCache;
#[cfg(windows)]
use gdi_pass::paint_gdi_pass;
use tomoyo_core::{Lang, ResolvedPreview, TypographyConfig};
use tomoyo_protocol::{ResultItem, SearchFilter, VisibleFilterTab};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::HDC;

#[cfg_attr(not(windows), allow(dead_code))]
pub const DEFAULT_WINDOW_WIDTH_DIP: i32 = 840;
#[cfg_attr(not(windows), allow(dead_code))]
pub const DEFAULT_PREVIEW_WIDTH_DIP: i32 = 310;
pub const HEADER_HEIGHT_DIP: i32 = 62;
pub const FOOTER_HEIGHT_DIP: i32 = 30;
pub const MIN_WINDOW_WIDTH_DIP: i32 = 520;
pub const MAX_WINDOW_WIDTH_DIP: i32 = 1600;

pub struct RenderState<'a> {
    pub lang: Lang,
    pub theme: ThemePalette,
    pub query: &'a str,
    pub hovered_logo: bool,
    pub mode_badge: Option<&'a str>,
    pub attached_capsule: Option<&'a str>,
    pub update_badge: Option<&'a str>,
    pub filter_bar_visible: bool,
    #[cfg_attr(windows, allow(dead_code))]
    pub filter_bar_wrap: bool,
    pub filter_tabs: &'a [VisibleFilterTab],
    pub active_filter: SearchFilter,
    pub hovered_filter: Option<SearchFilter>,
    #[cfg_attr(windows, allow(dead_code))]
    pub hovered_overflow: bool,
    pub filter_counts: Option<&'a FilterCounts>,
    #[cfg_attr(windows, allow(dead_code))]
    pub filter_pill_override: Option<(f32, f32)>,
    #[cfg(windows)]
    pub sidebar_scroll: f32,
    #[cfg(windows)]
    pub hovered_add_tag: bool,
    #[cfg(windows)]
    pub sidebar_drag: Option<SidebarDragVisual>,
    pub typography: &'a TypographyConfig,
    pub items: &'a [ResultItem],
    pub selected: usize,
    pub hovered: Option<usize>,
    pub launching_row: Option<(usize, bool)>,
    pub scroll_offset: usize,
    pub scrollbar_visual: ScrollbarVisual,
    pub max_visible_rows: usize,
    pub preview_width_dip: i32,
    pub action_menu_open: bool,
    pub selected_action: usize,
    pub preview: Option<&'a ResolvedPreview>,
    pub everything_status: Option<&'static str>,
    pub toast: Option<&'a str>,
    pub dpi_scale: f32,
}

pub(crate) struct FrameLayout {
    pub w_dip: f32,
    pub h_dip: f32,
    pub header_h: f32,
    pub row_h: f32,
    pub list_top: f32,
    pub fy0: f32,
    pub visible_cap: usize,
    pub visible_count: usize,
    pub has_preview: bool,
    pub list_x0: f32,
    pub list_x1: f32,
    pub list_w_dip: f32,
}

#[cfg(windows)]
pub struct FluentRenderer {
    surface: Option<DibSurface>,
    fonts: Option<FontPack>,
    icons: IconCache,
    image_icons: ImageIconCache,
    font_regular: &'static str,
    font_medium: &'static str,
}

#[cfg(windows)]
impl FluentRenderer {
    pub fn new(font_regular: &'static str, font_medium: &'static str) -> windows::core::Result<Self> {
        Ok(Self {
            surface: None,
            fonts: None,
            icons: IconCache::new(),
            image_icons: ImageIconCache::new(),
            font_regular,
            font_medium,
        })
    }

    pub fn set_notify_hwnd(&self, hwnd: isize) {
        self.icons.set_notify_hwnd(hwnd);
        self.image_icons.set_notify_hwnd(hwnd);
    }

    pub fn drain_ready_icons(&mut self) -> bool {
        let shell_upd = self.icons.drain_ready();
        let img_upd = self.image_icons.drain_ready();
        shell_upd || img_upd
    }

    pub fn wait_icons_idle_for_test(&mut self) {
        self.icons.wait_idle_and_drain();
        self.image_icons.wait_idle_and_drain();
    }

    pub fn hibernate_release(&mut self) {
        self.icons.clear();
        self.image_icons.clear();
        self.surface = None;
        self.fonts = None;
    }

    pub fn is_hibernated(&self) -> bool {
        self.surface.is_none() && self.fonts.is_none()
    }

    pub fn paint(&mut self, screen_hdc: HDC, width: i32, height: i32, state: &RenderState<'_>) {
        if width <= 0 || height <= 0 {
            return;
        }
        let scale = state.dpi_scale.max(1.0);
        let want_key = FontKey::from_config(scale, state.typography);
        if self.fonts.as_ref().map(|f| &f.key) != Some(&want_key) {
            self.fonts = Some(FontPack::create(
                scale,
                self.font_regular,
                self.font_medium,
                state.typography,
            ));
        }
        if self.surface.as_ref().map(|s| (s.width, s.height)) != Some((width, height)) {
            self.surface = None;
            self.surface = DibSurface::create(screen_hdc, width, height);
        }

        let Some(surface) = self.surface.as_mut() else { return };
        let Some(fonts) = self.fonts.as_ref() else { return };

        let lay = compute_frame_layout(width, height, scale, state);
        let icon_px = (32.0 * scale).round() as u32;
        for slot in 0..lay.visible_count {
            let idx = state.scroll_offset + slot;
            if let Some(item) = state.items.get(idx) {
                match &item.icon {
                    tomoyo_protocol::IconSpec::ImageFile(p) => {
                        let _ = self.image_icons.get_or_enqueue(p, icon_px);
                    }
                    tomoyo_protocol::IconSpec::ShellPath(p) => {
                        let _ = self.icons.get_or_enqueue(p, icon_px);
                    }
                    _ => {}
                }
            }
        }
        paint_sdf_pass(
            surface.pixels_mut(),
            &mut self.icons,
            &mut self.image_icons,
            width,
            height,
            scale,
            &lay,
            state,
        );
        paint_gdi_pass(surface, fonts, &mut self.icons, &mut self.image_icons, width, height, scale, &lay, state, screen_hdc);
    }
}

pub(crate) fn compute_frame_layout(width: i32, height: i32, scale: f32, state: &RenderState<'_>) -> FrameLayout {
    let w_dip = width as f32 / scale;
    let h_dip = height as f32 / scale;
    let header_h = HEADER_HEIGHT_DIP as f32;
    let footer_h = FOOTER_HEIGHT_DIP as f32;
    let row_h = state.typography.row_height_dip() as f32;
    #[cfg(windows)]
    let (list_x0, has_preview, list_w_dip, filter_h) = {
        let lx0 = if state.filter_bar_visible { filter_sidebar::SIDEBAR_WIDTH_DIP } else { 0.0 };
        let avail_w = (w_dip - lx0).max(320.0);
        let has_prev = state.preview.is_some() && !state.items.is_empty() && avail_w >= 540.0;
        let preview_w = (state.preview_width_dip as f32).clamp(220.0, (avail_w * 0.46).max(220.0));
        let lw = if has_prev { (avail_w - preview_w).max(320.0) } else { avail_w };
        (lx0, has_prev, lw, 0.0f32)
    };
    #[cfg(not(windows))]
    let (list_x0, has_preview, list_w_dip, filter_h) = {
        let has_prev = state.preview.is_some() && !state.items.is_empty() && w_dip >= 680.0;
        let preview_w = (state.preview_width_dip as f32).clamp(220.0, (w_dip * 0.48).max(220.0));
        let lw = if has_prev { (w_dip - preview_w).max(360.0) } else { w_dip };
        let fh = if state.filter_bar_visible {
            compute_filter_bar_layout(
                lw,
                header_h,
                state.filter_tabs,
                state.active_filter,
                state.filter_counts,
                state.filter_bar_wrap,
            )
            .height_dip as f32
        } else {
            0.0
        };
        (0.0f32, has_prev, lw, fh)
    };
    let list_x1 = list_x0 + list_w_dip;
    let list_top = header_h + filter_h + 4.0;
    let max_rows_by_h = ((h_dip - list_top - footer_h).max(0.0) / row_h).floor() as usize;
    let visible_cap = state.max_visible_rows.max(1).min(max_rows_by_h.max(1));
    let visible_count = state.items.len().min(visible_cap);
    FrameLayout {
        w_dip,
        h_dip,
        header_h,
        row_h,
        list_top,
        fy0: h_dip - footer_h,
        visible_cap,
        visible_count,
        has_preview,
        list_x0,
        list_x1,
        list_w_dip,
    }
}
