//! Native borderless topmost Pin-to-Desktop (`贴图`) window with wheel zoom, layered opacity,
//! copy/save/OCR/translate right-click menu, and strictly zero GDI handle leaks.

#![cfg(windows)]

use crate::export::{
    copy_bgra_to_clipboard, default_save_path, handoff_to_ocr_or_translate, save_bgra_as_png,
};
use tomoyo_core::TomoyoConfig;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FrameRect, InvalidateRect,
    SetStretchBltMode, StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    HALFTONE, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    LoadCursorW, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetLayeredWindowAttributes,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TrackPopupMenu, TranslateMessage, CS_DBLCLKS,
    GWLP_USERDATA, HTCAPTION, HWND_NOTOPMOST, HWND_TOPMOST, IDC_SIZEALL, LWA_ALPHA, MF_CHECKED,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_SHOW, TPM_LEFTALIGN, TPM_RETURNCMD, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDBLCLK, WM_MOUSEWHEEL, WM_NCHITTEST, WM_NCRBUTTONUP, WM_PAINT, WM_RBUTTONUP,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const CMD_COPY: u32 = 1;
const CMD_SAVE: u32 = 2;
const CMD_OCR: u32 = 3;
const CMD_TRANSLATE: u32 = 4;
const CMD_TOPMOST: u32 = 5;
const CMD_RESET: u32 = 6;
const CMD_CLOSE: u32 = 7;

struct PinState {
    img_w: u32,
    img_h: u32,
    bgra: Vec<u8>,
    scale: f32,
    opacity_pct: u8,
    topmost: bool,
    save_dir: String,
}

impl PinState {
    fn apply_opacity(&self, hwnd: HWND) {
        let alpha = ((self.opacity_pct as u16 * 255) / 100).clamp(50, 255) as u8;
        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
    }

    fn apply_zoom(&self, hwnd: HWND) {
        let mut rc = RECT::default();
        unsafe { let _ = GetWindowRect(hwnd, &mut rc); }
        let cx = (rc.left + rc.right) / 2;
        let cy = (rc.top + rc.bottom) / 2;
        let nw = ((self.img_w as f32) * self.scale).round().clamp(48.0, 4096.0) as i32;
        let nh = ((self.img_h as f32) * self.scale).round().clamp(48.0, 4096.0) as i32;
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                cx - nw / 2,
                cy - nh / 2,
                nw,
                nh,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }

    fn show_context_menu(&mut self, hwnd: HWND) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let _ = AppendMenuW(menu, MF_STRING, CMD_COPY as usize, w!("复制图片 (Ctrl+C)"));
            let _ = AppendMenuW(menu, MF_STRING, CMD_SAVE as usize, w!("保存为 PNG (Ctrl+S)"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CMD_OCR as usize, w!("OCR 识图取字 (O)"));
            let _ = AppendMenuW(menu, MF_STRING, CMD_TRANSLATE as usize, w!("截图翻译 (L)"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let top_flag = if self.topmost { MF_STRING | MF_CHECKED } else { MF_STRING | MF_UNCHECKED };
            let _ = AppendMenuW(menu, top_flag, CMD_TOPMOST as usize, w!("窗口置顶 (Topmost)"));
            let _ = AppendMenuW(menu, MF_STRING, CMD_RESET as usize, w!("重置 100% 尺寸与不透明度"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CMD_CLOSE as usize, w!("关闭贴图 (Esc)"));

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_RETURNCMD, pt.x, pt.y, Some(0), hwnd, None).0 as u32;
            let _ = DestroyMenu(menu);

            match cmd {
                CMD_COPY => { let _ = copy_bgra_to_clipboard(self.img_w, self.img_h, &self.bgra); }
                CMD_SAVE => {
                    let path = default_save_path(&self.save_dir);
                    let _ = save_bgra_as_png(&path, self.img_w, self.img_h, &self.bgra);
                }
                CMD_OCR => { let _ = handoff_to_ocr_or_translate(self.img_w, self.img_h, &self.bgra, false); }
                CMD_TRANSLATE => { let _ = handoff_to_ocr_or_translate(self.img_w, self.img_h, &self.bgra, true); }
                CMD_TOPMOST => {
                    self.topmost = !self.topmost;
                    let after = if self.topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
                    let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
                CMD_RESET => {
                    self.scale = 1.0;
                    self.opacity_pct = 100;
                    self.apply_opacity(hwnd);
                    self.apply_zoom(hwnd);
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

        let mut st = Box::new(PinState {
            img_w: width,
            img_h: height,
            bgra,
            scale: 1.0,
            opacity_pct: cfg.capture.pin_opacity.clamp(30, 100),
            topmost: true,
            save_dir: cfg.capture.save_dir,
        });

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            cls,
            w!("Tomoyo Pin"),
            WS_POPUP,
            screen_x,
            screen_y,
            width as i32,
            height as i32,
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
        WM_NCHITTEST => LRESULT(HTCAPTION as isize),
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
                let brush = CreateSolidBrush(COLORREF(0x00F6_823B)); // crisp accent 1px frame
                let _ = FrameRect(hdc, &rc, brush);
                let _ = DeleteObject(brush.into());
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
                let factor = if delta > 0 { 1.1 } else { 0.9 };
                st.scale = (st.scale * factor).clamp(0.25, 4.0);
                st.apply_zoom(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let _ = copy_bgra_to_clipboard(st.img_w, st.img_h, &st.bgra);
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
                let _ = copy_bgra_to_clipboard(st.img_w, st.img_h, &st.bgra);
            } else if ctrl && vk == b'S' as u16 {
                let path = default_save_path(&st.save_dir);
                let _ = save_bgra_as_png(&path, st.img_w, st.img_h, &st.bgra);
            } else if vk == b'O' as u16 {
                let _ = handoff_to_ocr_or_translate(st.img_w, st.img_h, &st.bgra, false);
            } else if vk == b'L' as u16 {
                let _ = handoff_to_ocr_or_translate(st.img_w, st.img_h, &st.bgra, true);
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
