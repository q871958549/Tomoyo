//! Live Vertical & Horizontal Scrolling Long-Screenshot capture session (`tomoyo-capture.exe`).
//!
//! Renders a click-through hollow border outside the capture region plus a floating minimalist
//! dark capsule HUD (excluded from capture via `WDA_EXCLUDEFROMCAPTURE`) with smooth-scroll
//! settling, right-pane wheel targeting, and direct Copy / Save PNG / Pin output (0 idle RAM).

#![cfg(windows)]

use crate::annotate::AnnotColor;
use crate::export::{copy_bgra_to_clipboard, default_save_path, save_bgra_as_png};
use crate::overlay_icons::{draw_capsule, fill_rounded_rect_aa, stroke_rounded_rect_aa};
use crate::screen::ScreenRect;
use crate::stitch::{frame_mean_diff, ScrollAxis, StitchCanvas, StitchOutcome};
use std::ffi::c_void;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, InvalidateRect, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL, HBITMAP, HDC, HFONT,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    VK_ESCAPE, VK_RETURN, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassW,
    SetCursorPos, SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity,
    SetWindowLongPtrW, ShowWindow, TranslateMessage, GWLP_USERDATA, IDC_ARROW, LWA_COLORKEY, MSG,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOW,
    SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const TIMER_CAPTURE: usize = 701;
const HUD_W: i32 = 436;
const HUD_H: i32 = 76;

pub enum ScrollOutcome {
    Done,
    Pin { width: u32, height: u32, bgra: Vec<u8>, screen_x: i32, screen_y: i32 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HudCmd { ToggleAxis, ToggleAuto, Save, Pin, Copy, Cancel }

#[derive(Clone, Copy)]
struct HudBtn { cmd: HudCmd, rect: ScreenRect, label: &'static str, accent: bool, active: bool }

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

        let mut st = Box::new(ScrollState {
            region_x: screen_x, region_y: screen_y, region_w: w, region_h: h, save_dir,
            canvas, last_raw, auto_scroll: false, awaiting_settle: false, settle_ticks: 0,
            idle_ticks: 0, hovered: None, btns: Vec::new(), frame_hwnd, mem_dc, dib_bmp,
            dib_bits: bits as *mut u8, font, outcome: None,
        });
        st.rebuild_btns();

        let hud_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW, hud_cls, w!("Tomoyo Long Capture"), WS_POPUP,
            hud_x, hud_y, HUD_W, HUD_H, None, None, Some(hinst.into()), None,
        ).map_err(|e| format!("HUD window create failed: {e}"))?;
        let _ = SetWindowDisplayAffinity(hud_hwnd, WDA_EXCLUDEFROMCAPTURE);
        SetWindowLongPtrW(hud_hwnd, GWLP_USERDATA, &mut *st as *mut ScrollState as isize);
        let _ = ShowWindow(hud_hwnd, SW_SHOW);
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
        let axis_lbl = if self.canvas.axis == ScrollAxis::Vertical { "纵向模式" } else { "横向模式" };
        let auto_lbl = if self.auto_scroll { "暂停自动" } else { "自动滚动" };
        let y = 38;
        self.btns = vec![
            HudBtn { cmd: HudCmd::ToggleAxis, rect: ScreenRect { x: 10, y, w: 70, h: 28 }, label: axis_lbl, accent: false, active: true },
            HudBtn { cmd: HudCmd::ToggleAuto, rect: ScreenRect { x: 86, y, w: 70, h: 28 }, label: auto_lbl, accent: false, active: self.auto_scroll },
            HudBtn { cmd: HudCmd::Save, rect: ScreenRect { x: 166, y, w: 54, h: 28 }, label: "保存", accent: false, active: false },
            HudBtn { cmd: HudCmd::Pin, rect: ScreenRect { x: 226, y, w: 54, h: 28 }, label: "贴图", accent: false, active: false },
            HudBtn { cmd: HudCmd::Copy, rect: ScreenRect { x: 286, y, w: 96, h: 28 }, label: "复制完成", accent: true, active: false },
            HudBtn { cmd: HudCmd::Cancel, rect: ScreenRect { x: 388, y, w: 38, h: 28 }, label: "✕", accent: false, active: false },
        ];
    }

    fn feed_settled_frame(&mut self, hwnd: HWND, frame: &[u8]) {
        match self.canvas.push_frame(frame, self.region_w as u32, self.region_h as u32) {
            StitchOutcome::Appended { .. } => {
                self.idle_ticks = 0;
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

unsafe extern "system" fn frame_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let mut ps = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rc);
            let key_br = CreateSolidBrush(COLORREF(0x00FF_00FF));
            let border_br = CreateSolidBrush(COLORREF(0x00F6_823B));
            FillRect(hdc, &rc, border_br);
            let inner = RECT { left: rc.left + 2, top: rc.top + 2, right: rc.right - 2, bottom: rc.bottom - 2 };
            FillRect(hdc, &inner, key_br);
            let _ = DeleteObject(key_br.into());
            let _ = DeleteObject(border_br.into());
            let _ = EndPaint(hwnd, &ps);
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe extern "system" fn hud_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut ScrollState;
    if ptr.is_null() { return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }; }
    let st = unsafe { &mut *ptr };
    match msg {
        WM_ERASEBKGND => LRESULT(1),
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
            paint_hud(st);
            unsafe {
                let _ = BitBlt(hdc, 0, 0, HUD_W, HUD_H, Some(st.mem_dc), 0, 0, SRCCOPY);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { let _ = DestroyWindow(st.frame_hwnd); PostQuitMessage(0); }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn paint_hud(st: &ScrollState) {
    let dst = unsafe { std::slice::from_raw_parts_mut(st.dib_bits, (HUD_W * HUD_H * 4) as usize) };
    for px in dst.chunks_exact_mut(4) { px[0] = 0x20; px[1] = 0x18; px[2] = 0x16; px[3] = 255; }
    draw_capsule(dst, HUD_W, HUD_H, ScreenRect { x: 0, y: 0, w: HUD_W, h: HUD_H }, 10.0);
    for b in &st.btns {
        let hov = st.hovered == Some(b.cmd);
        let bg = if b.accent {
            if hov { AnnotColor(0x1D, 0x4E, 0xD8) } else { AnnotColor(0x25, 0x63, 0xEB) }
        } else if b.active { AnnotColor(0x23, 0x35, 0x54) }
        else if hov { AnnotColor(0x2D, 0x32, 0x42) }
        else { AnnotColor(0x22, 0x25, 0x32) };
        fill_rounded_rect_aa(dst, HUD_W, HUD_H, b.rect, 6.0, bg, 1.0);
        if b.active && !b.accent {
            stroke_rounded_rect_aa(dst, HUD_W, HUD_H, b.rect, 6.0, 1.0, AnnotColor(0x60, 0xA5, 0xFA), 0.75);
        }
    }
    unsafe {
        let old = SelectObject(st.mem_dc, st.font.into());
        SetBkMode(st.mem_dc, TRANSPARENT);
        let axis_name = if st.canvas.axis == ScrollAxis::Vertical { "纵向长截图" } else { "横向长截图" };
        let hint = if st.auto_scroll { "自动滚动拼接中…" } else { "滚动目标窗口 或点「自动滚动」" };
        let head = format!("{axis_name} · {} 帧 · {}×{}  |  {hint}", st.canvas.frames, st.canvas.width, st.canvas.height);
        SetTextColor(st.mem_dc, COLORREF(0x00E2_E8F0));
        let mut hrc = RECT { left: 12, top: 8, right: HUD_W - 12, bottom: 30 };
        let mut hw: Vec<u16> = head.encode_utf16().collect();
        DrawTextW(st.mem_dc, &mut hw, &mut hrc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        for b in &st.btns {
            SetTextColor(st.mem_dc, COLORREF(if b.accent { 0x00FF_FFFF } else { 0x00F1_F5F9 }));
            let mut rc = RECT { left: b.rect.x, top: b.rect.y, right: b.rect.right(), bottom: b.rect.bottom() };
            let mut w: Vec<u16> = b.label.encode_utf16().collect();
            DrawTextW(st.mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        let _ = SelectObject(st.mem_dc, old);
    }
}

fn send_scroll_step(cx: i32, cy: i32, axis: ScrollAxis) {
    unsafe {
        let _ = SetCursorPos(cx, cy);
        let (flags, data) = match axis {
            ScrollAxis::Vertical => (MOUSEEVENTF_WHEEL, (-360i32) as u32),
            ScrollAxis::Horizontal => (MOUSEEVENTF_HWHEEL, 360u32),
        };
        let inp = [INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { mouseData: data, dwFlags: flags, ..Default::default() } },
        }];
        SendInput(&inp, std::mem::size_of::<INPUT>() as i32);
    }
}

fn capture_region_bgra(x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    if w <= 0 || h <= 0 { return None; }
    unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() { return None; }
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let old = SelectObject(mem_dc, bmp.into());
        let ok = BitBlt(mem_dc, 0, 0, w, h, Some(screen_dc), x, y, SRCCOPY | CAPTUREBLT).is_ok()
            || BitBlt(mem_dc, 0, 0, w, h, Some(screen_dc), x, y, SRCCOPY).is_ok();
        let out = if ok {
            let mut v = std::slice::from_raw_parts(bits as *const u8, (w as usize) * (h as usize) * 4).to_vec();
            for px in v.chunks_exact_mut(4) { px[3] = 255; }
            Some(v)
        } else { None };
        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        out
    }
}
