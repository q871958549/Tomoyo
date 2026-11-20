//! Pure Win32 `main_wndproc` message dispatcher delegating into cohesive `AppShell` methods.

use super::commands::{dispatch_menu_id, dispatch_wm_command};
use super::{
    AppShell, HOTKEY_ID, TIMER_CLEAR_TOAST, TIMER_HIBERNATE, WM_APP_CONFIG_SAVED,
    WM_APP_EDIT_KEY, WM_APP_PASTE_INTERCEPT, WM_APP_RESULTS_READY, WM_APP_UPDATE_READY,
    WM_APP_WAKE_EXISTING,
};
use crate::actions::{read_clipboard_image_png, read_clipboard_text};
use crate::drag::start_file_drag;
use crate::icons::WM_APP_ICON_READY;
use crate::preview_loader::WM_APP_PREVIEW_READY;
use crate::renderer::{
    FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP, MAX_WINDOW_WIDTH_DIP, MIN_WINDOW_WIDTH_DIP,
};
use crate::tray::WM_TRAY_CALLBACK;
use std::ffi::c_void;
use tomoyo_protocol::IconSpec;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, ScreenToClient, SetBkColor, SetTextColor, HDC, PAINTSTRUCT,
};
use windows::Win32::System::DataExchange::RemoveClipboardFormatListener;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, UnregisterHotKey, TME_LEAVE,
    TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetClientRect, GetCursorPos, GetWindowLongPtrW, IsWindowVisible, KillTimer,
    LoadCursorW, PostQuitMessage, SetCursor, GWLP_USERDATA, HTBOTTOM, HTBOTTOMLEFT,
    HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_SIZEWE,
    MINMAXINFO, WM_ACTIVATE, WM_CLIPBOARDUPDATE, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY,
    WM_ENDSESSION, WM_ENTERSIZEMOVE, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_HOTKEY,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST,
    WM_NCRBUTTONUP, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_SIZE, WM_TIMER,
};

pub(super) unsafe extern "system" fn main_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut AppShell;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let app = unsafe { &mut *ptr };

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_GETMINMAXINFO => {
            let mmi = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            let scale = app.dpi_scale.max(1.0);
            mmi.ptMinTrackSize.x = (MIN_WINDOW_WIDTH_DIP as f32 * scale) as i32;
            mmi.ptMinTrackSize.y = ((HEADER_HEIGHT_DIP + FOOTER_HEIGHT_DIP) as f32 * scale) as i32;
            mmi.ptMaxTrackSize.x = (MAX_WINDOW_WIDTH_DIP as f32 * scale) as i32;
            mmi.ptMaxTrackSize.y = (1080.0 * scale) as i32;
            LRESULT(0)
        }
        WM_NCHITTEST => hit_test_window_chrome(hwnd, app.dpi_scale, lparam),
        WM_SETCURSOR => {
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(hwnd, &mut pt);
                }
                if app.inter.dragging_divider || app.is_over_preview_divider(pt.x, pt.y) {
                    if let Ok(cur) = unsafe { LoadCursorW(None, IDC_SIZEWE) } {
                        unsafe { SetCursor(Some(cur)); }
                        return LRESULT(1);
                    }
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_ENTERSIZEMOVE => {
            app.inter.in_size_move = true;
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            app.inter.in_size_move = false;
            app.persist_window_geometry();
            LRESULT(0)
        }
        WM_SIZE => {
            let new_w = (lparam.0 & 0xFFFF) as i32;
            let new_h = ((lparam.0 >> 16) & 0xFFFF) as i32;
            if new_w > 0 && new_h > 0 {
                app.handle_window_resize(new_w, new_h);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut rc = RECT::default();
            let _ = unsafe { GetClientRect(hwnd, &mut rc) };
            app.render_into_hdc(hdc, rc.right - rc.left, rc.bottom - rc.top);
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut c_void);
            unsafe {
                SetTextColor(hdc, app.theme.text_primary.to_colorref());
                SetBkColor(hdc, app.theme.bg.to_colorref());
            }
            LRESULT(app.edit_bg_brush.0 as isize)
        }
        WM_COMMAND => dispatch_wm_command(hwnd, app, wparam),
        WM_APP_CONFIG_SAVED => {
            app.reload_config_and_all();
            app.show_toast(app.lang.pick("设置已实时同步生效", "Settings synced live"));
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wparam.0 as i32 == HOTKEY_ID {
                app.toggle_launcher();
            }
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            if let Some(txt) = read_clipboard_text() {
                app.inter.suppress_next_clip_img = false;
                app.clipboard.push_text(txt);
            } else if app.inter.suppress_next_clip_img {
                app.inter.suppress_next_clip_img = false;
            } else if let Some((path, w, h, digest)) = read_clipboard_image_png() {
                app.clipboard.push_image(path, w, h, digest);
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) == 0 {
                if app.inter.suppress_next_deactivate {
                    app.inter.suppress_next_deactivate = false;
                } else if unsafe { IsWindowVisible(hwnd).as_bool() } {
                    app.hide_launcher();
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                TIMER_HIBERNATE => {
                    if !unsafe { IsWindowVisible(hwnd).as_bool() } {
                        let cycles = app.run_hibernate();
                        tracing::info!("Entered hibernation cycle #{cycles}");
                    } else {
                        unsafe { let _ = KillTimer(Some(hwnd), TIMER_HIBERNATE); }
                    }
                }
                TIMER_CLEAR_TOAST => {
                    unsafe { let _ = KillTimer(Some(hwnd), TIMER_CLEAR_TOAST); }
                    app.toast = None;
                    app.invalidate();
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_APP_RESULTS_READY => {
            app.on_results_ready();
            LRESULT(0)
        }
        WM_APP_ICON_READY => {
            app.on_icons_ready();
            LRESULT(0)
        }
        WM_APP_PREVIEW_READY => {
            app.on_preview_ready();
            LRESULT(0)
        }
        WM_APP_EDIT_KEY => {
            let vk = (wparam.0 & 0xFFFF) as u16;
            app.handle_nav_key(
                vk,
                (lparam.0 & 1) != 0,
                (lparam.0 & 2) != 0,
                (lparam.0 & 4) != 0,
            );
            LRESULT(0)
        }
        WM_APP_PASTE_INTERCEPT => {
            app.on_paste_intercept();
            LRESULT(0)
        }
        WM_APP_UPDATE_READY => {
            app.on_update_ready();
            LRESULT(0)
        }
        WM_APP_WAKE_EXISTING => {
            app.show_launcher();
            LRESULT(0)
        }
        WM_NCRBUTTONUP => {
            if wparam.0 as u32 == HTCAPTION {
                app.show_logo_context_menu();
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_RBUTTONUP => {
            let (x, y) = unpack_xy(lparam);
            let y_dip = y as f32 / app.dpi_scale.max(1.0);
            if y_dip < HEADER_HEIGHT_DIP as f32 {
                app.show_logo_context_menu();
            } else if let Some(idx) = app.hit_test_row(x, y) {
                app.selected = idx;
                app.refresh_selected_preview();
                app.invalidate();
                app.show_result_context_menu(idx);
            } else {
                app.show_logo_context_menu();
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => handle_mouse_down(hwnd, app, lparam),
        WM_MOUSEMOVE => handle_mouse_move(hwnd, app, lparam),
        WM_MOUSELEAVE => {
            app.inter.mouse_tracked = false;
            let changed = app.inter.hovered.take().is_some()
                | app.inter.hovered_filter.take().is_some();
            if changed {
                app.invalidate();
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            app.inter.drag_origin = None;
            if app.inter.dragging_divider {
                app.inter.dragging_divider = false;
                unsafe { let _ = ReleaseCapture(); }
                app.persist_window_geometry();
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            app.inter.drag_origin = None;
            let (x, y) = unpack_xy(lparam);
            if let Some(idx) = app.hit_test_row(x, y) {
                app.selected = idx;
                app.execute_selected(0);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
            if !app.results.is_empty() {
                if delta < 0 && app.selected + 1 < app.results.len() {
                    app.selected += 1;
                } else if delta > 0 && app.selected > 0 {
                    app.selected -= 1;
                }
                app.ensure_selected_visible();
                app.refresh_selected_preview();
                app.resize_to_fit_results();
                app.invalidate();
            }
            LRESULT(0)
        }
        WM_TRAY_CALLBACK => {
            let ev = (lparam.0 & 0xFFFF) as u32;
            if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                app.toggle_launcher();
            } else if ev == WM_RBUTTONUP {
                let chosen = app.tray.as_ref().and_then(|tray| {
                    tray.show_context_menu(
                        app.lang,
                        app.theme,
                        app.dpi_scale,
                        app.font_regular,
                        app.font_medium,
                    )
                });
                if let Some(id) = chosen {
                    dispatch_menu_id(hwnd, app, id);
                }
            }
            LRESULT(0)
        }
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                tracing::info!("Windows session ending; performing clean shutdown checkpoint");
                app.shutdown_clean();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            app.shutdown_clean();
            unsafe {
                let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
                let _ = RemoveClipboardFormatListener(hwnd);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        m if m != 0 && m == app.taskbar_created_msg => {
            if let Some(tray) = &mut app.tray {
                tray.reinstall();
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn unpack_xy(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32)
}

fn hit_test_window_chrome(hwnd: HWND, dpi_scale: f32, lparam: LPARAM) -> LRESULT {
    let (sx, sy) = unpack_xy(lparam);
    let mut pt = POINT { x: sx, y: sy };
    let mut rc = RECT::default();
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
    let grip = (6.0 * dpi_scale.max(1.0)).round() as i32;
    let hit = match (pt.x < grip, pt.x >= w - grip, pt.y < grip, pt.y >= h - grip) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => {
            let header_px = (HEADER_HEIGHT_DIP as f32 * dpi_scale.max(1.0)) as i32;
            let footer_px = (FOOTER_HEIGHT_DIP as f32 * dpi_scale.max(1.0)) as i32;
            if pt.y < header_px || pt.y >= h - footer_px { HTCAPTION } else { HTCLIENT }
        }
    };
    LRESULT(hit as isize)
}

fn handle_mouse_down(hwnd: HWND, app: &mut AppShell, lparam: LPARAM) -> LRESULT {
    let (x, y) = unpack_xy(lparam);
    if app.is_over_preview_divider(x, y) {
        app.inter.dragging_divider = true;
        unsafe { SetCapture(hwnd); }
        return LRESULT(0);
    }
    if let Some(f) = app.hit_test_filter(x, y) {
        app.set_active_filter(f);
        unsafe { let _ = SetFocus(Some(app.edit_hwnd)); }
        return LRESULT(0);
    }
    if let Some(idx) = app.hit_test_row(x, y) {
        app.action_menu_open = false;
        app.selected = idx;
        app.refresh_selected_preview();
        app.resize_to_fit_results();
        app.invalidate();
        app.inter.drag_origin = match app.results.get(idx).map(|r| r.icon.clone()) {
            Some(IconSpec::ShellPath(path)) => Some((x, y, path)),
            _ => None,
        };
    }
    LRESULT(0)
}

fn handle_mouse_move(hwnd: HWND, app: &mut AppShell, lparam: LPARAM) -> LRESULT {
    let (x, y) = unpack_xy(lparam);
    if !app.inter.mouse_tracked {
        app.inter.mouse_tracked = true;
        let mut tme = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        unsafe { let _ = TrackMouseEvent(&mut tme); }
    }
    if app.inter.dragging_divider {
        let mut rc = RECT::default();
        unsafe { let _ = GetClientRect(hwnd, &mut rc); }
        let scale = app.dpi_scale.max(1.0);
        let w_dip = (rc.right - rc.left) as f32 / scale;
        let clamped = (w_dip - x as f32 / scale)
            .round()
            .clamp(220.0, ((w_dip - 380.0).max(220.0)).min(620.0)) as i32;
        if clamped != app.geom.preview_width_dip {
            app.geom.preview_width_dip = clamped;
            app.geom.list_width_dip = (w_dip as i32 - clamped).clamp(MIN_WINDOW_WIDTH_DIP, 1400);
            app.invalidate();
        }
        return LRESULT(0);
    }
    if let Some((ox, oy, path)) = app.inter.drag_origin.clone() {
        if (x - ox).abs() > 6 || (y - oy).abs() > 6 {
            app.inter.drag_origin = None;
            app.inter.suppress_next_deactivate = true;
            let _ = start_file_drag(&path);
            return LRESULT(0);
        }
    }
    let new_filter_hover = app.hit_test_filter(x, y);
    let new_hover = if new_filter_hover.is_some() { None } else { app.hit_test_row(x, y) };
    if new_hover != app.inter.hovered || new_filter_hover != app.inter.hovered_filter {
        app.inter.hovered = new_hover;
        app.inter.hovered_filter = new_filter_hover;
        app.invalidate();
    }
    LRESULT(0)
}
