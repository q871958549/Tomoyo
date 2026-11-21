//! Live Vertical & Horizontal Scrolling Long-Screenshot capture session (`tomoyo-capture.exe`).
//!
//! Renders a click-through hollow border outside the capture region plus a floating minimalist
//! dark capsule HUD (excluded from capture via `WDA_EXCLUDEFROMCAPTURE`) with smooth-scroll
//! settling, right-pane wheel targeting, and direct Copy / Save PNG / Pin output (0 idle RAM).

#![cfg(windows)]

use crate::export::{copy_bgra_to_clipboard, default_save_path, save_bgra_as_png};
use crate::scroll_hud::{
    build_hud_buttons, capture_region_bgra, frame_wndproc, paint_hud_canvas, send_scroll_step,
    HudBtn, HudCmd, HUD_H, HUD_W,
};
use crate::scroll_preview::LivePreviewStrip;
use crate::stitch::{frame_mean_diff, ScrollAxis, StitchCanvas, StitchOutcome};
use std::ffi::c_void;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject,
    EndPaint, InvalidateRect, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    FF_DONTCARE, FW_NORMAL, HBITMAP, HDC, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN, VK_SPACE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassW,
    SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity, SetWindowLongPtrW, ShowWindow,
    TranslateMessage, GWLP_USERDATA, IDC_ARROW, LWA_COLORKEY, MA_NOACTIVATE, MSG,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const TIMER_CAPTURE: usize = 701;

pub enum ScrollOutcome {
    Done,
    Pin { width: u32, height: u32, bgra: Vec<u8>, screen_x: i32, screen_y: i32 },
}

struct ScrollState {
    region_x: i32,
    region_y: i32,
    region_w: i32,
    region_h: i32,
    save_dir: String,
    canvas: StitchCanvas,
    last_raw: Vec<u8>,
    auto_scroll: bool,
    awaiting_settle: bool,
    settle_ticks: u32,
    idle_ticks: u32,
    hovered: Option<HudCmd>,
    btns: Vec<HudBtn>,
    frame_hwnd: HWND,
    preview: Option<LivePreviewStrip>,
    mem_dc: HDC,
    dib_bmp: HBITMAP,
    dib_bits: *mut u8,
    font: HFONT,
    outcome: Option<ScrollOutcome>,
}

impl Drop for ScrollState {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_invalid() { let _ = DeleteObject(self.font.into()); }
            if !self.dib_bmp.is_invalid() { let _ = DeleteObject(self.dib_bmp.into()); }
            if !self.mem_dc.is_invalid() { let _ = DeleteDC(self.mem_dc); }
        }
    }
}

pub fn run_scroll_capture(
    screen_x: i32, screen_y: i32, w: i32, h: i32, save_dir: String,
) -> Result<ScrollOutcome, String> {
    let first = capture_region_bgra(screen_x, screen_y, w, h)
        .ok_or_else(|| "Initial region capture failed".to_string())?;
    let last_raw = first.clone();
    let canvas = StitchCanvas::new(ScrollAxis::Vertical, w as u32, h as u32, first);

    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| format!("{e}"))?;
        let frame_cls = w!("TomoyoScrollFrameClass");
        let _ = RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(frame_wndproc), hInstance: hinst.into(), lpszClassName: frame_cls,
            ..Default::default()
        });
        let hud_cls = w!("TomoyoScrollHudClass");
        let _ = RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(hud_wndproc), hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), lpszClassName: hud_cls,
            ..Default::default()
        });

        let pad = 3;
        let frame_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            frame_cls, w!(""), WS_POPUP,
            screen_x - pad, screen_y - pad, w + pad * 2, h + pad * 2,
            None, None, Some(hinst.into()), None,
        ).map_err(|e| format!("Scroll frame create failed: {e}"))?;
        let _ = SetLayeredWindowAttributes(frame_hwnd, COLORREF(0x00FF_00FF), 0, LWA_COLORKEY);
        let _ = SetWindowDisplayAffinity(frame_hwnd, WDA_EXCLUDEFROMCAPTURE);
        let _ = ShowWindow(frame_hwnd, SW_SHOWNOACTIVATE);

        let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        let hud_x = (screen_x + (w - HUD_W) / 2).clamp(vx + 8, (vx + vw - HUD_W - 8).max(vx + 8));
        let hud_y = if screen_y + h + 10 + HUD_H <= vy + vh - 8 {
            screen_y + h + 10
        } else if screen_y - 10 - HUD_H >= vy + 8 {
            screen_y - 10 - HUD_H
        } else {
            vy + 14
        };

        let mem_dc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: HUD_W, biHeight: -HUD_H, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib_bmp = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|e| format!("HUD DIB failed: {e}"))?;
        let _ = SelectObject(mem_dc, dib_bmp.into());
        let face: Vec<u16> = "Microsoft YaHei UI".encode_utf16().chain(std::iter::once(0)).collect();
        let font = CreateFontW(
            -13, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(face.as_ptr()),
        );

        let mut preview = LivePreviewStrip::create(hinst.into(), screen_x, screen_y, w, h, vx, vy, vw, vh);
        if let Some(p) = preview.as_mut() {
            p.redraw(&canvas, font);
        }

        let mut st = Box::new(ScrollState {
            region_x: screen_x, region_y: screen_y, region_w: w, region_h: h, save_dir,
            canvas, last_raw, auto_scroll: false, awaiting_settle: false, settle_ticks: 0,
            idle_ticks: 0, hovered: None, btns: Vec::new(), frame_hwnd, preview, mem_dc, dib_bmp,
            dib_bits: bits as *mut u8, font, outcome: None,
        });
        st.rebuild_btns();

        let hud_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, hud_cls, w!("Tomoyo Long Capture"), WS_POPUP,
            hud_x, hud_y, HUD_W, HUD_H, None, None, Some(hinst.into()), None,
        ).map_err(|e| format!("HUD window create failed: {e}"))?;
        let _ = SetWindowDisplayAffinity(hud_hwnd, WDA_EXCLUDEFROMCAPTURE);
        SetWindowLongPtrW(hud_hwnd, GWLP_USERDATA, &mut *st as *mut ScrollState as isize);
        let _ = ShowWindow(hud_hwnd, SW_SHOWNOACTIVATE);
        let _ = SetTimer(Some(hud_hwnd), TIMER_CAPTURE, 150, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = DestroyWindow(frame_hwnd);
        Ok(st.outcome.take().unwrap_or(ScrollOutcome::Done))
    }
}

impl ScrollState {
    fn rebuild_btns(&mut self) {
        self.btns = build_hud_buttons(self.canvas.axis, self.auto_scroll);
    }

    fn feed_settled_frame(&mut self, hwnd: HWND, frame: &[u8]) {
        match self.canvas.push_frame(frame, self.region_w as u32, self.region_h as u32) {
            StitchOutcome::Appended { .. } => {
                self.idle_ticks = 0;
                if let Some(p) = self.preview.as_mut() {
                    p.redraw(&self.canvas, self.font);
                }
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            StitchOutcome::LimitReached => {
                self.auto_scroll = false;
                self.awaiting_settle = false;
                self.rebuild_btns();
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            StitchOutcome::Unchanged => {
                if self.auto_scroll {
                    self.idle_ticks += 1;
                    if self.idle_ticks >= 3 {
                        self.auto_scroll = false;
                        self.awaiting_settle = false;
                        self.rebuild_btns();
                        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
                    }
                }
            }
        }
    }

    fn execute(&mut self, hwnd: HWND, cmd: HudCmd) {
        match cmd {
            HudCmd::ToggleAxis => {
                let next_axis = if self.canvas.axis == ScrollAxis::Vertical {
                    ScrollAxis::Horizontal
                } else {
                    ScrollAxis::Vertical
                };
                self.auto_scroll = false;
                self.awaiting_settle = false;
                self.idle_ticks = 0;
                if let Some(first) = capture_region_bgra(self.region_x, self.region_y, self.region_w, self.region_h) {
                    self.last_raw = first.clone();
                    self.canvas = StitchCanvas::new(next_axis, self.region_w as u32, self.region_h as u32, first);
                    if let Some(p) = self.preview.as_mut() { p.redraw(&self.canvas, self.font); }
                }
                self.rebuild_btns();
            }
            HudCmd::ToggleAuto => {
                self.auto_scroll = !self.auto_scroll;
                self.awaiting_settle = false;
                self.settle_ticks = 0;
                self.idle_ticks = 0;
                self.rebuild_btns();
            }
            HudCmd::Save => {
                let path = default_save_path(&self.save_dir);
                let _ = save_bgra_as_png(&path, self.canvas.width, self.canvas.height, &self.canvas.bgra);
                let _ = copy_bgra_to_clipboard(self.canvas.width, self.canvas.height, &self.canvas.bgra);
                self.outcome = Some(ScrollOutcome::Done);
                unsafe { let _ = KillTimer(Some(hwnd), TIMER_CAPTURE); let _ = DestroyWindow(hwnd); }
                return;
            }
            HudCmd::Pin => {
                let _ = copy_bgra_to_clipboard(self.canvas.width, self.canvas.height, &self.canvas.bgra);
                self.outcome = Some(ScrollOutcome::Pin {
                    width: self.canvas.width,
                    height: self.canvas.height,
                    bgra: std::mem::take(&mut self.canvas.bgra),
                    screen_x: self.region_x,
                    screen_y: self.region_y,
                });
                unsafe { let _ = KillTimer(Some(hwnd), TIMER_CAPTURE); let _ = DestroyWindow(hwnd); }
                return;
            }
            HudCmd::Copy => {
                let _ = copy_bgra_to_clipboard(self.canvas.width, self.canvas.height, &self.canvas.bgra);
                self.outcome = Some(ScrollOutcome::Done);
                unsafe { let _ = KillTimer(Some(hwnd), TIMER_CAPTURE); let _ = DestroyWindow(hwnd); }
                return;
            }
            HudCmd::Cancel => {
                self.outcome = Some(ScrollOutcome::Done);
                unsafe { let _ = KillTimer(Some(hwnd), TIMER_CAPTURE); let _ = DestroyWindow(hwnd); }
                return;
            }
        }
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    }
}

unsafe extern "system" fn hud_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut ScrollState;
    if ptr.is_null() { return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }; }
    let st = unsafe { &mut *ptr };
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if wparam.0 == TIMER_CAPTURE => {
            if let Some(frame) = capture_region_bgra(st.region_x, st.region_y, st.region_w, st.region_h) {
                let diff = frame_mean_diff(&st.last_raw, &frame, st.region_w as u32, st.region_h as u32);
                if st.auto_scroll {
                    if !st.awaiting_settle {
                        st.last_raw = frame;
                        // Target right-center of region so windows with a left sidebar scroll their main pane
                        let tx = st.region_x + (st.region_w * 2) / 3;
                        let ty = st.region_y + st.region_h / 2;
                        send_scroll_step(tx, ty, st.canvas.axis);
                        st.awaiting_settle = true;
                        st.settle_ticks = 0;
                    } else if diff > 1.5 && st.settle_ticks < 4 {
                        st.last_raw = frame;
                        st.settle_ticks += 1;
                    } else {
                        st.last_raw = frame.clone();
                        st.awaiting_settle = false;
                        st.feed_settled_frame(hwnd, &frame);
                    }
                } else {
                    if diff > 1.8 && st.settle_ticks < 2 {
                        st.last_raw = frame;
                        st.settle_ticks += 1;
                    } else {
                        st.last_raw = frame.clone();
                        st.settle_ticks = 0;
                        st.feed_settled_frame(hwnd, &frame);
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);
            let next = st.btns.iter().find(|b| b.rect.contains(x, y)).map(|b| b.cmd);
            if next != st.hovered {
                st.hovered = next;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);
            if let Some(b) = st.btns.iter().find(|b| b.rect.contains(x, y)).copied() {
                st.execute(hwnd, b.cmd);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            if vk == VK_ESCAPE.0 { st.execute(hwnd, HudCmd::Cancel); }
            else if vk == VK_RETURN.0 { st.execute(hwnd, HudCmd::Copy); }
            else if vk == VK_SPACE.0 { st.execute(hwnd, HudCmd::ToggleAuto); }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            paint_hud_canvas(st.dib_bits, st.mem_dc, st.font, &st.canvas, st.auto_scroll, &st.btns, st.hovered);
            unsafe {
                let _ = BitBlt(hdc, 0, 0, HUD_W, HUD_H, Some(st.mem_dc), 0, 0, SRCCOPY);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            st.preview = None;
            unsafe { let _ = DestroyWindow(st.frame_hwnd); PostQuitMessage(0); }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
