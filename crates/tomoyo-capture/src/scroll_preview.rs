//! Real-time docked live preview strip for Long-Screenshot capture (`tomoyo-capture.exe`).
//!
//! Dynamically grows its window height alongside the stitched `StitchCanvas` (eliminating any
//! large empty black box on Frame 1), then clamps at screen budget with bottom-anchored tracking
//! and excludes itself from screen capture via `WDA_EXCLUDEFROMCAPTURE`.

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
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE,
    WM_ERASEBKGND, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

const PREVIEW_W: i32 = 216;
const PAD: i32 = 8;
const HEAD_H: i32 = 28;
const TRACK_W: i32 = 5;
const VIEW_W: i32 = PREVIEW_W - PAD * 2 - TRACK_W - 4;

#[derive(Debug, Clone, Copy, PartialEq)]
struct PreviewLayout {
    view_h: i32,
    draw_w: i32,
    draw_h: i32,
    src_x0: f32,
    src_y0: f32,
    src_w: f32,
    src_h: f32,
    thumb_ratio: f32,
}

fn compute_preview_layout(
    canvas_w: u32,
    canvas_h: u32,
    axis: ScrollAxis,
    view_w: i32,
    max_view_h: i32,
) -> PreviewLayout {
    let cw = canvas_w.max(1) as f32;
    let ch = canvas_h.max(1) as f32;
    match axis {
        ScrollAxis::Vertical => {
            let scale = view_w as f32 / cw;
            let scaled_h = (ch * scale).round() as i32;
            if scaled_h <= max_view_h {
                let vh = scaled_h.max(24);
                PreviewLayout {
                    view_h: vh,
                    draw_w: view_w,
                    draw_h: scaled_h.clamp(1, vh),
                    src_x0: 0.0,
                    src_y0: 0.0,
                    src_w: cw,
                    src_h: ch,
                    thumb_ratio: 1.0,
                }
            } else {
                let vis_src_h = (max_view_h as f32 / scale).min(ch);
                PreviewLayout {
                    view_h: max_view_h,
                    draw_w: view_w,
                    draw_h: max_view_h,
                    src_x0: 0.0,
                    src_y0: ch - vis_src_h,
                    src_w: cw,
                    src_h: vis_src_h,
                    thumb_ratio: (vis_src_h / ch).clamp(0.10, 1.0),
                }
            }
        }
        ScrollAxis::Horizontal => {
            let scale = (max_view_h as f32 / ch).min(view_w as f32 / cw.min(ch * 1.4));
            let scaled_w = cw * scale;
            let dh = (ch * scale).round().clamp(24.0, max_view_h as f32) as i32;
            if scaled_w <= view_w as f32 {
                PreviewLayout {
                    view_h: dh,
                    draw_w: (scaled_w.round() as i32).max(1),
                    draw_h: dh,
                    src_x0: 0.0,
                    src_y0: 0.0,
                    src_w: cw,
                    src_h: ch,
                    thumb_ratio: 1.0,
                }
            } else {
                let vis_src_w = (view_w as f32 / scale).min(cw);
                PreviewLayout {
                    view_h: dh,
                    draw_w: view_w,
                    draw_h: dh,
                    src_x0: cw - vis_src_w,
                    src_y0: 0.0,
                    src_w: vis_src_w,
                    src_h: ch,
                    thumb_ratio: (vis_src_w / cw).clamp(0.10, 1.0),
                }
            }
        }
    }
}

pub struct LivePreviewStrip {
    pub hwnd: HWND,
    x: i32,
    y: i32,
    preferred_y: i32,
    work_top: i32,
    work_bottom: i32,
    width: i32,
    height: i32,
    max_win_h: i32,
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
            let max_win_h = (vh - 48).clamp(240, 560);
            let max_view_h = (max_win_h - HEAD_H - PAD).max(40);
            let init_layout = compute_preview_layout(
                region_w.max(1) as u32,
                region_h.max(1) as u32,
                ScrollAxis::Vertical,
                VIEW_W,
                max_view_h,
            );
            let init_h = (HEAD_H + init_layout.view_h + PAD).clamp(68, max_win_h);
            let x = if region_x + region_w + 12 + PREVIEW_W <= vx + vw - 8 {
                region_x + region_w + 12
            } else if region_x - 12 - PREVIEW_W >= vx + 8 {
                region_x - 12 - PREVIEW_W
            } else {
                (vx + vw - PREVIEW_W - 12).max(vx + 8)
            };
            let work_top = vy + 8;
            let work_bottom = (vy + vh - 8).max(work_top + 68);
            let y = region_y.clamp(work_top, (work_bottom - init_h).max(work_top));

            let mem_dc = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: PREVIEW_W,
                    biHeight: -max_win_h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
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
                x, y, PREVIEW_W, init_h, None, None, Some(hinst), None,
            ).ok()?;
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, PREVIEW_W, init_h, SWP_NOACTIVATE);

            let strip = Self {
                hwnd,
                x,
                y,
                preferred_y: region_y,
                work_top,
                work_bottom,
                width: PREVIEW_W,
                height: init_h,
                max_win_h,
                mem_dc,
                dib_bmp,
                dib_bits: bits as *mut u8,
            };
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, strip.mem_dc.0 as isize);
            Some(strip)
        }
    }

    /// Re-renders the live stitched thumbnail and grows the docked window seamlessly with content.
    pub fn redraw(&mut self, canvas: &StitchCanvas, font: HFONT) {
        let max_view_h = (self.max_win_h - HEAD_H - PAD).max(40);
        let lo = compute_preview_layout(canvas.width, canvas.height, canvas.axis, VIEW_W, max_view_h);
        let pw = self.width;
        let ph = (HEAD_H + lo.view_h + PAD).clamp(68, self.max_win_h);
        let next_y = self.preferred_y.clamp(self.work_top, (self.work_bottom - ph).max(self.work_top));
        if ph != self.height || next_y != self.y {
            self.height = ph;
            self.y = next_y;
            unsafe {
                let _ = SetWindowPos(self.hwnd, None, self.x, next_y, pw, ph, SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }

        let dst = unsafe { std::slice::from_raw_parts_mut(self.dib_bits, (pw * self.max_win_h * 4) as usize) };
        for px in dst[..(pw * ph * 4) as usize].as_chunks_mut::<4>().0 {
            px[0] = 0x20; px[1] = 0x18; px[2] = 0x16; px[3] = 255;
        }
        draw_capsule(dst, pw, ph, ScreenRect { x: 0, y: 0, w: pw, h: ph }, 10.0);

        let view_x = PAD;
        let view_y = HEAD_H;
        let view_h = lo.view_h;
        let view_rect = ScreenRect { x: view_x, y: view_y, w: VIEW_W, h: view_h };
        fill_rounded_rect_aa(dst, pw, ph, view_rect, 4.0, AnnotColor(0x0E, 0x10, 0x16), 1.0);

        // Crisp 2x2 area-averaged downscale from `canvas.bgra` into the fitted preview viewport
        let c_stride = canvas.width as usize;
        for dy in 0..lo.draw_h {
            let fy = lo.src_y0 + (dy as f32 + 0.5) * lo.src_h / (lo.draw_h as f32);
            let sy0 = (fy as u32).min(canvas.height - 1);
            let sy1 = (sy0 + 1).min(canvas.height - 1);
            let dst_row = ((view_y + dy) as usize) * (pw as usize);
            for dx in 0..lo.draw_w {
                let fx = lo.src_x0 + (dx as f32 + 0.5) * lo.src_w / (lo.draw_w as f32);
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

        let tx = view_x + VIEW_W + 4;
        fill_rounded_rect_aa(dst, pw, ph, ScreenRect { x: tx, y: view_y, w: TRACK_W, h: view_h }, 2.5, AnnotColor(0x28, 0x2D, 0x3D), 1.0);
        let thumb_h = ((view_h as f32) * lo.thumb_ratio).round().clamp(16.0, view_h as f32) as i32;
        fill_rounded_rect_aa(dst, pw, ph, ScreenRect { x: tx, y: view_y + view_h - thumb_h, w: TRACK_W, h: thumb_h }, 2.5, AnnotColor(0x3B, 0x82, 0xF6), 1.0);

        unsafe {
            let old = SelectObject(self.mem_dc, font.into());
            SetBkMode(self.mem_dc, TRANSPARENT);
            SetTextColor(self.mem_dc, COLORREF(0x00E2_E8F0));
            let title = format!("实时预览 · {} 帧 ({}×{})", canvas.frames, canvas.width, canvas.height);
            let mut rc = RECT { left: 10, top: 4, right: pw - 8, bottom: HEAD_H };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_layout_eliminates_initial_empty_black_box_and_grows_until_budget() {
        let initial = compute_preview_layout(1000, 450, ScrollAxis::Vertical, VIEW_W, 520);
        assert_eq!(initial.draw_h, initial.view_h);
        assert!(initial.view_h < 120, "expected compact height without void, got {}", initial.view_h);
        assert_eq!(initial.thumb_ratio, 1.0);

        let tall = compute_preview_layout(1000, 4000, ScrollAxis::Vertical, VIEW_W, 520);
        assert_eq!(tall.view_h, 520);
        assert_eq!(tall.draw_h, 520);
        assert!(tall.thumb_ratio < 0.8);
    }
}
