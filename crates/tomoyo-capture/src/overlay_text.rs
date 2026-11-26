//! 2x supersampled pure-alpha crisp text rasterizer (Alibaba PuHuiTi 3.0 65 Medium, zero ClearType
//! RGB color fringing, pixel-identical across live overlay & PNG export) + HUD GDI label renderer.

#![cfg(windows)]

use crate::annotate::{text_line_height, AnnotColor, Annotation};
use crate::overlay_draw::{
    dim_badge_rect, loupe_rect, snap_badge_rect_avoiding, tooltip_rect_for, wheel_hud_rect,
    ToolbarCmd, ToolbarLayout,
};
use crate::overlay_icons::blend_px;
use crate::screen::{DesktopSnapshot, ScreenRect};
use crate::snap_detect::SnapCandidate;
use std::collections::HashMap;
use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    GdiFlush, GetTextExtentPoint32W, SelectObject, SetBkMode, SetTextColor, TextOutW,
    ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
    FF_DONTCARE, FW_BOLD, FW_MEDIUM, FW_NORMAL, HDC, HFONT, OUT_DEFAULT_PRECIS, TRANSPARENT,
};

#[derive(Clone)]
pub struct CachedTextMask {
    pub w: i32,
    pub h: i32,
    pub alpha: Vec<u8>,
}

#[derive(Default)]
pub struct TextMaskCache {
    entries: HashMap<(String, i32, bool), CachedTextMask>,
}

impl TextMaskCache {
    pub fn get_or_rasterize(
        &mut self,
        text: &str,
        size: i32,
        bold: bool,
        font_face: &str,
    ) -> Option<&CachedTextMask> {
        let key = (text.to_string(), size.clamp(12, 64), bold);
        if !self.entries.contains_key(&key) {
            if self.entries.len() >= 64 {
                self.entries.clear();
            }
            let mask = rasterize_text_mask_2x(text, key.1, bold, font_face)?;
            self.entries.insert(key.clone(), mask);
        }
        self.entries.get(&key)
    }
}

pub(crate) fn make_2x_text_font(size_px: i32, bold: bool, font_face: &str) -> HFONT {
    let weight = if bold {
        FW_BOLD.0 as i32
    } else if font_face.contains("65 Medium") {
        FW_NORMAL.0 as i32
    } else {
        FW_MEDIUM.0 as i32
    };
    let face: Vec<u16> = font_face.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateFontW(
            -(size_px.clamp(12, 64) * 2),
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
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

/// Measures exact logical pixel width of the first `char_count` characters of a single line,
/// matching the 2x supersampled rasterizer to sub-pixel precision.
pub fn measure_line_prefix_px(
    line: &str,
    char_count: usize,
    size_px: i32,
    bold: bool,
    font_face: &str,
) -> i32 {
    if char_count == 0 || line.is_empty() {
        return 0;
    }
    let prefix: String = line.chars().take(char_count).collect();
    let wide: Vec<u16> = prefix.encode_utf16().collect();
    if wide.is_empty() {
        return 0;
    }
    unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return 0;
        }
        let font = make_2x_text_font(size_px, bold, font_face);
        let old = SelectObject(dc, font.into());
        let mut sz = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &wide, &mut sz);
        let _ = SelectObject(dc, old);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(dc);
        (sz.cx + 1) / 2
    }
}

/// Measures multi-line text logical bounding `(width_px, height_px)`.
pub fn measure_multiline_px(text: &str, size_px: i32, bold: bool, font_face: &str) -> (i32, i32) {
    let lh = text_line_height(size_px);
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let mut max_w = 0i32;
    unsafe {
        let dc = CreateCompatibleDC(None);
        if !dc.is_invalid() {
            let font = make_2x_text_font(size_px, bold, font_face);
            let old = SelectObject(dc, font.into());
            for line in &lines {
                if line.is_empty() {
                    continue;
                }
                let wide: Vec<u16> = line.encode_utf16().collect();
                let mut sz = SIZE::default();
                if GetTextExtentPoint32W(dc, &wide, &mut sz).as_bool() {
                    max_w = max_w.max((sz.cx + 1) / 2);
                }
            }
            let _ = SelectObject(dc, old);
            let _ = DeleteObject(font.into());
            let _ = DeleteDC(dc);
        }
    }
    (max_w.min(3600), (lines.len().max(1) as i32 * lh).min(2000))
}

#[inline]
fn crisp_coverage_u8(cov: u32) -> u8 {
    if cov == 0 {
        return 0;
    }
    if cov >= 255 {
        return 255;
    }
    let f = (cov as f32 / 255.0).powf(0.84);
    (f * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// Rasterizes multi-line text at 2x scale using pure grayscale `ANTIALIASED_QUALITY` (zero RGB
/// subpixel color fringes) and box-downsamples into a sharp 1x alpha coverage mask (0 GDI leaks).
pub fn rasterize_text_mask_2x(
    text: &str,
    size_px: i32,
    bold: bool,
    font_face: &str,
) -> Option<CachedTextMask> {
    if text.is_empty() {
        return None;
    }
    let s = size_px.clamp(12, 64);
    let lh = text_line_height(s);
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();

    unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let font = make_2x_text_font(s, bold, font_face);
        let old_font = SelectObject(dc, font.into());

        let mut max_w2 = 0i32;
        let mut wide_lines: Vec<Vec<u16>> = Vec::with_capacity(lines.len());
        for line in &lines {
            let wv: Vec<u16> = line.encode_utf16().collect();
            if !wv.is_empty() {
                let mut sz = SIZE::default();
                if GetTextExtentPoint32W(dc, &wv, &mut sz).as_bool() {
                    max_w2 = max_w2.max(sz.cx);
                }
            }
            wide_lines.push(wv);
        }
        let box_w = ((max_w2 + 1) / 2 + 6).clamp(1, 3600);
        let box_h = (lines.len().max(1) as i32 * lh + 4).clamp(1, 2000);
        let (w2, h2) = (box_w * 2, box_h * 2);

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w2,
                biHeight: -h2,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let mask = if let Ok(bmp) = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            let old_bmp = SelectObject(dc, bmp.into());
            let slice = std::slice::from_raw_parts_mut(bits as *mut u8, (w2 as usize) * (h2 as usize) * 4);
            slice.fill(0);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0x00FF_FFFF));
            for (li, wv) in wide_lines.iter().enumerate() {
                if !wv.is_empty() {
                    let _ = TextOutW(dc, 2, (li as i32) * lh * 2 + 2, wv);
                }
            }
            let _ = GdiFlush();

            let mut alpha = vec![0u8; (box_w as usize) * (box_h as usize)];
            let stride2 = (w2 as usize) * 4;
            for py in 0..box_h as usize {
                let r0 = (py * 2) * stride2;
                let r1 = (py * 2 + 1) * stride2;
                let dst_row = py * (box_w as usize);
                for px in 0..box_w as usize {
                    let c0 = px * 8;
                    let sum = slice[r0 + c0 + 1] as u32
                        + slice[r0 + c0 + 5] as u32
                        + slice[r1 + c0 + 1] as u32
                        + slice[r1 + c0 + 5] as u32;
                    alpha[dst_row + px] = crisp_coverage_u8((sum + 2) >> 2);
                }
            }
            let _ = SelectObject(dc, old_bmp);
            let _ = DeleteObject(bmp.into());
            Some(CachedTextMask { w: box_w, h: box_h, alpha })
        } else {
            None
        };

        let _ = SelectObject(dc, old_font);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(dc);
        mask
    }
}

/// Blits a supersampled alpha mask onto a BGRA buffer with adaptive soft contrast shadow.
pub fn blit_text_mask_bgra(
    bgra: &mut [u8],
    dst_w: i32,
    dst_h: i32,
    x: i32,
    y: i32,
    mask: &CachedTextMask,
    color: AnnotColor,
) {
    let lum = (77u32 * color.0 as u32 + 150 * color.1 as u32 + 29 * color.2 as u32) >> 8;
    let is_dark_text = lum < 72;
    let shadow = if is_dark_text { AnnotColor(0xFF, 0xFF, 0xFF) } else { AnnotColor(0x0A, 0x0E, 0x16) };
    let (sh_a1, sh_a2) = if is_dark_text { (0.42f32, 0.22f32) } else { (0.46f32, 0.20f32) };

    let bw = mask.w as usize;
    for py in 0..mask.h {
        let dy = y + py;
        if dy < -2 || dy >= dst_h + 2 {
            continue;
        }
        let row = (py as usize) * bw;
        for px in 0..mask.w {
            let a_u8 = mask.alpha[row + px as usize];
            if a_u8 == 0 {
                continue;
            }
            let a = a_u8 as f32 / 255.0;
            let dx = x + px;
            blend_px(bgra, dst_w, dst_h, dx + 1, dy + 1, shadow, a * sh_a1);
            blend_px(bgra, dst_w, dst_h, dx, dy + 1, shadow, a * sh_a2);
        }
    }
    for py in 0..mask.h {
        let dy = y + py;
        if dy < 0 || dy >= dst_h {
            continue;
        }
        let row = (py as usize) * bw;
        for px in 0..mask.w {
            let a_u8 = mask.alpha[row + px as usize];
            if a_u8 == 0 {
                continue;
            }
            blend_px(bgra, dst_w, dst_h, x + px, dy, color, a_u8 as f32 / 255.0);
        }
    }
}

/// Renders all committed `Annotation::Text` entries directly into a BGRA buffer (live & export).
#[allow(clippy::too_many_arguments)]
pub fn render_text_annotations_bgra(
    bgra: &mut [u8],
    width: i32,
    height: i32,
    offset_x: i32,
    offset_y: i32,
    annotations: &[Annotation],
    font_face: &str,
    mut cache: Option<&mut TextMaskCache>,
) {
    for ann in annotations {
        let Annotation::Text { x, y, text, size, color, bold } = ann else { continue };
        if text.trim().is_empty() {
            continue;
        }
        let (tx, ty) = (x - offset_x - 1, y - offset_y - 1);
        if let Some(c) = cache.as_deref_mut() {
            if let Some(mask) = c.get_or_rasterize(text, *size, *bold, font_face) {
                blit_text_mask_bgra(bgra, width, height, tx, ty, mask, *color);
            }
        } else if let Some(mask) = rasterize_text_mask_2x(text, *size, *bold, font_face) {
            blit_text_mask_bgra(bgra, width, height, tx, ty, &mask, *color);
        }
    }
}

/// Bakes `Annotation::Text` entries into a cropped BGRA buffer with exact 2x supersampling.
pub fn bake_text_annotations_into_bgra(
    bgra: &mut [u8],
    width: i32,
    height: i32,
    offset_x: i32,
    offset_y: i32,
    annotations: &[Annotation],
) {
    if !annotations.iter().any(|a| matches!(a, Annotation::Text { text, .. } if !text.trim().is_empty())) {
        return;
    }
    let fonts = tomoyo_core::PrivateFonts::load_default();
    render_text_annotations_bgra(bgra, width, height, offset_x, offset_y, annotations, fonts.medium_family(), None);
}

#[allow(clippy::too_many_arguments)]
pub fn paint_overlay_gdi_labels(
    mem_dc: HDC,
    font: HFONT,
    snap: &DesktopSnapshot,
    sel: Option<ScreenRect>,
    hover_snap: Option<SnapCandidate>,
    hover_visual: Option<ScreenRect>,
    cycle_count: usize,
    cursor: (i32, i32),
    show_loupe: bool,
    layout: &ToolbarLayout,
    hovered_cmd: Option<ToolbarCmd>,
    hex_lower: bool,
    rgb_mode: bool,
    wheel_hud: Option<&str>,
) {
    unsafe {
        let old = SelectObject(mem_dc, font.into());
        SetBkMode(mem_dc, TRANSPARENT);
        for btn in &layout.hits {
            if btn.pill_text.is_empty() { continue; }
            let col = if btn.danger { 0x00FF_FFFF } else if btn.active { 0x00FA_A560 } else { 0x00E2_E8F0 };
            SetTextColor(mem_dc, COLORREF(col));
            let mut rc = RECT { left: btn.rect.x, top: btn.rect.y, right: btn.rect.right(), bottom: btn.rect.bottom() };
            let mut w: Vec<u16> = btn.pill_text.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        if let Some(s) = sel.filter(|r| r.is_valid()) {
            let b = dim_badge_rect(s, snap.width);
            let mut rc = RECT { left: b.x, top: b.y, right: b.right(), bottom: b.bottom() };
            SetTextColor(mem_dc, COLORREF(0x00F8_FAFC));
            let rec_open = layout.hits.iter().any(|h| h.cmd == ToolbarCmd::RecStart);
            let badge_txt = if rec_open { format!("录制 {} × {}", s.w, s.h) } else { format!("{} × {}", s.w, s.h) };
            let mut w: Vec<u16> = badge_txt.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        } else if let Some(h) = hover_snap {
            let vis_r = hover_visual.unwrap_or(h.rect);
            let avoid = show_loupe.then(|| loupe_rect(cursor, snap.width, snap.height));
            let b = snap_badge_rect_avoiding(vis_r, snap.width, snap.height, avoid);
            let mut rc = RECT { left: b.x, top: b.y, right: b.right(), bottom: b.bottom() };
            SetTextColor(mem_dc, COLORREF(0x00FE_F2E0));
            let txt = if cycle_count <= 1 {
                format!("{} {}×{} · 拖拽/单击 · Alt免吸附", h.kind.label(), h.rect.w, h.rect.h)
            } else {
                format!("{} {}×{} · 单击 · Tab切层 · Alt免", h.kind.label(), h.rect.w, h.rect.h)
            };
            let mut w: Vec<u16> = txt.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        if let Some(cmd) = hovered_cmd
            && let Some(btn) = layout.hits.iter().find(|b| b.cmd == cmd)
            && let Some(tr) = tooltip_rect_for(btn, snap.width, snap.height)
        {
            let mut rc = RECT { left: tr.x, top: tr.y, right: tr.right(), bottom: tr.bottom() };
            SetTextColor(mem_dc, COLORREF(0x00F8_FAFC));
            let mut w: Vec<u16> = btn.tooltip.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        if let Some(txt) = wheel_hud {
            let r = wheel_hud_rect(cursor, snap.width, snap.height);
            let mut rc = RECT { left: r.x, top: r.y, right: r.right(), bottom: r.bottom() };
            SetTextColor(mem_dc, COLORREF(0x00FA_A560));
            let mut w: Vec<u16> = txt.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        if show_loupe {
            let lr = loupe_rect(cursor, snap.width, snap.height);
            let (r, g, b) = snap.pixel_rgb(cursor.0, cursor.1);
            let readout = if rgb_mode { format!("{r},{g},{b}") }
                else if hex_lower { format!("#{r:02x}{g:02x}{b:02x}") }
                else { format!("#{r:02X}{g:02X}{b:02X}") };
            let mut rc1 = RECT { left: lr.x + 33, top: lr.y + 148, right: lr.right() - 8, bottom: lr.y + 166 };
            SetTextColor(mem_dc, COLORREF(0x00F8_FAFC));
            let mut w1: Vec<u16> = format!("{readout}  {},{}", cursor.0, cursor.1).encode_utf16().collect();
            DrawTextW(mem_dc, &mut w1, &mut rc1, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            let mut rc2 = RECT { left: lr.x + 8, top: lr.y + 168, right: lr.right() - 8, bottom: lr.bottom() - 4 };
            SetTextColor(mem_dc, COLORREF(0x0094_A3B8));
            let mut w2: Vec<u16> = "C 复制 · Shift 切换".encode_utf16().collect();
            DrawTextW(mem_dc, &mut w2, &mut rc2, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        let _ = SelectObject(mem_dc, old);
    }
}
