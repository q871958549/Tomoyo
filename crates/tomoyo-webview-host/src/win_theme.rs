//! Win32 DWM titlebar theme sync, WebView2 virtual-host folder mapping, and file URL helpers.

use std::ffi::c_void;
use std::path::PathBuf;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller, ICoreWebView2Controller2, ICoreWebView2_3, COREWEBVIEW2_COLOR,
    COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
};
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetSystemMetrics, KillTimer, LoadCursorW, LoadImageW, RegisterClassExW,
    SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, HICON,
    HWND_NOTOPMOST, HWND_TOPMOST, ICON_BIG, ICON_SMALL, IDC_ARROW, IMAGE_ICON, LR_SHARED,
    SM_CXICON, SM_CXSCREEN, SM_CXSMICON, SM_CYICON, SM_CYSCREEN, SM_CYSMICON, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOW, WM_SETICON, WNDCLASSEXW,
    WNDPROC, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

/// Loads the embedded Tomoyo application logo (`RT_GROUP_ICON` ID `1` from `winresource`)
/// at both standard taskbar (`SM_CXICON`) and titlebar (`SM_CXSMICON`) DPI-matched sizes.
pub fn load_embedded_window_icons(hinst: HMODULE) -> (Option<HICON>, Option<HICON>) {
    let res_id_1 = PCWSTR(std::ptr::without_provenance::<u16>(1));
    unsafe {
        let big = LoadImageW(
            Some(hinst.into()),
            res_id_1,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXICON),
            GetSystemMetrics(SM_CYICON),
            LR_SHARED,
        )
        .ok()
        .map(|h| HICON(h.0));
        let small = LoadImageW(
            Some(hinst.into()),
            res_id_1,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_SHARED,
        )
        .ok()
        .map(|h| HICON(h.0))
        .or(big);
        (big, small)
    }
}

pub fn theme_canvas_rgb(theme: &str) -> (u8, u8, u8) {
    match theme.trim().to_ascii_lowercase().as_str() {
        "light" | "white" => (0xF6, 0xF8, 0xFC),
        "midnight" => (0x0A, 0x0F, 0x1A),
        _ => (0x13, 0x14, 0x1C),
    }
}

pub fn create_theme_bg_brush(theme: &str) -> HBRUSH {
    let (r, g, b) = theme_canvas_rgb(theme);
    unsafe { CreateSolidBrush(COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))) }
}

pub unsafe fn configure_controller_visuals(controller: &ICoreWebView2Controller, theme: &str) {
    let (r, g, b) = theme_canvas_rgb(theme);
    if let Ok(ctrl2) = controller.cast::<ICoreWebView2Controller2>() {
        let _ = unsafe {
            ctrl2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: r, G: g, B: b })
        };
    }
}

pub unsafe fn reveal_host_window_and_notify(
    hwnd: HWND,
    fallback_timer_id: usize,
    notify_hwnd: Option<isize>,
) {
    unsafe {
        let _ = KillTimer(Some(hwnd), fallback_timer_id);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
    if let Some(target) = notify_hwnd {
        crate::bridge_os::notify_parent_mode(target, 6);
    }
}

pub unsafe fn toggle_pin_topmost_window(hwnd: HWND, base_title: &str, is_topmost: &mut bool) {
    *is_topmost = !*is_topmost;
    let insert_after = if *is_topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    unsafe {
        let _ = SetWindowPos(hwnd, Some(insert_after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        let new_title = if *is_topmost {
            format!("📌 {base_title}")
        } else {
            base_title.to_string()
        };
        let tw = HSTRING::from(new_title);
        let _ = SetWindowTextW(hwnd, PCWSTR(tw.as_ptr()));
    }
}

pub unsafe fn create_themed_host_window(
    cls: PCWSTR,
    wndproc: WNDPROC,
    title: &str,
    theme: &str,
    width: i32,
    height: i32,
    visible_initially: bool,
) -> HWND {
    unsafe {
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
        let (big_icon, small_icon) = load_embedded_window_icons(hinst);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: wndproc,
            hInstance: hinst.into(),
            hIcon: big_icon.unwrap_or_default(),
            hIconSm: small_icon.unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: create_theme_bg_brush(theme),
            lpszClassName: cls,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
        let x = ((GetSystemMetrics(SM_CXSCREEN) - width) / 2).max(40);
        let y = ((GetSystemMetrics(SM_CYSCREEN) - height) / 2).max(40);
        let title_w = HSTRING::from(title);
        let style = if visible_initially { WS_OVERLAPPEDWINDOW | WS_VISIBLE } else { WS_OVERLAPPEDWINDOW };
        let hwnd = CreateWindowExW(
            Default::default(), cls, PCWSTR(title_w.as_ptr()),
            style, x, y, width, height,
            None, None, Some(hinst.into()), None,
        )
        .expect("CreateWindowExW failed");
        if let Some(ic) = small_icon {
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM_SETICON_SMALL), Some(windows::Win32::Foundation::LPARAM(ic.0 as isize)));
        }
        if let Some(ic) = big_icon {
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM_SETICON_BIG), Some(windows::Win32::Foundation::LPARAM(ic.0 as isize)));
        }
        apply_host_window_theme(hwnd, theme);
        hwnd
    }
}

const WPARAM_SETICON_SMALL: windows::Win32::Foundation::WPARAM = windows::Win32::Foundation::WPARAM(ICON_SMALL as usize);
const WPARAM_SETICON_BIG: windows::Win32::Foundation::WPARAM = windows::Win32::Foundation::WPARAM(ICON_BIG as usize);

pub fn apply_host_window_theme(hwnd: HWND, theme: &str) {
    let (is_dark, caption_bgr, text_bgr) = match theme.trim().to_ascii_lowercase().as_str() {
        "light" | "white" => (0i32, COLORREF(0x00FBF7F6), COLORREF(0x00221815)),
        "midnight" => (1i32, COLORREF(0x001A100E), COLORREF(0x00F9F5F1)),
        _ => (1i32, COLORREF(0x001C1413), COLORREF(0x00F8F4F3)),
    };

    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &is_dark as *const _ as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &caption_bgr as *const _ as *const c_void,
            std::mem::size_of::<COLORREF>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            &text_bgr as *const _ as *const c_void,
            std::mem::size_of::<COLORREF>() as u32,
        );
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

pub unsafe fn map_vdir(wv3: &ICoreWebView2_3, host: PCWSTR, dir: Option<&PathBuf>) {
    if let Some(d) = dir {
        let dir_w = HSTRING::from(d.to_string_lossy().as_ref());
        let _ = unsafe {
            wv3.SetVirtualHostNameToFolderMapping(
                host,
                PCWSTR(dir_w.as_ptr()),
                COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
            )
        };
    }
}

pub fn to_file_url(entry: &str) -> String {
    if entry.starts_with("http://") || entry.starts_with("https://") || entry.starts_with("file://") {
        return entry.to_string();
    }
    let abs = std::fs::canonicalize(PathBuf::from(entry)).unwrap_or_else(|_| PathBuf::from(entry));
    let s = abs.to_string_lossy();
    let cleaned = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    format!("file:///{cleaned}")
}
