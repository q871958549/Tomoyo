//! Win32 DWM titlebar theme sync, WebView2 virtual-host folder mapping, and file URL helpers.

use std::ffi::c_void;
use std::path::PathBuf;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller, ICoreWebView2Controller2, ICoreWebView2_3, COREWEBVIEW2_COLOR,
    COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
};
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, CreateSolidBrush, DeleteObject, FillRect, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBRUSH, HDC,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, CreateWindowExW, DestroyIcon, GetClientRect, GetSystemMetrics,
    GetWindowRect, IsZoomed, LoadCursorW, LoadImageW, RegisterClassExW, SendMessageW,
    SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, HICON, HTBOTTOM,
    HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
    HWND_NOTOPMOST, HWND_TOPMOST, ICONINFO, ICON_BIG, ICON_SMALL, IDC_ARROW, IMAGE_ICON,
    LR_SHARED, MINMAXINFO, NCCALCSIZE_PARAMS, SM_CXICON, SM_CXPADDEDBORDER, SM_CXSCREEN,
    SM_CXSIZEFRAME, SM_CXSMICON, SM_CYICON, SM_CYSCREEN, SM_CYSIZEFRAME, SM_CYSMICON,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOW, WM_SETICON,
    WNDCLASSEXW, WNDPROC, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
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

fn create_hicon_from_image(path: &str, size: i32) -> Option<HICON> {
    let sz = size.clamp(16, 128) as u32;
    let (_, _, dw, dh, rgba) =
        tomoyo_core::wic_thumb::decode_image_thumbnail_wic(std::path::Path::new(path), sz, sz)?;
    let resized = if dw != sz || dh != sz {
        tomoyo_core::wic_thumb::scale_rgba_bilinear(&rgba, dw, dh, sz, sz)
    } else {
        rgba
    };
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: sz as i32,
            biHeight: -(sz as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    unsafe {
        let color_bmp = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(color_bmp.into());
            return None;
        }
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (sz * sz * 4) as usize);
        for (src_px, dst_px) in resized.as_chunks::<4>().0.iter().zip(dst.as_chunks_mut::<4>().0.iter_mut()) {
            dst_px[0] = src_px[2];
            dst_px[1] = src_px[1];
            dst_px[2] = src_px[0];
            dst_px[3] = src_px[3];
        }
        let mask_bmp = CreateBitmap(sz as i32, sz as i32, 1, 1, None);
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color_bmp.into());
        if !mask_bmp.is_invalid() {
            let _ = DeleteObject(mask_bmp.into());
        }
        icon
    }
}

/// Loads custom plugin `HICON` handles if provided, otherwise falls back to embedded Tomoyo icon.
/// Returns `(big, small, owns_handles)`.
pub fn load_window_icons(hinst: HMODULE, icon_path: Option<&str>) -> (Option<HICON>, Option<HICON>, bool) {
    if let Some(p) = icon_path.map(str::trim).filter(|s| !s.is_empty()) {
        let (big_sz, sm_sz) = unsafe { (GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CXSMICON)) };
        if let Some(big) = create_hicon_from_image(p, big_sz) {
            let small = create_hicon_from_image(p, sm_sz);
            return (Some(big), small.or(Some(big)), true);
        }
    }
    let (big, small) = load_embedded_window_icons(hinst);
    (big, small, false)
}

pub unsafe fn apply_window_icons(hwnd: HWND, icons: (Option<HICON>, Option<HICON>)) {
    unsafe {
        if let Some(ic) = icons.1 {
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM_SETICON_SMALL), Some(windows::Win32::Foundation::LPARAM(ic.0 as isize)));
        }
        if let Some(ic) = icons.0 {
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM_SETICON_BIG), Some(windows::Win32::Foundation::LPARAM(ic.0 as isize)));
        }
    }
}

pub unsafe fn destroy_owned_icons(big: Option<HICON>, small: Option<HICON>, owns: bool) {
    if !owns {
        return;
    }
    unsafe {
        if let Some(b) = big && !b.is_invalid() {
            let _ = DestroyIcon(b);
        }
        if let Some(s) = small && !s.is_invalid() && Some(s) != big {
            let _ = DestroyIcon(s);
        }
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

use windows::Win32::UI::WindowsAndMessaging::KillTimer;

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

#[allow(clippy::too_many_arguments)]
pub unsafe fn create_themed_host_window(
    cls: PCWSTR,
    wndproc: WNDPROC,
    title: &str,
    theme: &str,
    width: i32,
    height: i32,
    visible_initially: bool,
    frameless: bool,
    icons: (Option<HICON>, Option<HICON>),
) -> HWND {
    unsafe {
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: wndproc,
            hInstance: hinst.into(),
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
        apply_window_icons(hwnd, icons);
        if frameless {
            apply_frameless_dwm_style(hwnd);
        }
        apply_host_window_theme(hwnd, theme);
        hwnd
    }
}

pub unsafe fn apply_frameless_dwm_style(hwnd: HWND) {
    unsafe {
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const c_void,
            std::mem::size_of_val(&corner) as u32,
        );
        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 1,
            cyBottomHeight: 0,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
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

pub fn set_webview_bounds_for_window(
    hwnd: HWND,
    ctrl: &ICoreWebView2Controller,
    frameless: bool,
) {
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        if frameless && !IsZoomed(hwnd).as_bool() {
            rc.left += 4;
            rc.top += 3;
            rc.right = (rc.right - 4).max(rc.left + 100);
            rc.bottom = (rc.bottom - 4).max(rc.top + 100);
        }
        let _ = ctrl.SetBounds(rc);
    }
}

pub unsafe fn handle_frameless_nccalcsize(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if wparam.0 != 0 && IsZoomed(hwnd).as_bool() {
            let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
            let pad_x = GetSystemMetrics(SM_CXSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER);
            let pad_y = GetSystemMetrics(SM_CYSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER);
            params.rgrc[0].left += pad_x;
            params.rgrc[0].top += pad_y;
            params.rgrc[0].right -= pad_x;
            params.rgrc[0].bottom -= pad_y;
        }
        LRESULT(0)
    }
}

pub unsafe fn handle_frameless_nchittest(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    unsafe {
        if !IsZoomed(hwnd).as_bool() {
            let sx = (lparam.0 & 0xFFFF) as i16 as i32;
            let sy = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut rc = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rc);
            let hit = match (sx < rc.left + 6, sx >= rc.right - 6, sy < rc.top + 6, sy >= rc.bottom - 6) {
                (true, _, true, _) => HTTOPLEFT,
                (_, true, true, _) => HTTOPRIGHT,
                (true, _, _, true) => HTBOTTOMLEFT,
                (_, true, _, true) => HTBOTTOMRIGHT,
                (true, _, _, _) => HTLEFT,
                (_, true, _, _) => HTRIGHT,
                (_, _, true, _) => HTTOP,
                (_, _, _, true) => HTBOTTOM,
                _ => HTCLIENT,
            };
            return LRESULT(hit as isize);
        }
        LRESULT(HTCLIENT as isize)
    }
}

pub unsafe fn handle_frameless_erasebkgnd(hwnd: HWND, wparam: WPARAM, theme: &str) -> LRESULT {
    unsafe {
        let dc = HDC(wparam.0 as *mut c_void);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let brush = create_theme_bg_brush(theme);
        let _ = FillRect(dc, &rc, brush);
        let _ = DeleteObject(brush.into());
        LRESULT(1)
    }
}

pub unsafe fn handle_frameless_getminmaxinfo(lparam: LPARAM) -> LRESULT {
    unsafe {
        let mmi = &mut *(lparam.0 as *mut MINMAXINFO);
        mmi.ptMinTrackSize.x = 760;
        mmi.ptMinTrackSize.y = 520;
        LRESULT(0)
    }
}

