//! RAII GDI DIB surface, ClearType FontPack & text drawing helpers (strict zero-handle-leak Drop).

use super::theme::Rgb;
use std::ffi::c_void;
use tomoyo_core::TypographyConfig;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    GetTextExtentPoint32W, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DRAW_TEXT_FORMAT, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_TOP, DT_VCENTER,
    DT_WORDBREAK, FF_DONTCARE, FW_NORMAL, HBITMAP, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS,
    TRANSPARENT,
};

pub struct DibSurface {
    pub mem_dc: HDC,
    pub bitmap: HBITMAP,
    pub old_obj: HGDIOBJ,
    bits: *mut u8,
    pub width: i32,
    pub height: i32,
}

impl DibSurface {
    pub fn create(ref_dc: HDC, width: i32, height: i32) -> Option<Self> {
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

    pub fn pixels_mut(&mut self) -> &mut [u8] {
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

#[derive(Clone, PartialEq, Eq)]
pub struct FontKey {
    pub dpi_key: i32,
    pub font_family: String,
    pub title_weight: u16,
    pub subtitle_weight: u16,
    pub title_size: u8,
}

impl FontKey {
    pub fn from_config(dpi_scale: f32, typo: &TypographyConfig) -> Self {
        Self {
            dpi_key: (dpi_scale * 100.0).round() as i32,
            font_family: typo.font_family.clone(),
            title_weight: typo.title_weight,
            subtitle_weight: typo.subtitle_weight,
            title_size: typo.title_size.clamp(14, 18),
        }
    }
}

pub struct FontPack {
    pub key: FontKey,
    pub title: HFONT,
    pub subtitle: HFONT,
    pub chip: HFONT,
    pub chip_active: HFONT,
    pub badge: HFONT,
    pub footer: HFONT,
    pub preview_body: HFONT,
    pub preview_mono: HFONT,
}

pub fn resolve_face_and_weight<'a>(
    custom_family: &'a str,
    requested_weight: u16,
    regular_face: &'a str,
    medium_face: &'a str,
) -> (&'a str, i32) {
    if !custom_family.is_empty() {
        return (custom_family, requested_weight as i32);
    }
    match requested_weight {
        0..=449 => (regular_face, requested_weight as i32),
        // Alibaba PuHuiTi 3.0 65 Medium is registered as a standalone face whose natural outline
        // is already weight 500; passing FW_NORMAL avoids GDI faux-bold horizontal double-striking.
        450..=549 => (medium_face, FW_NORMAL.0 as i32),
        _ => (medium_face, requested_weight as i32),
    }
}

impl FontPack {
    pub fn create(
        dpi_scale: f32,
        regular_face: &str,
        medium_face: &str,
        typo: &TypographyConfig,
    ) -> Self {
        let key = FontKey::from_config(dpi_scale, typo);
        let title_px = key.title_size as f32 * dpi_scale;
        let sub_px = typo.subtitle_size_dip() * dpi_scale;
        let chip_px = sub_px.clamp(11.5 * dpi_scale, 13.5 * dpi_scale);
        let (title_face, title_w) =
            resolve_face_and_weight(&typo.font_family, key.title_weight, regular_face, medium_face);
        let (sub_face, sub_w) =
            resolve_face_and_weight(&typo.font_family, key.subtitle_weight, regular_face, medium_face);
        let (med_face, med_w) =
            resolve_face_and_weight(&typo.font_family, 500, regular_face, medium_face);
        let (ui_face, ui_w) =
            resolve_face_and_weight(&typo.font_family, 400, regular_face, medium_face);
        Self {
            key,
            title: make_gdi_font(title_px, title_w, title_face),
            subtitle: make_gdi_font(sub_px, sub_w, sub_face),
            chip: make_gdi_font(chip_px, sub_w.min(500), sub_face),
            chip_active: make_gdi_font(chip_px, med_w, med_face),
            badge: make_gdi_font(11.5 * dpi_scale, med_w, med_face),
            footer: make_gdi_font(12.0 * dpi_scale, ui_w, ui_face),
            preview_body: make_gdi_font(sub_px.max(12.5 * dpi_scale), ui_w, ui_face),
            preview_mono: make_gdi_font(12.5 * dpi_scale, FW_NORMAL.0 as i32, "Consolas"),
        }
    }
}

impl Drop for FontPack {
    fn drop(&mut self) {
        unsafe {
            for f in [
                self.title,
                self.subtitle,
                self.chip,
                self.chip_active,
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

pub unsafe fn draw_text_gdi(
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

pub unsafe fn draw_text_highlighted_gdi(
    dc: HDC,
    font: HFONT,
    base_color: Rgb,
    highlight_color: Rgb,
    text: &str,
    spans: &[(usize, usize)],
    rect: RECT,
) {
    if spans.is_empty() {
        unsafe {
            draw_text_gdi(dc, font, base_color, text, rect, DT_LEFT);
        }
        return;
    }

    // Build contiguous (slice, is_highlighted) segments using validated char boundaries.
    let mut runs: Vec<(&str, bool)> = Vec::with_capacity(spans.len() * 2 + 1);
    let mut pos = 0usize;
    for &(s, e) in spans {
        if s > pos && s <= text.len() && text.is_char_boundary(s) {
            runs.push((&text[pos..s], false));
        }
        if s < e && e <= text.len() && text.is_char_boundary(s) && text.is_char_boundary(e) {
            runs.push((&text[s..e], true));
            pos = e;
        }
    }
    if pos < text.len() && text.is_char_boundary(pos) {
        runs.push((&text[pos..], false));
    }
    if runs.is_empty() {
        return;
    }

    unsafe {
        let old = SelectObject(dc, font.into());
        let mut cursor_x = rect.left;
        for (idx, (seg, is_hl)) in runs.iter().enumerate() {
            if cursor_x >= rect.right {
                break;
            }
            let mut wide: Vec<u16> = seg.encode_utf16().collect();
            if wide.is_empty() {
                continue;
            }
            let mut sz = SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &wide, &mut sz);
            let is_last = idx + 1 == runs.len();
            let overflows = cursor_x + sz.cx > rect.right;
            let mut seg_rc = RECT {
                left: cursor_x,
                top: rect.top,
                right: if is_last || overflows {
                    rect.right
                } else {
                    (cursor_x + sz.cx).min(rect.right)
                },
                bottom: rect.bottom,
            };
            SetTextColor(
                dc,
                if *is_hl { highlight_color } else { base_color }.to_colorref(),
            );
            let flags = if is_last || overflows {
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX
            } else {
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX
            };
            let _ = DrawTextW(dc, &mut wide, &mut seg_rc, flags);
            cursor_x += sz.cx;
            if overflows {
                break;
            }
        }
        let _ = SelectObject(dc, old);
    }
}

pub unsafe fn draw_multiline_gdi(dc: HDC, font: HFONT, color: Rgb, text: &str, mut rect: RECT) {
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

pub fn dip_rect(l: f32, t: f32, r: f32, b: f32, scale: f32) -> RECT {
    RECT {
        left: (l * scale).round() as i32,
        top: (t * scale).round() as i32,
        right: (r * scale).round() as i32,
        bottom: (b * scale).round() as i32,
    }
}
