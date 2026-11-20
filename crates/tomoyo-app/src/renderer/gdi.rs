//! RAII GDI DIB surface, ClearType FontPack & text drawing helpers (strict zero-handle-leak Drop).

use super::theme::Rgb;
use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DRAW_TEXT_FORMAT,
    DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_TOP, DT_VCENTER, DT_WORDBREAK,
    FF_DONTCARE, FW_NORMAL, HBITMAP, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, TRANSPARENT,
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
