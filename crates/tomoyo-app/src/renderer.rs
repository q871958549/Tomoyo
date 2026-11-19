//! Ultra-low-memory Fluent renderer using a pure-Rust SDF anti-aliased 32-bit
//! `CreateDIBSection` framebuffer + native ClearType GDI text + `DrawIconEx` shell icons
//! + Modern White artistic brand emblem + multi-theme palettes + right-side live preview pane.

use crate::brand::paint_modern_white_emblem;
use crate::icons::IconCache;
use std::ffi::c_void;
use tomoyo_core::{current_process_stats, Lang, ResolvedPreview};
use tomoyo_protocol::{IconSpec, ResultCategory, ResultItem};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    GdiFlush, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DRAW_TEXT_FORMAT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE,
    DT_TOP, DT_VCENTER, DT_WORDBREAK, FF_DONTCARE, FW_BOLD, FW_MEDIUM, FW_NORMAL, FW_SEMIBOLD,
    HBITMAP, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_NORMAL};

pub const DEFAULT_WINDOW_WIDTH_DIP: i32 = 720;
pub const DEFAULT_PREVIEW_WIDTH_DIP: i32 = 310;
pub const HEADER_HEIGHT_DIP: i32 = 62;
pub const ROW_HEIGHT_DIP: i32 = 54;
pub const FOOTER_HEIGHT_DIP: i32 = 30;
pub const MIN_WINDOW_WIDTH_DIP: i32 = 520;
pub const MAX_WINDOW_WIDTH_DIP: i32 = 1600;

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn to_colorref(self) -> COLORREF {
        COLORREF((self.0 as u32) | ((self.1 as u32) << 8) | ((self.2 as u32) << 16))
    }
}

#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub is_dark: bool,
    pub bg: Rgb,
    pub footer_bg: Rgb,
    pub preview_bg: Rgb,
    pub accent: Rgb,
    pub card_sel: Rgb,
    pub card_hover: Rgb,
    pub scroll_thumb: Rgb,
    pub pill_bg: Rgb,
    pub pop_bg: Rgb,
    pub border: Rgb,
    pub divider: Rgb,
    pub text_primary: Rgb,
    pub text_muted: Rgb,
    pub on_accent: Rgb,
}

impl ThemePalette {
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "light" | "white" => Self {
                is_dark: false,
                bg: Rgb(0xF8, 0xF9, 0xFC),
                footer_bg: Rgb(0xF0, 0xF2, 0xF7),
                preview_bg: Rgb(0xFF, 0xFF, 0xFF),
                accent: Rgb(0x18, 0x1C, 0x28),
                card_sel: Rgb(0xEA, 0xEE, 0xF6),
                card_hover: Rgb(0xF1, 0xF4, 0xFA),
                scroll_thumb: Rgb(0xC2, 0xCA, 0xDB),
                pill_bg: Rgb(0xE6, 0xEA, 0xF2),
                pop_bg: Rgb(0xFF, 0xFF, 0xFF),
                border: Rgb(0xD5, 0xDB, 0xE7),
                divider: Rgb(0xE2, 0xE6, 0xF0),
                text_primary: Rgb(0x15, 0x18, 0x22),
                text_muted: Rgb(0x63, 0x6B, 0x7E),
                on_accent: Rgb(0xFC, 0xFC, 0xFE),
            },
            "midnight" => Self {
                is_dark: true,
                bg: Rgb(0x0E, 0x10, 0x1A),
                footer_bg: Rgb(0x0A, 0x0B, 0x13),
                preview_bg: Rgb(0x12, 0x15, 0x23),
                accent: Rgb(0x63, 0x66, 0xF1),
                card_sel: Rgb(0x1B, 0x20, 0x35),
                card_hover: Rgb(0x15, 0x19, 0x2B),
                scroll_thumb: Rgb(0x36, 0x42, 0x68),
                pill_bg: Rgb(0x1E, 0x24, 0x3C),
                pop_bg: Rgb(0x14, 0x18, 0x28),
                border: Rgb(0x28, 0x30, 0x4A),
                divider: Rgb(0x1E, 0x24, 0x38),
                text_primary: Rgb(0xF1, 0xF5, 0xF9),
                text_muted: Rgb(0x88, 0x94, 0xB0),
                on_accent: Rgb(0xFF, 0xFF, 0xFF),
            },
            _ => Self {
                is_dark: true,
                bg: Rgb(0x16, 0x16, 0x1E),
                footer_bg: Rgb(0x11, 0x11, 0x18),
                preview_bg: Rgb(0x1A, 0x1B, 0x25),
                accent: Rgb(0x7C, 0x6A, 0xF7),
                card_sel: Rgb(0x24, 0x25, 0x36),
                card_hover: Rgb(0x1D, 0x1E, 0x2B),
                scroll_thumb: Rgb(0x3C, 0x40, 0x5A),
                pill_bg: Rgb(0x28, 0x29, 0x3C),
                pop_bg: Rgb(0x1C, 0x1D, 0x2A),
                border: Rgb(0x2E, 0x2F, 0x40),
                divider: Rgb(0x26, 0x27, 0x36),
                text_primary: Rgb(0xF3, 0xF4, 0xF8),
                text_muted: Rgb(0x8E, 0x93, 0xA8),
                on_accent: Rgb(0xFF, 0xFF, 0xFF),
            },
        }
    }
}

pub struct RenderState<'a> {
    pub lang: Lang,
    pub theme: ThemePalette,
    pub mode_badge: Option<&'a str>,
    pub attached_capsule: Option<&'a str>,
    pub update_badge: Option<&'a str>,
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

struct DibSurface {
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_obj: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}

impl DibSurface {
    fn create(ref_dc: HDC, width: i32, height: i32) -> Option<Self> {
        unsafe {
            let mem_dc = CreateCompatibleDC(Some(ref_dc));
            if mem_dc.is_invalid() {
                return None;
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap =
                CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            if bitmap.is_invalid() || bits.is_null() {
                let _ = DeleteDC(mem_dc);
                return None;
            }
            let old_obj = SelectObject(mem_dc, bitmap.into());
            let _ = SetBkMode(mem_dc, TRANSPARENT);
            Some(Self {
                mem_dc,
                bitmap,
                old_obj,
                bits: bits as *mut u8,
                width,
                height,
            })
        }
    }

    fn pixels_mut(&mut self) -> &mut [u8] {
        let len = (self.width as usize) * (self.height as usize) * 4;
        unsafe { std::slice::from_raw_parts_mut(self.bits, len) }
    }
}

impl Drop for DibSurface {
    fn drop(&mut self) {
        unsafe {
            if !self.mem_dc.is_invalid() {
                let _ = SelectObject(self.mem_dc, self.old_obj);
                if !self.bitmap.is_invalid() {
                    let _ = DeleteObject(self.bitmap.into());
                }
                let _ = DeleteDC(self.mem_dc);
            }
        }
    }
}

struct FontPack {
    dpi_key: i32,
    title: HFONT,
    subtitle: HFONT,
    badge: HFONT,
    footer: HFONT,
    preview_body: HFONT,
    preview_mono: HFONT,
}

impl FontPack {
    fn create(dpi_scale: f32, regular_face: &str, medium_face: &str) -> Self {
        let dpi_key = (dpi_scale * 100.0).round() as i32;
        Self {
            dpi_key,
            title: make_gdi_font(15.5 * dpi_scale, FW_SEMIBOLD.0 as i32, medium_face),
            subtitle: make_gdi_font(12.5 * dpi_scale, FW_NORMAL.0 as i32, regular_face),
            badge: make_gdi_font(11.0 * dpi_scale, FW_BOLD.0 as i32, medium_face),
            footer: make_gdi_font(12.0 * dpi_scale, FW_MEDIUM.0 as i32, regular_face),
            preview_body: make_gdi_font(12.5 * dpi_scale, FW_NORMAL.0 as i32, regular_face),
            preview_mono: make_gdi_font(12.0 * dpi_scale, FW_NORMAL.0 as i32, "Consolas"),
        }
    }
}

impl Drop for FontPack {
    fn drop(&mut self) {
        unsafe {
            for f in [
                self.title,
                self.subtitle,
                self.badge,
                self.footer,
                self.preview_body,
                self.preview_mono,
            ] {
                if !f.is_invalid() {
                    let _ = DeleteObject(f.into());
                }
            }
        }
    }
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

    /// Releases ALL offscreen DIB surfaces, GDI fonts, and cached shell `HICON`s on hibernation!
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

        let need_surface = match &self.surface {
            Some(s) => s.width != width || s.height != height,
            None => true,
        };
        if need_surface {
            self.surface = None;
            self.surface = DibSurface::create(screen_hdc, width, height);
        }

        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Some(fonts) = self.fonts.as_ref() else {
            return;
        };

        let pal = state.theme;
        let w_dip = width as f32 / scale;
        let h_dip = height as f32 / scale;
        let header_h = HEADER_HEIGHT_DIP as f32;
        let footer_h = FOOTER_HEIGHT_DIP as f32;
        let row_h = ROW_HEIGHT_DIP as f32;
        let list_top = header_h + 4.0;
        let max_rows_by_h = ((h_dip - header_h - footer_h - 4.0).max(0.0) / row_h).floor() as usize;
        let visible_cap = state.max_visible_rows.max(1).min(max_rows_by_h.max(1));
        let visible_count = state.items.len().min(visible_cap);
        let has_preview = state.preview.is_some() && !state.items.is_empty() && w_dip >= 680.0;
        let preview_w = (state.preview_width_dip as f32).clamp(220.0, (w_dip * 0.48).max(220.0));
        let list_w_dip = if has_preview {
            (w_dip - preview_w).max(380.0)
        } else {
            w_dip
        };

        // ============================================================
        // PASS 1: Pure-Rust Anti-Aliased SDF Graphics + Modern White Emblem
        // ============================================================
        {
            let pixels = surface.pixels_mut();
            let mut canvas = Canvas {
                buf: pixels,
                w: width,
                h: height,
                scale,
            };

            canvas.clear(pal.bg);
            let fy0 = h_dip - footer_h;

            // Preview right pane background & vertical divider
            if has_preview {
                canvas.fill_rect_dip(list_w_dip, header_h, w_dip, fy0, pal.preview_bg);
                canvas.fill_rect_dip(list_w_dip, header_h, list_w_dip + 1.0, fy0, pal.divider);
            }

            // Footer strip
            canvas.fill_rect_dip(0.0, fy0, w_dip, h_dip, pal.footer_bg);
            canvas.fill_rect_dip(0.0, fy0, w_dip, fy0 + 1.0, pal.divider);

            // Header Modern White Porcelain Sculptural "T" Emblem (36x36 DIP)
            paint_modern_white_emblem(
                canvas.buf,
                width,
                height,
                (14.0 * scale).round() as i32,
                (13.0 * scale).round() as i32,
                (36.0 * scale).round() as i32,
            );

            // Mode badge pill & Super Match attached capsule on right side of header
            let mut header_right_x = w_dip - 16.0;
            if let Some(mode) = state.mode_badge {
                let pill_w = (mode.chars().count() as f32 * 9.5 + 22.0).clamp(58.0, 130.0);
                let px0 = header_right_x - pill_w;
                canvas.fill_rounded_rect(px0, 19.0, header_right_x, 43.0, 6.0, pal.pill_bg);
                canvas.stroke_rounded_rect(px0, 19.0, header_right_x, 43.0, 6.0, pal.accent);
                header_right_x = px0 - 8.0;
            }
            if let Some(cap) = state.attached_capsule {
                let cap_w = (cap.chars().count() as f32 * 8.5 + 24.0).clamp(96.0, 210.0);
                let cx0 = header_right_x - cap_w;
                canvas.fill_rounded_rect(cx0, 18.0, header_right_x, 44.0, 8.0, pal.card_sel);
                canvas.stroke_rounded_rect(cx0, 18.0, header_right_x, 44.0, 8.0, pal.accent);
            }

            // Preview image thumbnail (pure-Rust RGBA alpha blit into DIB)
            if has_preview {
                if let Some(prev) = state.preview {
                    if let Some((iw, ih, rgba)) = &prev.image_rgba {
                        let px0 = list_w_dip + 14.0;
                        let px1 = w_dip - 14.0;
                        let avail_w = (px1 - px0).max(40.0);
                        let avail_h = (fy0 - (header_h + 60.0) - 52.0).clamp(60.0, 210.0);
                        let img_scale = (avail_w / (*iw as f32)).min(avail_h / (*ih as f32)).min(1.0);
                        let dw = (*iw as f32 * img_scale).max(16.0);
                        let dh = (*ih as f32 * img_scale).max(16.0);
                        let dx = px0 + (avail_w - dw) * 0.5;
                        let dy = header_h + 56.0;
                        canvas.fill_rounded_rect(dx - 3.0, dy - 3.0, dx + dw + 3.0, dy + dh + 3.0, 6.0, pal.pill_bg);
                        canvas.draw_rgba_image(dx, dy, dw, dh, *iw, *ih, rgba);
                        canvas.stroke_rounded_rect(dx - 3.0, dy - 3.0, dx + dw + 3.0, dy + dh + 3.0, 6.0, pal.border);
                    }
                }
            }

            // Header bottom divider
            if !state.items.is_empty() {
                canvas.fill_rect_dip(12.0, header_h, w_dip - 12.0, header_h + 1.0, pal.divider);
            }

            // Result cards & badges
            for slot in 0..visible_count {
                let idx = state.scroll_offset + slot;
                let Some(item) = state.items.get(idx) else {
                    break;
                };
                let y0 = list_top + (slot as f32) * row_h;
                let y1 = y0 + row_h - 2.0;
                let is_sel = idx == state.selected;
                let is_hov = state.hovered == Some(idx) && !is_sel;

                if is_sel {
                    canvas.fill_rounded_rect(10.0, y0 + 2.0, list_w_dip - 12.0, y1 - 2.0, 8.0, pal.card_sel);
                    canvas.fill_rounded_rect(10.0, y0 + 10.0, 13.5, y1 - 10.0, 2.0, pal.accent);
                } else if is_hov {
                    canvas.fill_rounded_rect(10.0, y0 + 2.0, list_w_dip - 12.0, y1 - 2.0, 8.0, pal.card_hover);
                }

                if !matches!(item.icon, IconSpec::ShellPath(_)) {
                    canvas.fill_rounded_rect(
                        22.0,
                        y0 + 11.0,
                        52.0,
                        y0 + 41.0,
                        7.0,
                        category_rgb(item.category),
                    );
                }

                let mut text_right = list_w_dip - 20.0;
                if is_sel || slot < 8 {
                    text_right -= 48.0;
                }
                if let Some(badge) = &item.badge {
                    let bw = (badge.chars().count() as f32 * 6.5 + 16.0).clamp(42.0, 118.0);
                    let bx0 = text_right - bw;
                    canvas.fill_rounded_rect(bx0, y0 + 16.0, text_right, y0 + 36.0, 5.0, pal.pill_bg);
                }
            }

            // Crisp thin scrollbar thumb when total items exceed visible rows
            if state.items.len() > visible_cap && visible_cap > 0 {
                let track_top = list_top + 4.0;
                let track_h = (fy0 - track_top - 6.0).max(20.0);
                let ratio = (visible_cap as f32 / state.items.len() as f32).clamp(0.12, 1.0);
                let thumb_h = (track_h * ratio).max(24.0);
                let max_scroll = (state.items.len().saturating_sub(visible_cap)).max(1) as f32;
                let frac = (state.scroll_offset as f32 / max_scroll).clamp(0.0, 1.0);
                let ty0 = track_top + frac * (track_h - thumb_h);
                canvas.fill_rounded_rect(
                    list_w_dip - 7.0,
                    ty0,
                    list_w_dip - 3.0,
                    ty0 + thumb_h,
                    2.0,
                    pal.scroll_thumb,
                );
            }

            // Everything connection status dot in footer
            let dot_col = if state.everything_status.is_some() {
                Rgb(0x10, 0xB9, 0x81)
            } else {
                Rgb(0x6B, 0x72, 0x80)
            };
            canvas.fill_rounded_rect(14.0, fy0 + 11.0, 22.0, fy0 + 19.0, 4.0, dot_col);

            // Optional new-version update pill in footer
            if let Some(ver) = state.update_badge {
                let uw = (ver.chars().count() as f32 * 7.0 + 18.0).clamp(64.0, 120.0);
                let ux1 = w_dip - 184.0;
                let ux0 = ux1 - uw;
                if ux0 > 220.0 {
                    canvas.fill_rounded_rect(ux0, fy0 + 5.0, ux1, fy0 + 25.0, 5.0, pal.accent);
                }
            }

            // Action menu drawer background
            if state.action_menu_open {
                if let Some(sel_item) = state.items.get(state.selected) {
                    if !sel_item.actions.is_empty() {
                        let action_count = sel_item.actions.len().min(6);
                        let menu_w = 240.0;
                        let item_h = 30.0;
                        let menu_h = (action_count as f32) * item_h + 16.0;
                        let mx1 = list_w_dip - 14.0;
                        let mx0 = mx1 - menu_w;
                        let my1 = fy0 - 10.0;
                        let my0 = (my1 - menu_h).max(header_h + 8.0);

                        canvas.fill_rounded_rect(mx0, my0, mx1, my1, 8.0, pal.pop_bg);
                        canvas.stroke_rounded_rect(mx0, my0, mx1, my1, 8.0, pal.accent);
                        for ai in 0..action_count {
                            if ai == state.selected_action {
                                let ay0 = my0 + 8.0 + (ai as f32) * item_h;
                                let ay1 = ay0 + item_h - 2.0;
                                canvas.fill_rounded_rect(mx0 + 6.0, ay0, mx1 - 6.0, ay1, 5.0, pal.accent);
                            }
                        }
                    }
                }
            }

            // Crisp 1px outer border + subtle bottom-right resize grip indicator
            canvas.fill_rect_dip(0.0, 0.0, w_dip, 1.0, pal.border);
            canvas.fill_rect_dip(0.0, h_dip - 1.0, w_dip, h_dip, pal.border);
            canvas.fill_rect_dip(0.0, 0.0, 1.0, h_dip, pal.border);
            canvas.fill_rect_dip(w_dip - 1.0, 0.0, w_dip, h_dip, pal.border);
            for step in 0..3 {
                let off = 4.0 + (step as f32) * 3.5;
                canvas.fill_rect_dip(
                    w_dip - off - 1.5,
                    h_dip - 4.5,
                    w_dip - off,
                    h_dip - 3.0,
                    pal.text_muted,
                );
            }
        }

        // ============================================================
        // PASS 2: Native ClearType GDI Text & Real Shell Icons
        // ============================================================
        let dc = surface.mem_dc;
        unsafe {
            let _ = GdiFlush();

            // Header mode badge & Super Match attached capsule text
            let mut header_right_x = w_dip - 16.0;
            if let Some(mode) = state.mode_badge {
                let pill_w = (mode.chars().count() as f32 * 9.5 + 22.0).clamp(58.0, 130.0);
                let px0 = header_right_x - pill_w;
                draw_text_gdi(
                    dc,
                    fonts.badge,
                    pal.text_primary,
                    mode,
                    dip_rect(px0, 19.0, header_right_x, 43.0, scale),
                    DT_CENTER,
                );
                header_right_x = px0 - 8.0;
            }
            if let Some(cap) = state.attached_capsule {
                let cap_w = (cap.chars().count() as f32 * 8.5 + 24.0).clamp(96.0, 210.0);
                let cx0 = header_right_x - cap_w;
                draw_text_gdi(
                    dc,
                    fonts.badge,
                    pal.text_primary,
                    cap,
                    dip_rect(cx0 + 6.0, 18.0, header_right_x - 6.0, 44.0, scale),
                    DT_CENTER,
                );
            }

            // Result rows
            for slot in 0..visible_count {
                let idx = state.scroll_offset + slot;
                let Some(item) = state.items.get(idx) else {
                    break;
                };
                let y0 = list_top + (slot as f32) * row_h;
                let y1 = y0 + row_h - 2.0;
                let is_sel = idx == state.selected;

                let mut drew_shell_icon = false;
                if let IconSpec::ShellPath(path) = &item.icon {
                    if let Some(hicon) = self.icons.get_or_load(path) {
                        let ix = (22.0 * scale).round() as i32;
                        let iy = ((y0 + 11.0) * scale).round() as i32;
                        let isize = (30.0 * scale).round() as i32;
                        let _ = DrawIconEx(dc, ix, iy, hicon, isize, isize, 0, None, DI_NORMAL);
                        drew_shell_icon = true;
                    }
                }
                if !drew_shell_icon {
                    if matches!(item.icon, IconSpec::ShellPath(_)) {
                        let mut canvas = Canvas {
                            buf: surface.pixels_mut(),
                            w: width,
                            h: height,
                            scale,
                        };
                        canvas.fill_rounded_rect(
                            22.0,
                            y0 + 11.0,
                            52.0,
                            y0 + 41.0,
                            7.0,
                            category_rgb(item.category),
                        );
                        let _ = GdiFlush();
                    }
                    let label = match &item.icon {
                        IconSpec::Monogram(m) => m.as_str(),
                        IconSpec::Symbol(s) => s.as_str(),
                        _ => item.category.short_label(),
                    };
                    draw_text_gdi(
                        dc,
                        fonts.badge,
                        Rgb(0xFF, 0xFF, 0xFF),
                        label,
                        dip_rect(22.0, y0 + 11.0, 52.0, y0 + 41.0, scale),
                        DT_CENTER,
                    );
                }

                let mut text_right = list_w_dip - 20.0;
                let shortcut_text = if is_sel {
                    "Enter".to_string()
                } else if slot < 8 {
                    format!("Alt+{}", slot + 1)
                } else {
                    String::new()
                };
                if !shortcut_text.is_empty() {
                    let hint_w = 40.0;
                    draw_text_gdi(
                        dc,
                        fonts.footer,
                        pal.text_muted,
                        &shortcut_text,
                        dip_rect(text_right - hint_w, y0 + 8.0, text_right, y1 - 8.0, scale),
                        DT_RIGHT,
                    );
                    text_right -= hint_w + 8.0;
                }

                if let Some(badge) = &item.badge {
                    let bw = (badge.chars().count() as f32 * 6.5 + 16.0).clamp(42.0, 118.0);
                    let bx0 = text_right - bw;
                    draw_text_gdi(
                        dc,
                        fonts.badge,
                        pal.text_muted,
                        badge,
                        dip_rect(bx0, y0 + 16.0, text_right, y0 + 36.0, scale),
                        DT_CENTER,
                    );
                    text_right = bx0 - 10.0;
                }

                draw_text_gdi(
                    dc,
                    fonts.title,
                    pal.text_primary,
                    &item.title,
                    dip_rect(62.0, y0 + 5.0, text_right, y0 + 28.0, scale),
                    DT_LEFT,
                );
                draw_text_gdi(
                    dc,
                    fonts.subtitle,
                    pal.text_muted,
                    &item.subtitle,
                    dip_rect(62.0, y0 + 28.0, text_right, y0 + 47.0, scale),
                    DT_LEFT,
                );
            }

            // Right-Side Live Preview Pane Content
            if has_preview {
                if let Some(prev) = state.preview {
                    let px0 = list_w_dip + 14.0;
                    let px1 = w_dip - 14.0;
                    let fy0 = h_dip - footer_h;

                    draw_text_gdi(
                        dc,
                        fonts.title,
                        pal.text_primary,
                        &prev.title,
                        dip_rect(px0, header_h + 10.0, px1, header_h + 32.0, scale),
                        DT_LEFT,
                    );
                    draw_text_gdi(
                        dc,
                        fonts.footer,
                        pal.text_muted,
                        &prev.meta_line,
                        dip_rect(px0, header_h + 32.0, px1, header_h + 50.0, scale),
                        DT_LEFT,
                    );
                    let body_font = if prev.is_monospace {
                        fonts.preview_mono
                    } else {
                        fonts.preview_body
                    };
                    let body_top = if let Some((iw, ih, _)) = &prev.image_rgba {
                        let avail_w = (px1 - px0).max(40.0);
                        let avail_h = (fy0 - (header_h + 60.0) - 52.0).clamp(60.0, 210.0);
                        let img_scale = (avail_w / (*iw as f32)).min(avail_h / (*ih as f32)).min(1.0);
                        let dh = (*ih as f32 * img_scale).max(16.0);
                        header_h + 66.0 + dh
                    } else {
                        header_h + 58.0
                    };
                    draw_multiline_gdi(
                        dc,
                        body_font,
                        pal.text_primary,
                        &prev.body,
                        dip_rect(px0, body_top, px1, fy0 - 10.0, scale),
                    );
                }
            }

            // Action menu text
            if state.action_menu_open {
                if let Some(sel_item) = state.items.get(state.selected) {
                    if !sel_item.actions.is_empty() {
                        let action_count = sel_item.actions.len().min(6);
                        let menu_w = 240.0;
                        let item_h = 30.0;
                        let menu_h = (action_count as f32) * item_h + 16.0;
                        let mx1 = list_w_dip - 14.0;
                        let mx0 = mx1 - menu_w;
                        let my1 = h_dip - footer_h - 10.0;
                        let my0 = (my1 - menu_h).max(header_h + 8.0);

                        for (ai, act) in sel_item.actions.iter().take(action_count).enumerate() {
                            let ay0 = my0 + 8.0 + (ai as f32) * item_h;
                            let ay1 = ay0 + item_h - 2.0;
                            let col = if ai == state.selected_action {
                                pal.on_accent
                            } else {
                                pal.text_primary
                            };
                            draw_text_gdi(
                                dc,
                                fonts.footer,
                                col,
                                &act.label,
                                dip_rect(mx0 + 14.0, ay0, mx1 - 14.0, ay1, scale),
                                DT_LEFT,
                            );
                        }
                    }
                }
            }

            // Footer text + live RAM telemetry
            let fy0 = h_dip - footer_h;
            let left_text = if let Some(toast) = state.toast {
                toast.to_string()
            } else if state.action_menu_open {
                state
                    .lang
                    .pick(
                        "动作菜单：↑↓ 选择 · 回车执行 · Esc 返回",
                        "Actions: ↑↓ Select · Enter Execute · Esc Close",
                    )
                    .to_string()
            } else {
                let ev_label = state
                    .everything_status
                    .unwrap_or_else(|| state.lang.pick("Everything 未运行", "Everything offline"));
                match state.lang {
                    Lang::Zh => format!("{ev_label} · f 文件 · cb 剪贴板 · g 网页 · Ctrl+, 设置 · Ctrl+P 预览"),
                    Lang::En => format!("{ev_label} · f Files · cb Clip · g Web · Ctrl+, Settings · Ctrl+P Preview"),
                }
            };
            let left_right_bound = if let Some(ver) = state.update_badge {
                let uw = (ver.chars().count() as f32 * 7.0 + 18.0).clamp(64.0, 120.0);
                let ux1 = w_dip - 184.0;
                let ux0 = ux1 - uw;
                if ux0 > 220.0 {
                    draw_text_gdi(
                        dc,
                        fonts.badge,
                        pal.on_accent,
                        ver,
                        dip_rect(ux0, fy0 + 5.0, ux1, fy0 + 25.0, scale),
                        DT_CENTER,
                    );
                    ux0 - 10.0
                } else {
                    w_dip - 175.0
                }
            } else {
                w_dip - 175.0
            };

            draw_text_gdi(
                dc,
                fonts.footer,
                pal.text_muted,
                &left_text,
                dip_rect(28.0, fy0, left_right_bound, h_dip, scale),
                DT_LEFT,
            );

            let stats = current_process_stats();
            let mem_text = match state.lang {
                Lang::Zh => format!("{} 项 · 内存 {:.1} MB", state.items.len(), stats.working_set_mb()),
                Lang::En => format!("{} hits · RAM {:.1} MB", state.items.len(), stats.working_set_mb()),
            };
            draw_text_gdi(
                dc,
                fonts.footer,
                pal.text_muted,
                &mem_text,
                dip_rect(w_dip - 180.0, fy0, w_dip - 14.0, h_dip, scale),
                DT_RIGHT,
            );

            let _ = BitBlt(screen_hdc, 0, 0, width, height, Some(dc), 0, 0, SRCCOPY);
        }
    }
}

// --- Zero-dependency Pure-Rust SDF Anti-Aliased Software Rasterizer ---

struct Canvas<'a> {
    buf: &'a mut [u8],
    w: i32,
    h: i32,
    scale: f32,
}

impl<'a> Canvas<'a> {
    fn clear(&mut self, c: Rgb) {
        let px = [c.2, c.1, c.0, 255];
        for chunk in self.buf.chunks_exact_mut(4) {
            chunk.copy_from_slice(&px);
        }
    }

    fn fill_rect_dip(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb) {
        let ix0 = (x0 * self.scale).floor().max(0.0) as i32;
        let iy0 = (y0 * self.scale).floor().max(0.0) as i32;
        let ix1 = (x1 * self.scale).ceil().min(self.w as f32) as i32;
        let iy1 = (y1 * self.scale).ceil().min(self.h as f32) as i32;
        let px = [c.2, c.1, c.0, 255];
        for y in iy0..iy1 {
            let row = (y * self.w) as usize * 4;
            for x in ix0..ix1 {
                let off = row + (x as usize) * 4;
                self.buf[off..off + 4].copy_from_slice(&px);
            }
        }
    }

    fn fill_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r_dip: f32, c: Rgb) {
        let px0 = x0 * self.scale;
        let py0 = y0 * self.scale;
        let px1 = x1 * self.scale;
        let py1 = y1 * self.scale;
        let r = (r_dip * self.scale)
            .min((px1 - px0) * 0.5)
            .min((py1 - py0) * 0.5);

        let cx = (px0 + px1) * 0.5;
        let cy = (py0 + py1) * 0.5;
        let hx = (px1 - px0) * 0.5 - r;
        let hy = (py1 - py0) * 0.5 - r;

        let ix0 = (px0.floor() as i32 - 1).max(0);
        let iy0 = (py0.floor() as i32 - 1).max(0);
        let ix1 = (px1.ceil() as i32 + 1).min(self.w);
        let iy1 = (py1.ceil() as i32 + 1).min(self.h);

        for y in iy0..iy1 {
            let dy = (y as f32 + 0.5 - cy).abs() - hy;
            let row = (y * self.w) as usize * 4;
            for x in ix0..ix1 {
                let dx = (x as f32 + 0.5 - cx).abs() - hx;
                let dist =
                    (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - r;
                let alpha = (0.5 - dist).clamp(0.0, 1.0);
                if alpha > 0.0 {
                    blend_pixel(&mut self.buf[row + (x as usize) * 4..], c, alpha);
                }
            }
        }
    }

    fn stroke_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r_dip: f32, c: Rgb) {
        let px0 = x0 * self.scale;
        let py0 = y0 * self.scale;
        let px1 = x1 * self.scale;
        let py1 = y1 * self.scale;
        let r = (r_dip * self.scale)
            .min((px1 - px0) * 0.5)
            .min((py1 - py0) * 0.5);
        let half_stroke = 0.65 * self.scale;

        let cx = (px0 + px1) * 0.5;
        let cy = (py0 + py1) * 0.5;
        let hx = (px1 - px0) * 0.5 - r;
        let hy = (py1 - py0) * 0.5 - r;

        let ix0 = (px0.floor() as i32 - 2).max(0);
        let iy0 = (py0.floor() as i32 - 2).max(0);
        let ix1 = (px1.ceil() as i32 + 2).min(self.w);
        let iy1 = (py1.ceil() as i32 + 2).min(self.h);

        for y in iy0..iy1 {
            let dy = (y as f32 + 0.5 - cy).abs() - hy;
            let row = (y * self.w) as usize * 4;
            for x in ix0..ix1 {
                let dx = (x as f32 + 0.5 - cx).abs() - hx;
                let dist =
                    (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - r;
                let edge_dist = dist.abs() - half_stroke;
                let alpha = (0.5 - edge_dist).clamp(0.0, 1.0);
                if alpha > 0.0 {
                    blend_pixel(&mut self.buf[row + (x as usize) * 4..], c, alpha);
                }
            }
        }
    }

    fn draw_rgba_image(
        &mut self,
        x0_dip: f32,
        y0_dip: f32,
        w_dip: f32,
        h_dip: f32,
        src_w: u32,
        src_h: u32,
        rgba: &[u8],
    ) {
        if src_w == 0 || src_h == 0 || rgba.len() < (src_w as usize * src_h as usize * 4) {
            return;
        }
        let px0 = (x0_dip * self.scale).round() as i32;
        let py0 = (y0_dip * self.scale).round() as i32;
        let pw = (w_dip * self.scale).round().max(1.0) as i32;
        let ph = (h_dip * self.scale).round().max(1.0) as i32;

        let x_start = px0.max(0);
        let y_start = py0.max(0);
        let x_end = (px0 + pw).min(self.w);
        let y_end = (py0 + ph).min(self.h);

        for y in y_start..y_end {
            let sy = (((y - py0) as u64 * src_h as u64) / ph as u64).min(src_h as u64 - 1) as usize;
            let row = (y * self.w) as usize * 4;
            for x in x_start..x_end {
                let sx =
                    (((x - px0) as u64 * src_w as u64) / pw as u64).min(src_w as u64 - 1) as usize;
                let si = (sy * src_w as usize + sx) * 4;
                let a = rgba[si + 3] as f32 / 255.0;
                if a > 0.0 {
                    blend_pixel(
                        &mut self.buf[row + (x as usize) * 4..],
                        Rgb(rgba[si], rgba[si + 1], rgba[si + 2]),
                        a,
                    );
                }
            }
        }
    }
}

#[inline(always)]
fn blend_pixel(dst: &mut [u8], src: Rgb, alpha: f32) {
    if alpha >= 0.999 {
        dst[0] = src.2;
        dst[1] = src.1;
        dst[2] = src.0;
        dst[3] = 255;
        return;
    }
    let inv = 1.0 - alpha;
    dst[0] = (src.2 as f32 * alpha + dst[0] as f32 * inv) as u8;
    dst[1] = (src.1 as f32 * alpha + dst[1] as f32 * inv) as u8;
    dst[2] = (src.0 as f32 * alpha + dst[2] as f32 * inv) as u8;
    dst[3] = 255;
}

fn make_gdi_font(px_height: f32, weight: i32, face: &str) -> HFONT {
    let face_w: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateFontW(
            -px_height.round() as i32,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(face_w.as_ptr()),
        )
    }
}

unsafe fn draw_text_gdi(
    dc: HDC,
    font: HFONT,
    color: Rgb,
    text: &str,
    mut rect: RECT,
    align: DRAW_TEXT_FORMAT,
) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return;
    }
    unsafe {
        let old = SelectObject(dc, font.into());
        SetTextColor(dc, color.to_colorref());
        let _ = DrawTextW(
            dc,
            &mut wide,
            &mut rect,
            align | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        let _ = SelectObject(dc, old);
    }
}

unsafe fn draw_multiline_gdi(dc: HDC, font: HFONT, color: Rgb, text: &str, mut rect: RECT) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return;
    }
    unsafe {
        let old = SelectObject(dc, font.into());
        SetTextColor(dc, color.to_colorref());
        let _ = DrawTextW(
            dc,
            &mut wide,
            &mut rect,
            DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        let _ = SelectObject(dc, old);
    }
}

fn dip_rect(l: f32, t: f32, r: f32, b: f32, scale: f32) -> RECT {
    RECT {
        left: (l * scale).round() as i32,
        top: (t * scale).round() as i32,
        right: (r * scale).round() as i32,
        bottom: (b * scale).round() as i32,
    }
}

fn category_rgb(cat: ResultCategory) -> Rgb {
    match cat {
        ResultCategory::App => Rgb(0x3B, 0x82, 0xF6),
        ResultCategory::File => Rgb(0x10, 0xB9, 0x81),
        ResultCategory::Folder => Rgb(0x05, 0x96, 0x69),
        ResultCategory::Calc => Rgb(0xF5, 0x9E, 0x0B),
        ResultCategory::Clipboard => Rgb(0x0E, 0xA5, 0xE9),
        ResultCategory::Window => Rgb(0x06, 0xB6, 0xD4),
        ResultCategory::System => Rgb(0xEC, 0x48, 0x99),
        ResultCategory::Plugin => Rgb(0x8B, 0x5C, 0xF6),
        ResultCategory::Web => Rgb(0x63, 0x66, 0xF1),
    }
}
