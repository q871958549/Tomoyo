//! RAII GDI DIB surface, ClearType FontPack & text drawing helpers (strict zero-handle-leak Drop).

use super::theme::Rgb;
use std::ffi::c_void;
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

pub struct FontPack {
    pub dpi_key: i32,
    pub title: HFONT,
    pub subtitle: HFONT,
    pub chip: HFONT,
    pub chip_active: HFONT,
    pub badge: HFONT,
    pub footer: HFONT,
    pub preview_body: HFONT,
    pub preview_mono: HFONT,
}

impl FontPack {
    pub fn create(dpi_scale: f32, regular_face: &str, medium_face: &str) -> Self {
        let dpi_key = (dpi_scale * 100.0).round() as i32;
        // Always pass FW_NORMAL (400) so GDI uses the native outlines of `55 Regular`
        // and `65 Medium` without algorithmic faux-bold horizontal double-striking.
        let normal = FW_NORMAL.0 as i32;
        Self {
            dpi_key,
            title: make_gdi_font(15.5 * dpi_scale, normal, medium_face),
            subtitle: make_gdi_font(12.5 * dpi_scale, normal, regular_face),
            chip: make_gdi_font(12.5 * dpi_scale, normal, regular_face),
            chip_active: make_gdi_font(12.5 * dpi_scale, normal, medium_face),
            badge: make_gdi_font(11.5 * dpi_scale, normal, medium_face),
            footer: make_gdi_font(12.0 * dpi_scale, normal, regular_face),
            preview_body: make_gdi_font(12.5 * dpi_scale, normal, regular_face),
            preview_mono: make_gdi_font(12.0 * dpi_scale, normal, "Consolas"),
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
