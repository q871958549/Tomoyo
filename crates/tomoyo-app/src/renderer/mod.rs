//! Low-memory native renderer orchestrator (SDF 32-bit DIB + ClearType GDI + Async Shell Icons).

mod canvas;
pub(crate) mod filter_bar;
mod gdi;
mod gdi_pass;
mod sdf_pass;
mod theme;

pub use canvas::Canvas;
pub use filter_bar::{
    compute_filter_counts, compute_filter_hitboxes, FilterCounts, FILTER_BAR_HEIGHT_DIP,
};
pub use gdi::{dip_rect, draw_text_gdi, DibSurface, FontPack};
pub use theme::{Rgb, ThemePalette};

use crate::icons::IconCache;
use gdi_pass::paint_gdi_pass;
use sdf_pass::paint_sdf_pass;
use tomoyo_core::{Lang, ResolvedPreview};
use tomoyo_protocol::{ResultItem, SearchFilter};
use windows::Win32::Graphics::Gdi::HDC;

pub const DEFAULT_WINDOW_WIDTH_DIP: i32 = 720;
pub const DEFAULT_PREVIEW_WIDTH_DIP: i32 = 310;
pub const HEADER_HEIGHT_DIP: i32 = 62;
pub const ROW_HEIGHT_DIP: i32 = 54;
pub const FOOTER_HEIGHT_DIP: i32 = 30;
pub const MIN_WINDOW_WIDTH_DIP: i32 = 520;
pub const MAX_WINDOW_WIDTH_DIP: i32 = 1600;

pub struct RenderState<'a> {
    pub lang: Lang,
    pub theme: ThemePalette,
    pub query: &'a str,
    pub mode_badge: Option<&'a str>,
    pub attached_capsule: Option<&'a str>,
    pub update_badge: Option<&'a str>,
    pub filter_bar_visible: bool,
    pub active_filter: SearchFilter,
    pub hovered_filter: Option<SearchFilter>,
    pub filter_counts: Option<&'a FilterCounts>,
    pub items: &'a [ResultItem],
    pub selected: usize,
    pub hovered: Option<usize>,
    pub scroll_offset: usize,
    pub max_visible_rows: usize,
    pub preview_width_dip: i32,
    pub action_menu_open: bool,
    pub selected_action: usize,
    pub preview: Option<&'a ResolvedPreview>,
    pub everything_status: Option<&'static str>,
    pub toast: Option<&'a str>,
    pub dpi_scale: f32,
}

pub(super) struct FrameLayout {
    pub w_dip: f32,
    pub h_dip: f32,
    pub header_h: f32,
    pub row_h: f32,
    pub list_top: f32,
    pub fy0: f32,
    pub visible_cap: usize,
    pub visible_count: usize,
    pub has_preview: bool,
    pub list_w_dip: f32,
}

pub struct FluentRenderer {
    surface: Option<DibSurface>,
    fonts: Option<FontPack>,
    icons: IconCache,
    font_regular: &'static str,
    font_medium: &'static str,
}

impl FluentRenderer {
    pub fn new(font_regular: &'static str, font_medium: &'static str) -> windows::core::Result<Self> {
        Ok(Self {
            surface: None,
            fonts: None,
            icons: IconCache::new(),
            font_regular,
            font_medium,
        })
    }

    pub fn set_notify_hwnd(&self, hwnd: isize) {
        self.icons.set_notify_hwnd(hwnd);
    }

    pub fn drain_ready_icons(&mut self) -> bool {
        self.icons.drain_ready()
    }

    pub fn wait_icons_idle_for_test(&mut self) {
        self.icons.wait_idle_and_drain();
    }

    pub fn hibernate_release(&mut self) {
        self.icons.clear();
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
        let dpi_key = (scale * 100.0).round() as i32;
        if self.fonts.as_ref().map(|f| f.dpi_key) != Some(dpi_key) {
            self.fonts = Some(FontPack::create(scale, self.font_regular, self.font_medium));
        }
        if self.surface.as_ref().map(|s| (s.width, s.height)) != Some((width, height)) {
            self.surface = None;
            self.surface = DibSurface::create(screen_hdc, width, height);
        }

        let Some(surface) = self.surface.as_mut() else { return };
        let Some(fonts) = self.fonts.as_ref() else { return };

        let lay = compute_layout(width, height, scale, state);
        paint_sdf_pass(surface, width, height, scale, &lay, state);
        paint_gdi_pass(surface, fonts, &mut self.icons, width, height, scale, &lay, state, screen_hdc);
    }
}

fn compute_layout(width: i32, height: i32, scale: f32, state: &RenderState<'_>) -> FrameLayout {
    let w_dip = width as f32 / scale;
    let h_dip = height as f32 / scale;
    let header_h = HEADER_HEIGHT_DIP as f32;
    let footer_h = FOOTER_HEIGHT_DIP as f32;
    let row_h = ROW_HEIGHT_DIP as f32;
    let filter_h = if state.filter_bar_visible {
        FILTER_BAR_HEIGHT_DIP as f32
    } else {
        0.0
    };
    let list_top = header_h + filter_h + 4.0;
    let max_rows_by_h = ((h_dip - list_top - footer_h).max(0.0) / row_h).floor() as usize;
    let visible_cap = state.max_visible_rows.max(1).min(max_rows_by_h.max(1));
    let visible_count = state.items.len().min(visible_cap);
    let has_preview = state.preview.is_some() && !state.items.is_empty() && w_dip >= 680.0;
    let preview_w = (state.preview_width_dip as f32).clamp(220.0, (w_dip * 0.48).max(220.0));
    let list_w_dip = if has_preview { (w_dip - preview_w).max(380.0) } else { w_dip };
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
        list_w_dip,
    }
}
