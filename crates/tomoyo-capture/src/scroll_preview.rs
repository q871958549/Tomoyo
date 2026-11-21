//! Real-time docked live preview strip for Long-Screenshot capture (`tomoyo-capture.exe`).
//!
//! Displays a live aspect-preserved downscaled view of the stitched `StitchCanvas` right beside
//! the capture region (anchored to the newest stitched tail so users always see exactly what has
//! been captured), excluded from screen capture via `WDA_EXCLUDEFROMCAPTURE`.

#![cfg(windows)]

use crate::annotate::AnnotColor;
use crate::overlay_icons::{draw_capsule, fill_rounded_rect_aa, stroke_rounded_rect_aa};
use crate::screen::ScreenRect;
use crate::stitch::{ScrollAxis, StitchCanvas};
use std::ffi::c_void;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, DrawTextW,
    EndPaint, InvalidateRect, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HBITMAP, HDC, HFONT, PAINTSTRUCT,
    SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, RegisterClassW,
    SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA,
    HWND_TOPMOST, SWP_NOACTIVATE, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WM_ERASEBKGND,
    WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub struct LivePreviewStrip {
    pub hwnd: HWND,
    width: i32,
    height: i32,
    mem_dc: HDC,
    dib_bmp: HBITMAP,
    dib_bits: *mut u8,
}

impl Drop for LivePreviewStrip {
    fn drop(&mut self) {
        unsafe {
            if !self.hwnd.is_invalid() { let _ = DestroyWindow(self.hwnd); }
            if !self.dib_bmp.is_invalid() { let _ = DeleteObject(self.dib_bmp.into()); }
            if !self.mem_dc.is_invalid() { let _ = DeleteDC(self.mem_dc); }
        }
    }
}

impl LivePreviewStrip {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        hinst: windows::Win32::Foundation::HINSTANCE,
        region_x: i32, region_y: i32, region_w: i32, region_h: i32,
        vx: i32, vy: i32, vw: i32, vh: i32,
    ) -> Option<Self> {
        let cls = w!("TomoyoScrollPreviewClass");
        unsafe {
            let _ = RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(preview_wndproc),
                hInstance: hinst,
                lpszClassName: cls,
                ..Default::default()
            });
            let w = 216;
            let h = region_h.clamp(240, (vh - 48).clamp(240, 560));
            let x = if region_x + region_w + 12 + w <= vx + vw - 8 {
                region_x + region_w + 12
            } else if region_x - 12 - w >= vx + 8 {
                region_x - 12 - w
            } else {
                (vx + vw - w - 12).max(vx + 8)
            };
            let y = region_y.clamp(vy + 8, (vy + vh - h - 8).max(vy + 8));

            let mem_dc = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let dib_bmp = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            let _ = SelectObject(mem_dc, dib_bmp.into());

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                cls, w!("Tomoyo Preview"), WS_POPUP,
                x, y, w, h, None, None, Some(hinst), None,
            ).ok()?;
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);

            let strip = Self { hwnd, width: w, height: h, mem_dc, dib_bmp, dib_bits: bits as *mut u8 };
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, strip.mem_dc.0 as isize);
            Some(strip)
        }
    }

    /// Re-renders the live stitched thumbnail (latest bottom/right tail + progress bar) into DIB.
    pub fn redraw(&mut self, canvas: &StitchCanvas, font: HFONT) {
        let (pw, ph) = (self.width, self.height);
        let dst = unsafe { std::slice::from_raw_parts_mut(self.dib_bits, (pw * ph * 4) as usize) };
        for px in dst.chunks_exact_mut(4) { px[0] = 0x20; px[1] = 0x18; px[2] = 0x16; px[3] = 255; }
        draw_capsule(dst, pw, ph, ScreenRect { x: 0, y: 0, w: pw, h: ph }, 10.0);

        let pad = 8;
        let head_h = 28;
        let track_w = 5;
        let view_x = pad;
        let view_y = head_h;
        let view_w = (pw - pad * 2 - track_w - 4).max(32);
        let view_h = (ph - head_h - pad).max(40);
        let view_rect = ScreenRect { x: view_x, y: view_y, w: view_w, h: view_h };
        fill_rounded_rect_aa(dst, pw, ph, view_rect, 4.0, AnnotColor(0x0E, 0x10, 0x16), 1.0);

        let (cw, ch) = (canvas.width.max(1) as f32, canvas.height.max(1) as f32);
        let (draw_w, draw_h, src_x0, src_y0, src_w, src_h, thumb_ratio) = match canvas.axis {
            ScrollAxis::Vertical => {
                let scale = view_w as f32 / cw;
                let scaled_h = ch * scale;
                if scaled_h <= view_h as f32 {
                    (view_w, (scaled_h.round() as i32).max(1), 0.0f32, 0.0f32, cw, ch, 1.0f32)
                } else {
                    let vis_src_h = (view_h as f32 / scale).min(ch);
                    (view_w, view_h, 0.0, ch - vis_src_h, cw, vis_src_h, (vis_src_h / ch).clamp(0.10, 1.0))
                }
            }
            ScrollAxis::Horizontal => {
                let scale = (view_h as f32 / ch).min(view_w as f32 / cw.min(ch * 1.4));
                let scaled_w = cw * scale;
                let dh = (ch * scale).round().clamp(1.0, view_h as f32) as i32;
                if scaled_w <= view_w as f32 {
                    ((scaled_w.round() as i32).max(1), dh, 0.0f32, 0.0f32, cw, ch, 1.0f32)
                } else {
                    let vis_src_w = (view_w as f32 / scale).min(cw);
                    (view_w, dh, cw - vis_src_w, 0.0, vis_src_w, ch, (vis_src_w / cw).clamp(0.10, 1.0))
                }
            }
        };

        // Crisp 2x2 area-averaged downscale from `canvas.bgra` into preview DIB
        let c_stride = canvas.width as usize;
        for dy in 0..draw_h {
            let fy = src_y0 + (dy as f32 + 0.5) * src_h / (draw_h as f32);
            let sy0 = (fy as u32).min(canvas.height - 1);
            let sy1 = (sy0 + 1).min(canvas.height - 1);
            let dst_row = ((view_y + dy) as usize) * (pw as usize);
            for dx in 0..draw_w {
                let fx = src_x0 + (dx as f32 + 0.5) * src_w / (draw_w as f32);
                let sx0 = (fx as u32).min(canvas.width - 1);
                let sx1 = (sx0 + 1).min(canvas.width - 1);
                let o00 = (sy0 as usize * c_stride + sx0 as usize) * 4;
                let o10 = (sy0 as usize * c_stride + sx1 as usize) * 4;
                let o01 = (sy1 as usize * c_stride + sx0 as usize) * 4;
                let o11 = (sy1 as usize * c_stride + sx1 as usize) * 4;
                let doff = (dst_row + (view_x + dx) as usize) * 4;
                for ch_i in 0..3 {
                    let avg = (canvas.bgra[o00 + ch_i] as u16
                        + canvas.bgra[o10 + ch_i] as u16
                        + canvas.bgra[o01 + ch_i] as u16
                        + canvas.bgra[o11 + ch_i] as u16) >> 2;
                    dst[doff + ch_i] = avg as u8;
                }
                dst[doff + 3] = 255;
            }
        }
        stroke_rounded_rect_aa(dst, pw, ph, view_rect, 4.0, 1.0, AnnotColor(0x38, 0x3D, 0x4F), 0.9);

        // Right-edge vertical scroll depth indicator (anchored at bottom = latest content)
        let tx = view_x + view_w + 4;
        fill_rounded_rect_aa(dst, pw, ph, ScreenRect { x: tx, y: view_y, w: track_w, h: view_h }, 2.5, AnnotColor(0x28, 0x2D, 0x3D), 1.0);
        let thumb_h = ((view_h as f32) * thumb_ratio).round().clamp(18.0, view_h as f32) as i32;
        fill_rounded_rect_aa(dst, pw, ph, ScreenRect { x: tx, y: view_y + view_h - thumb_h, w: track_w, h: thumb_h }, 2.5, AnnotColor(0x3B, 0x82, 0xF6), 1.0);

        unsafe {
            let old = SelectObject(self.mem_dc, font.into());
            SetBkMode(self.mem_dc, TRANSPARENT);
            SetTextColor(self.mem_dc, COLORREF(0x00E2_E8F0));
            let title = format!("实时预览 · {} 帧 ({}×{})", canvas.frames, canvas.width, canvas.height);
            let mut rc = RECT { left: 10, top: 4, right: pw - 8, bottom: head_h };
            let mut tw: Vec<u16> = title.encode_utf16().collect();
            DrawTextW(self.mem_dc, &mut tw, &mut rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            let _ = SelectObject(self.mem_dc, old);
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }
}

unsafe extern "system" fn preview_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            let raw_dc = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            if raw_dc != 0 {
                let mut rc = RECT::default();
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
                    let _ = BitBlt(hdc, 0, 0, rc.right - rc.left, rc.bottom - rc.top, Some(HDC(raw_dc as *mut c_void)), 0, 0, SRCCOPY);
                }
            }
            unsafe { let _ = EndPaint(hwnd, &ps); }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
