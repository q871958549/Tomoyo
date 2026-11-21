//! Win32 DWM titlebar theme sync, WebView2 virtual-host folder mapping, and file URL helpers.

use std::ffi::c_void;
use std::path::PathBuf;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_3, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, SetWindowPos, HICON, IMAGE_ICON, LR_SHARED, SM_CXICON,
    SM_CXSMICON, SM_CYICON, SM_CYSMICON, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER,
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
