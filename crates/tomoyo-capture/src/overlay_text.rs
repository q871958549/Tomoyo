//! ClearType GDI text annotation rasterizer (live overlay + final BGRA export) and
//! borderless native Win32 inline EDIT box (full Chinese IME support) for `tomoyo-capture`.

#![cfg(windows)]

use crate::annotate::{AnnotColor, Annotation};
use crate::overlay_draw::{
    dim_badge_rect, loupe_rect, snap_badge_rect, tooltip_rect_for, wheel_hud_rect, ToolbarCmd,
    ToolbarLayout,
};
use crate::screen::{DesktopSnapshot, ScreenRect};
use crate::snap_detect::SnapCandidate;
use std::ffi::c_void;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    GdiFlush, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DT_CENTER, DT_LEFT, DT_NOCLIP, DT_SINGLELINE, DT_TOP, DT_VCENTER, FF_DONTCARE, FW_NORMAL, HDC,
    HFONT, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetWindowTextLengthW, GetWindowTextW, SendMessageW,
    ES_AUTOHSCROLL, WM_SETFONT, WS_BORDER, WS_CHILD, WS_VISIBLE,
};

fn make_text_font(size_px: i32) -> HFONT {
    let face: Vec<u16> = "Microsoft YaHei UI".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateFontW(
            -size_px.clamp(12, 56), 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0,
            DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, PCWSTR(face.as_ptr()),
        )
    }
}

pub fn render_text_annotations_gdi(
    hdc: HDC,
    offset_x: i32,
    offset_y: i32,
    annotations: &[Annotation],
) {
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        for ann in annotations {
            let Annotation::Text { x, y, text, size, color } = ann else { continue };
            if text.trim().is_empty() { continue; }
            let font = make_text_font(*size);
            let old = SelectObject(hdc, font.into());
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            let (tx, ty) = (x - offset_x, y - offset_y);
            // Subtle shadow for readability across light and dark backgrounds
            let shadow = if color.0 < 60 && color.1 < 60 && color.2 < 60 {
                COLORREF(0x00FF_FFFF)
            } else {
                COLORREF(0x0010_1014)
            };
            SetTextColor(hdc, shadow);
            let mut sr = RECT { left: tx + 1, top: ty + 1, right: tx + 1000, bottom: ty + size + 16 };
            DrawTextW(hdc, &mut wide, &mut sr, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOCLIP);

            SetTextColor(hdc, COLORREF((color.0 as u32) | ((color.1 as u32) << 8) | ((color.2 as u32) << 16)));
            let mut rc = RECT { left: tx, top: ty, right: tx + 1000, bottom: ty + size + 16 };
            DrawTextW(hdc, &mut wide, &mut rc, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOCLIP);
            let _ = SelectObject(hdc, old);
            let _ = DeleteObject(font.into());
        }
    }
}

/// Bakes any `Annotation::Text` entries into a cropped BGRA buffer using an ephemeral DIB (0 GDI leaks).
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
    unsafe {
        let mem_dc = CreateCompatibleDC(None);
        if mem_dc.is_invalid() { return; }
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
        if let Ok(bmp) = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            let old = SelectObject(mem_dc, bmp.into());
            let slice = std::slice::from_raw_parts_mut(bits as *mut u8, bgra.len());
            slice.copy_from_slice(bgra);
            render_text_annotations_gdi(mem_dc, offset_x, offset_y, annotations);
            let _ = GdiFlush();
            bgra.copy_from_slice(slice);
            for px in bgra.as_chunks_mut::<4>().0 { px[3] = 255; }
            let _ = SelectObject(mem_dc, old);
            let _ = DeleteObject(bmp.into());
        }
        let _ = DeleteDC(mem_dc);
    }
}

pub struct InlineTextEditor {
    pub hwnd: HWND,
    pub font: HFONT,
    pub x: i32,
    pub y: i32,
    pub size: i32,
    pub color: AnnotColor,
}

impl InlineTextEditor {
    pub fn spawn(parent: HWND, x: i32, y: i32, size: i32, color: AnnotColor) -> Option<Self> {
        unsafe {
            let h = CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_BORDER | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                x, y, 240, (size + 12).clamp(26, 68),
                Some(parent), None, None, None,
            ).ok()?;
            let font = make_text_font(size);
            SendMessageW(h, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            let _ = SetFocus(Some(h));
            Some(Self { hwnd: h, font, x, y, size, color })
        }
    }

    pub fn commit_and_close(self) -> Option<Annotation> {
        let text = unsafe {
            let len = GetWindowTextLengthW(self.hwnd);
            let mut buf = vec![0u16; (len.max(0) as usize) + 1];
            let n = if len > 0 { (GetWindowTextW(self.hwnd, &mut buf)) as usize } else { 0 };
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteObject(self.font.into());
            String::from_utf16_lossy(&buf[..n])
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(Annotation::Text {
                x: self.x,
                y: self.y,
                text: trimmed.to_string(),
                size: self.size,
                color: self.color,
            })
        }
    }

    pub fn cancel_and_close(self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteObject(self.font.into());
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn paint_overlay_gdi_labels(
    mem_dc: HDC,
    font: HFONT,
    snap: &DesktopSnapshot,
    sel: Option<ScreenRect>,
    hover_snap: Option<SnapCandidate>,
    cycle_count: usize,
    annotations: &[Annotation],
    cursor: (i32, i32),
    show_loupe: bool,
    layout: &ToolbarLayout,
    hovered_cmd: Option<ToolbarCmd>,
    hex_lower: bool,
    rgb_mode: bool,
    wheel_hud: Option<&str>,
) {
    if sel.is_some() {
        render_text_annotations_gdi(mem_dc, 0, 0, annotations);
    }
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
            let b = snap_badge_rect(h.rect, snap.width);
            let mut rc = RECT { left: b.x, top: b.y, right: b.right(), bottom: b.bottom() };
            SetTextColor(mem_dc, COLORREF(0x00FE_F2E0));
            let txt = if cycle_count <= 1 {
                format!("{} {}×{} · 拖拽框选 / 单击全屏", h.kind.label(), h.rect.w, h.rect.h)
            } else {
                format!("{} {}×{} · 单击选中 · Tab切层", h.kind.label(), h.rect.w, h.rect.h)
            };
            let mut w: Vec<u16> = txt.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        if let Some(cmd) = hovered_cmd
            && let Some(btn) = layout.hits.iter().find(|b| b.cmd == cmd)
                && let Some(tr) = tooltip_rect_for(btn, snap.width, snap.height) {
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
