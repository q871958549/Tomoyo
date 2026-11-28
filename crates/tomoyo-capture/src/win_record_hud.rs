//! Floating Win32 recording border & control/completion capsule HUD (`WDA_EXCLUDEFROMCAPTURE`).

#![cfg(windows)]

use crate::screen::ScreenRect;
use crate::win_record::SharedRecState;
use crate::win_record_hud_draw::{
    build_rec_hud_buttons, copy_file_to_clipboard, paint_rec_hud, HudButton, HudCmd, HUD_H,
    HUD_W_DONE, HUD_W_ERR, HUD_W_REC,
};
use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC,
    DeleteObject, EndPaint, FillRect, InvalidateRect, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    FF_DONTCARE, FW_NORMAL, HBITMAP, HDC, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, VK_ESCAPE, VK_RETURN, VK_SPACE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassW,
    SendMessageW, SetCursor, SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, GWLP_USERDATA, HTCAPTION,
    IDC_ARROW, IDC_HAND, LWA_COLORKEY, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW,
    SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_NCLBUTTONDOWN, WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const TIMER_HUD_TICK: usize = 801;
const WAIT_OBJECT_0_U32: u32 = 0;

struct RecHudState {
    rect: ScreenRect,
    shared: Arc<SharedRecState>,
    worker: Option<JoinHandle<()>>,
    stop_event: HANDLE,
    frame_hwnd: HWND,
    frame_paused: bool,
    win_w: i32,
    resized_mode: u8,
    hovered: Option<usize>,
    tick_phase: u32,
    active_ms: u64,
    last_tick: Instant,
    buttons: Vec<HudButton>,
    mem_dc: HDC,
    dib_bmp: HBITMAP,
    dib_bits: *mut u8,
    font: HFONT,
}

impl Drop for RecHudState {
    fn drop(&mut self) {
        unsafe {
            if !self.mem_dc.is_invalid() { let _ = DeleteDC(self.mem_dc); }
            if !self.dib_bmp.is_invalid() { let _ = DeleteObject(self.dib_bmp.into()); }
            if !self.font.is_invalid() { let _ = DeleteObject(self.font.into()); }
        }
    }
}

impl RecHudState {
    fn rebuild_buttons(&mut self) {
        self.buttons = build_rec_hud_buttons(self.win_w, self.shared.mode.load(Ordering::Acquire));
    }

    fn hide_border(&mut self) {
        if !self.frame_hwnd.is_invalid() {
            unsafe { let _ = DestroyWindow(self.frame_hwnd); }
            self.frame_hwnd = HWND::default();
        }
    }

    fn begin_finish(&mut self, hwnd: HWND) {
        if self.shared.mode.load(Ordering::Acquire) >= 2 { return; }
        self.shared.mode.store(2, Ordering::Release);
        self.shared.stop_requested.store(true, Ordering::Release);
        self.hide_border();
        self.rebuild_buttons();
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    }

    fn resize_dib(&mut self, new_w: i32) {
        if new_w == self.win_w { return; }
        unsafe {
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: new_w, biHeight: -HUD_H, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            if let Ok(bmp) = CreateDIBSection(Some(self.mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                && !bmp.is_invalid() && !bits.is_null()
            {
                let _ = SelectObject(self.mem_dc, bmp.into());
                if !self.dib_bmp.is_invalid() { let _ = DeleteObject(self.dib_bmp.into()); }
                self.dib_bmp = bmp;
                self.dib_bits = bits as *mut u8;
                self.win_w = new_w;
            }
        }
    }
}

pub(crate) fn run_recording_hud_loop(
    rect: ScreenRect,
    shared: Arc<SharedRecState>,
    worker: Option<JoinHandle<()>>,
    stop_event: HANDLE,
) {
    unsafe {
        let Ok(hinst) = GetModuleHandleW(None) else { return; };
        let frame_cls = w!("TomoyoRecBorderClass");
        let _ = RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(rec_border_wndproc), hInstance: hinst.into(), lpszClassName: frame_cls,
            ..Default::default()
        });
        let hud_cls = w!("TomoyoRecHudClass");
        let _ = RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(rec_hud_wndproc), hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), lpszClassName: hud_cls,
            ..Default::default()
        });

        let pad = 3;
        let frame_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            frame_cls, w!(""), WS_POPUP,
            rect.x - pad, rect.y - pad, rect.w + pad * 2, rect.h + pad * 2,
            None, None, Some(hinst.into()), None,
        ).unwrap_or_default();
        if !frame_hwnd.is_invalid() {
            SetWindowLongPtrW(frame_hwnd, GWLP_USERDATA, 0);
            let _ = SetLayeredWindowAttributes(frame_hwnd, COLORREF(0x00FF_00FF), 0, LWA_COLORKEY);
            let _ = SetWindowDisplayAffinity(frame_hwnd, WDA_EXCLUDEFROMCAPTURE);
            let _ = ShowWindow(frame_hwnd, SW_SHOWNOACTIVATE);
        }

        let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        let hud_x = (rect.x + (rect.w - HUD_W_REC) / 2).clamp(vx + 8, (vx + vw - HUD_W_REC - 8).max(vx + 8));
        let hud_y = if rect.bottom() + 10 + HUD_H <= vy + vh - 8 {
            rect.bottom() + 10
        } else if rect.y - 10 - HUD_H >= vy + 8 {
            rect.y - 10 - HUD_H
        } else {
            vy + 14
        };

        let mem_dc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: HUD_W_REC, biHeight: -HUD_H, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let Ok(dib_bmp) = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(mem_dc);
            if !frame_hwnd.is_invalid() { let _ = DestroyWindow(frame_hwnd); }
            return;
        };
        let _ = SelectObject(mem_dc, dib_bmp.into());
        let face: Vec<u16> = "Microsoft YaHei UI".encode_utf16().chain(std::iter::once(0)).collect();
        let font = CreateFontW(
            -13, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(face.as_ptr()),
        );

        let mut st = Box::new(RecHudState {
            rect, shared: Arc::clone(&shared), worker, stop_event, frame_hwnd, frame_paused: false,
            win_w: HUD_W_REC, resized_mode: 0, hovered: None, tick_phase: 0, active_ms: 0,
            last_tick: Instant::now(), buttons: Vec::new(), mem_dc, dib_bmp,
            dib_bits: bits as *mut u8, font,
        });
        st.rebuild_buttons();

        let Ok(hud_hwnd) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW, hud_cls, w!("Tomoyo 录屏控制"), WS_POPUP,
            hud_x, hud_y, HUD_W_REC, HUD_H, None, None, Some(hinst.into()), None,
        ) else {
            st.hide_border();
            shared.discard_on_stop.store(true, Ordering::Release);
            shared.stop_requested.store(true, Ordering::Release);
            if let Some(w) = st.worker.take() { let _ = w.join(); }
            return;
        };
        SetWindowLongPtrW(hud_hwnd, GWLP_USERDATA, &mut *st as *mut RecHudState as isize);
        let _ = SetWindowDisplayAffinity(hud_hwnd, WDA_EXCLUDEFROMCAPTURE);
        let _ = ShowWindow(hud_hwnd, SW_SHOWNOACTIVATE);
        let _ = InvalidateRect(Some(hud_hwnd), None, false);
        SetTimer(Some(hud_hwnd), TIMER_HUD_TICK, 120, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        st.hide_border();
        shared.stop_requested.store(true, Ordering::Release);
        if let Some(w) = st.worker.take() { let _ = w.join(); }
    }
}

unsafe extern "system" fn rec_border_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let mut ps = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rc);
            let paused = GetWindowLongPtrW(hwnd, GWLP_USERDATA) != 0;
            let col = if paused { COLORREF(0x000B_9EF5) } else { COLORREF(0x0044_44EF) };
            let key_br = CreateSolidBrush(COLORREF(0x00FF_00FF));
            let border_br = CreateSolidBrush(col);
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

unsafe extern "system" fn rec_hud_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut RecHudState;
    if ptr.is_null() { return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }; }
    let st = unsafe { &mut *ptr };
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_SETCURSOR => {
            if let Ok(h) = unsafe { LoadCursorW(None, if st.hovered.is_some() { IDC_HAND } else { IDC_ARROW }) } {
                unsafe { SetCursor(Some(h)); }
            }
            LRESULT(1)
        }
        WM_TIMER if wparam.0 == TIMER_HUD_TICK => {
            let now = Instant::now();
            let dt = now.saturating_duration_since(st.last_tick).as_millis() as u64;
            st.last_tick = now;
            st.tick_phase = st.tick_phase.wrapping_add(1);
            if !st.stop_event.is_invalid() && unsafe { WaitForSingleObject(st.stop_event, 0) }.0 == WAIT_OBJECT_0_U32 {
                st.begin_finish(hwnd);
            }
            let m = st.shared.mode.load(Ordering::Acquire);
            if m == 0 { st.active_ms = st.active_ms.saturating_add(dt.min(500)); }
            let paused = m == 1;
            if paused != st.frame_paused && !st.frame_hwnd.is_invalid() {
                st.frame_paused = paused;
                unsafe {
                    SetWindowLongPtrW(st.frame_hwnd, GWLP_USERDATA, isize::from(paused));
                    let _ = InvalidateRect(Some(st.frame_hwnd), None, false);
                }
            }
            if (m == 3 || m == 4) && st.resized_mode != m {
                st.resized_mode = m;
                st.hide_border();
                st.resize_dib(if m == 3 { HUD_W_DONE } else { HUD_W_ERR });
                st.rebuild_buttons();
                unsafe { let _ = SetWindowPos(hwnd, None, 0, 0, st.win_w, HUD_H, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE); }
            }
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);
            let h = st.buttons.iter().position(|(_, r, _, _, _)| r.contains(x, y));
            if h != st.hovered {
                st.hovered = h;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);
            let Some(idx) = st.buttons.iter().position(|(_, r, _, _, _)| r.contains(x, y)) else {
                unsafe {
                    let _ = ReleaseCapture();
                    SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(HTCAPTION as usize)), Some(LPARAM(0)));
                }
                return LRESULT(0);
            };
            handle_hud_cmd(st, hwnd, st.buttons[idx].0);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            if vk == VK_ESCAPE.0 { handle_hud_cmd(st, hwnd, HudCmd::Cancel); }
            else if vk == VK_RETURN.0 && st.shared.mode.load(Ordering::Acquire) <= 1 { st.begin_finish(hwnd); }
            else if vk == VK_SPACE.0 && st.shared.mode.load(Ordering::Acquire) <= 1 { handle_hud_cmd(st, hwnd, HudCmd::PauseResume); }
            LRESULT(0)
        }
        WM_PAINT => {
            paint_rec_hud(hwnd, st.dib_bits, st.mem_dc, st.font, st.win_w, st.rect, &st.shared, &st.buttons, st.hovered, st.tick_phase, st.active_ms);
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { let _ = KillTimer(Some(hwnd), TIMER_HUD_TICK); PostQuitMessage(0); }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn handle_hud_cmd(st: &mut RecHudState, hwnd: HWND, cmd: HudCmd) {
    match cmd {
        HudCmd::PauseResume => {
            let cur = st.shared.mode.load(Ordering::Acquire);
            st.shared.mode.store(if cur == 0 { 1 } else { 0 }, Ordering::Release);
            st.rebuild_buttons();
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
        }
        HudCmd::Finish => st.begin_finish(hwnd),
        HudCmd::Cancel => {
            if st.shared.mode.load(Ordering::Acquire) < 2 {
                st.shared.discard_on_stop.store(true, Ordering::Release);
                st.shared.stop_requested.store(true, Ordering::Release);
            }
            unsafe { let _ = DestroyWindow(hwnd); }
        }
        HudCmd::CopyFile => {
            if let Ok(fp) = st.shared.final_path.lock()
                && let Some(p) = fp.as_ref()
                && copy_file_to_clipboard(p).is_ok()
            {
                st.shared.copied_flash.store(true, Ordering::Release);
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
        }
        HudCmd::Play => {
            if let Ok(fp) = st.shared.final_path.lock() && let Some(p) = fp.as_ref() {
                let wide: Vec<u16> = p.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
                unsafe { ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOW); }
            }
        }
        HudCmd::Reveal => {
            if let Ok(fp) = st.shared.final_path.lock() && let Some(p) = fp.as_ref() {
                let _ = tomoyo_core::reveal_file_in_file_manager(&p.to_string_lossy());
            }
        }
    }
}
