//! Long-screenshot HUD painting, hollow capture frame WndProc, synthetic wheel & BitBlt capture.

#![cfg(windows)]

use crate::annotate::AnnotColor;
use crate::overlay_icons::{draw_capsule, fill_rounded_rect_aa, stroke_rounded_rect_aa};
use crate::screen::ScreenRect;
use crate::stitch::{ScrollAxis, StitchCanvas};
use std::ffi::c_void;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, DT_CENTER,
    DT_LEFT, DT_SINGLELINE, DT_VCENTER, HDC, HFONT, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, GetClientRect, SetCursorPos, WM_PAINT};

pub(crate) const HUD_W: i32 = 436;
pub(crate) const HUD_H: i32 = 76;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HudCmd {
    ToggleAxis,
    ToggleAuto,
    Save,
    Pin,
    Copy,
    Cancel,
}

#[derive(Clone, Copy)]
pub(crate) struct HudBtn {
    pub cmd: HudCmd,
    pub rect: ScreenRect,
    pub label: &'static str,
    pub accent: bool,
    pub active: bool,
}

pub(crate) fn build_hud_buttons(axis: ScrollAxis, auto_scroll: bool) -> Vec<HudBtn> {
    let axis_lbl = if axis == ScrollAxis::Vertical { "纵向模式" } else { "横向模式" };
    let auto_lbl = if auto_scroll { "暂停自动" } else { "自动滚动" };
    let y = 38;
    vec![
        HudBtn { cmd: HudCmd::ToggleAxis, rect: ScreenRect { x: 10, y, w: 70, h: 28 }, label: axis_lbl, accent: false, active: true },
        HudBtn { cmd: HudCmd::ToggleAuto, rect: ScreenRect { x: 86, y, w: 70, h: 28 }, label: auto_lbl, accent: false, active: auto_scroll },
        HudBtn { cmd: HudCmd::Save, rect: ScreenRect { x: 166, y, w: 54, h: 28 }, label: "保存", accent: false, active: false },
        HudBtn { cmd: HudCmd::Pin, rect: ScreenRect { x: 226, y, w: 54, h: 28 }, label: "贴图", accent: false, active: false },
        HudBtn { cmd: HudCmd::Copy, rect: ScreenRect { x: 286, y, w: 96, h: 28 }, label: "复制完成", accent: true, active: false },
        HudBtn { cmd: HudCmd::Cancel, rect: ScreenRect { x: 388, y, w: 38, h: 28 }, label: "✕", accent: false, active: false },
    ]
}

pub(crate) unsafe extern "system" fn frame_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
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

pub(crate) fn paint_hud_canvas(
    dib_bits: *mut u8,
    mem_dc: HDC,
    font: HFONT,
    canvas: &StitchCanvas,
    auto_scroll: bool,
    btns: &[HudBtn],
    hovered: Option<HudCmd>,
) {
    let dst = unsafe { std::slice::from_raw_parts_mut(dib_bits, (HUD_W * HUD_H * 4) as usize) };
    for px in dst.as_chunks_mut::<4>().0 { px[0] = 0x20; px[1] = 0x18; px[2] = 0x16; px[3] = 255; }
    draw_capsule(dst, HUD_W, HUD_H, ScreenRect { x: 0, y: 0, w: HUD_W, h: HUD_H }, 10.0);
    for b in btns {
        let hov = hovered == Some(b.cmd);
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
        let old = SelectObject(mem_dc, font.into());
        SetBkMode(mem_dc, TRANSPARENT);
        let axis_name = if canvas.axis == ScrollAxis::Vertical { "纵向长截图" } else { "横向长截图" };
        let hint = if auto_scroll { "自动滚动拼接中…" } else { "滚动目标窗口 或点「自动滚动」" };
        let head = format!("{axis_name} · {} 帧 · {}×{}  |  {hint}", canvas.frames, canvas.width, canvas.height);
        SetTextColor(mem_dc, COLORREF(0x00E2_E8F0));
        let mut hrc = RECT { left: 12, top: 8, right: HUD_W - 12, bottom: 30 };
        let mut hw: Vec<u16> = head.encode_utf16().collect();
        DrawTextW(mem_dc, &mut hw, &mut hrc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        for b in btns {
            SetTextColor(mem_dc, COLORREF(if b.accent { 0x00FF_FFFF } else { 0x00F1_F5F9 }));
            let mut rc = RECT { left: b.rect.x, top: b.rect.y, right: b.rect.right(), bottom: b.rect.bottom() };
            let mut w: Vec<u16> = b.label.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        let _ = SelectObject(mem_dc, old);
    }
}

pub(crate) fn send_scroll_step(cx: i32, cy: i32, axis: ScrollAxis) {
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

pub(crate) fn capture_region_bgra(x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
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
            for px in v.as_chunks_mut::<4>().0 { px[3] = 255; }
            Some(v)
        } else { None };
        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        out
    }
}
