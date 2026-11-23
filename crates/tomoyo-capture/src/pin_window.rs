//! Native borderless topmost Pin-to-Desktop (`贴图`) window with wheel zoom, layered opacity,
//! copy/save/OCR/translate right-click menu, and strictly zero GDI handle leaks.

#![cfg(windows)]

use crate::export::{
    copy_bgra_to_clipboard, copy_text_to_clipboard, default_save_path, handoff_to_ocr_or_translate,
    save_bgra_as_png,
};
use crate::pin_clipboard::{
    clamp_pin_origin, compute_pin_zoom_origin, handoff_text_to_translator, virtual_desktop_rect,
};
use tomoyo_core::TomoyoConfig;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FrameRect, GetMonitorInfoW,
    InvalidateRect, MonitorFromPoint, SetStretchBltMode, StretchDIBits, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HALFTONE, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetClientRect, GetCursorPos, GetMessageW, GetSystemMetrics,
    GetWindowLongPtrW, GetWindowRect, LoadCursorW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TrackPopupMenu, TranslateMessage, CS_DBLCLKS, GWLP_USERDATA, HWND_NOTOPMOST, HWND_TOPMOST,
    IDC_SIZEALL, LWA_ALPHA, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOW, TPM_LEFTALIGN,
    TPM_RETURNCMD, WM_CAPTURECHANGED, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCLBUTTONDBLCLK,
    WM_NCMBUTTONUP, WM_NCRBUTTONUP, WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const CMD_COPY: u32 = 1;
const CMD_COPY_IMG: u32 = 8;
const CMD_SAVE: u32 = 2;
const CMD_OCR: u32 = 3;
const CMD_TRANSLATE: u32 = 4;
const CMD_TOPMOST: u32 = 5;
const CMD_RESET: u32 = 6;
const CMD_CLOSE: u32 = 7;

#[derive(Default)]
struct DragState {
    active: bool,
    moved: bool,
    start_cursor: POINT,
    start_x: i32,
    start_y: i32,
}

struct PinState {
    img_w: u32,
    img_h: u32,
    bgra: Vec<u8>,
    source_text: Option<String>,
    scale: f32,
    opacity_pct: u8,
    topmost: bool,
    save_dir: String,
    drag: DragState,
}

impl PinState {
    fn apply_opacity(&self, hwnd: HWND) {
        let alpha = ((self.opacity_pct as u16 * 255) / 100).clamp(50, 255) as u8;
        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
    }

    fn apply_zoom(&self, hwnd: HWND, cursor: Option<POINT>) {
        let mut rc = RECT::default();
        unsafe { let _ = GetWindowRect(hwnd, &mut rc); }
        let nw = ((self.img_w as f32) * self.scale).round().clamp(48.0, 4096.0) as i32;
        let nh = ((self.img_h as f32) * self.scale).round().clamp(48.0, 4096.0) as i32;
        let (nx, ny) = compute_pin_zoom_origin(rc, nw, nh, cursor, virtual_desktop_rect());
        unsafe {
            let _ = SetWindowPos(hwnd, None, nx, ny, nw, nh, SWP_NOZORDER | SWP_NOACTIVATE);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }

    fn show_context_menu(&mut self, hwnd: HWND) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            if self.source_text.is_some() {
                let _ = AppendMenuW(menu, MF_STRING, CMD_COPY as usize, w!("复制原始文本 (Ctrl+C)"));
                let _ = AppendMenuW(menu, MF_STRING, CMD_COPY_IMG as usize, w!("复制为图片"));
            } else {
                let _ = AppendMenuW(menu, MF_STRING, CMD_COPY as usize, w!("复制图片 (Ctrl+C)"));
            }
            let _ = AppendMenuW(menu, MF_STRING, CMD_SAVE as usize, w!("保存为 PNG (Ctrl+S)"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            if self.source_text.is_none() {
                let _ = AppendMenuW(menu, MF_STRING, CMD_OCR as usize, w!("OCR 识图取字 (O)"));
            }
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                CMD_TRANSLATE as usize,
                if self.source_text.is_some() { w!("翻译原文 (L)") } else { w!("截图翻译 (L)") },
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let top_flag = if self.topmost { MF_STRING | MF_CHECKED } else { MF_STRING | MF_UNCHECKED };
            let _ = AppendMenuW(menu, top_flag, CMD_TOPMOST as usize, w!("窗口置顶 (Topmost)"));
            let _ = AppendMenuW(menu, MF_STRING, CMD_RESET as usize, w!("重置 100% 尺寸与不透明度"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CMD_CLOSE as usize, w!("关闭贴图 (双击 / 中键 / Esc)"));

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_RETURNCMD, pt.x, pt.y, Some(0), hwnd, None).0 as u32;
            let _ = DestroyMenu(menu);

            match cmd {
                CMD_COPY => {
                    if let Some(txt) = &self.source_text {
                        copy_text_to_clipboard(txt);
                    } else {
                        let _ = copy_bgra_to_clipboard(self.img_w, self.img_h, &self.bgra);
                    }
                }
                CMD_COPY_IMG => { let _ = copy_bgra_to_clipboard(self.img_w, self.img_h, &self.bgra); }
                CMD_SAVE => {
                    let path = default_save_path(&self.save_dir);
                    let _ = save_bgra_as_png(&path, self.img_w, self.img_h, &self.bgra);
                }
                CMD_OCR => { let _ = handoff_to_ocr_or_translate(self.img_w, self.img_h, &self.bgra, false); }
                CMD_TRANSLATE => {
                    if let Some(txt) = &self.source_text {
                        handoff_text_to_translator(txt);
                    } else {
                        let _ = handoff_to_ocr_or_translate(self.img_w, self.img_h, &self.bgra, true);
                    }
                }
                CMD_TOPMOST => {
                    self.topmost = !self.topmost;
                    let after = if self.topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
                    let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
                CMD_RESET => {
                    self.scale = 1.0;
                    self.opacity_pct = 100;
                    self.apply_opacity(hwnd);
                    self.apply_zoom(hwnd, None);
                }
                CMD_CLOSE => { let _ = DestroyWindow(hwnd); }
                _ => {}
            }
        }
    }
}

pub fn run_pin_window(
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    screen_x: i32,
    screen_y: i32,
) -> Result<(), String> {
    run_pin_window_with_text(width, height, bgra, screen_x, screen_y, None)
}

pub fn run_pin_window_with_text(
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    screen_x: i32,
    screen_y: i32,
    source_text: Option<String>,
) -> Result<(), String> {
    let cfg = TomoyoConfig::load_or_create();
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| format!("{e}"))?;
        let cls = w!("TomoyoPinWindowClass");
        let wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(pin_wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_SIZEALL).unwrap_or_default(),
            lpszClassName: cls,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        let mut cur = POINT::default();
        let _ = GetCursorPos(&mut cur);
        let anchor = if screen_x == i32::MIN { cur } else { POINT { x: screen_x, y: screen_y } };
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
            mi.rcWork
        } else {
            RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN).max(640), bottom: GetSystemMetrics(SM_CYSCREEN).max(480) }
        };
        let sw = (work.right - work.left).max(640) as f32;
        let sh = (work.bottom - work.top).max(480) as f32;
        let init_scale = ((sw * 0.82) / (width.max(1) as f32))
            .min((sh * 0.82) / (height.max(1) as f32))
            .clamp(0.15, 1.0);
        let init_w = ((width as f32) * init_scale).round().max(48.0) as i32;
        let init_h = ((height as f32) * init_scale).round().max(48.0) as i32;
        let raw_x = if screen_x == i32::MIN { cur.x - init_w / 2 } else { screen_x };
        let raw_y = if screen_y == i32::MIN { cur.y - init_h / 2 } else { screen_y };
        let px = raw_x.clamp(work.left + 16, (work.right - init_w - 16).max(work.left + 16));
        let py = raw_y.clamp(work.top + 16, (work.bottom - init_h - 16).max(work.top + 16));

        let mut st = Box::new(PinState {
            img_w: width,
            img_h: height,
            bgra,
            source_text,
            scale: init_scale,
            opacity_pct: cfg.capture.pin_opacity.clamp(30, 100),
            topmost: true,
            save_dir: cfg.capture.save_dir,
            drag: DragState::default(),
        });

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            cls,
            w!("Tomoyo Pin"),
            WS_POPUP,
            px,
            py,
            init_w,
            init_h,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .map_err(|e| format!("CreateWindowExW pin failed: {e}"))?;

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *st as *mut PinState as isize);
        st.apply_opacity(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = InvalidateRect(Some(hwnd), None, false);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

unsafe extern "system" fn pin_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PinState;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let st = unsafe { &mut *ptr };

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_LBUTTONDOWN => {
            let mut pt = POINT::default();
            let mut rc = RECT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
                let _ = GetWindowRect(hwnd, &mut rc);
                SetCapture(hwnd);
            }
            st.drag = DragState { active: true, moved: false, start_cursor: pt, start_x: rc.left, start_y: rc.top };
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if st.drag.active {
                let mut pt = POINT::default();
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                    let _ = GetWindowRect(hwnd, &mut rc);
                }
                let dx = pt.x - st.drag.start_cursor.x;
                let dy = pt.y - st.drag.start_cursor.y;
                if st.drag.moved || dx.abs() >= 2 || dy.abs() >= 2 {
                    st.drag.moved = true;
                    let (nx, ny) = clamp_pin_origin(
                        st.drag.start_x + dx,
                        st.drag.start_y + dy,
                        (rc.right - rc.left).max(1),
                        (rc.bottom - rc.top).max(1),
                        virtual_desktop_rect(),
                    );
                    unsafe {
                        let _ = SetWindowPos(hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            if st.drag.active {
                st.drag.active = false;
                if msg == WM_LBUTTONUP {
                    unsafe { let _ = ReleaseCapture(); }
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut rc = RECT::default();
            unsafe { let _ = GetClientRect(hwnd, &mut rc); }
            let cw = (rc.right - rc.left).max(1);
            let ch = (rc.bottom - rc.top).max(1);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: st.img_w as i32,
                    biHeight: -(st.img_h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            unsafe {
                let _ = SetStretchBltMode(hdc, HALFTONE);
                StretchDIBits(
                    hdc,
                    0,
                    0,
                    cw,
                    ch,
                    0,
                    0,
                    st.img_w as i32,
                    st.img_h as i32,
                    Some(st.bgra.as_ptr() as *const std::ffi::c_void),
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                let outer_brush = CreateSolidBrush(COLORREF(0x003B_291E));
                let _ = FrameRect(hdc, &rc, outer_brush);
                let _ = DeleteObject(outer_brush.into());
                if cw > 4 && ch > 4 {
                    let inner = RECT { left: 1, top: 1, right: cw - 1, bottom: ch - 1 };
                    let accent_brush = CreateSolidBrush(COLORREF(0x00F6_823B));
                    let _ = FrameRect(hdc, &inner, accent_brush);
                    let _ = DeleteObject(accent_brush.into());
                }
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
            let ctrl = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
            if ctrl {
                let next = if delta > 0 {
                    st.opacity_pct.saturating_add(10).min(100)
                } else {
                    st.opacity_pct.saturating_sub(10).max(20)
                };
                if next != st.opacity_pct {
                    st.opacity_pct = next;
                    st.apply_opacity(hwnd);
                }
            } else {
                let mut cur = POINT::default();
                unsafe { let _ = GetCursorPos(&mut cur); }
                let factor = if delta > 0 { 1.1 } else { 0.9 };
                st.scale = (st.scale * factor).clamp(0.25, 4.0);
                st.apply_zoom(hwnd, Some(cur));
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK | WM_NCLBUTTONDBLCLK | WM_MBUTTONUP | WM_NCMBUTTONUP => {
            unsafe { let _ = DestroyWindow(hwnd); }
            LRESULT(0)
        }
        WM_RBUTTONUP | WM_NCRBUTTONUP => {
            st.show_context_menu(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            let ctrl = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
            if vk == VK_ESCAPE.0 {
                unsafe { let _ = DestroyWindow(hwnd); }
            } else if ctrl && vk == b'C' as u16 {
                if let Some(txt) = &st.source_text {
                    copy_text_to_clipboard(txt);
                } else {
                    let _ = copy_bgra_to_clipboard(st.img_w, st.img_h, &st.bgra);
                }
            } else if ctrl && vk == b'S' as u16 {
                let path = default_save_path(&st.save_dir);
                let _ = save_bgra_as_png(&path, st.img_w, st.img_h, &st.bgra);
            } else if vk == b'O' as u16 {
                let _ = handoff_to_ocr_or_translate(st.img_w, st.img_h, &st.bgra, false);
            } else if vk == b'L' as u16 {
                if let Some(txt) = &st.source_text {
                    handoff_text_to_translator(txt);
                } else {
                    let _ = handoff_to_ocr_or_translate(st.img_w, st.img_h, &st.bgra, true);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0); }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
