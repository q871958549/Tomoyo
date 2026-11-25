//! Pointer, cursor, drag-resize, filter-tab, and wheel handlers delegated from `wndproc.rs`.

use super::AppShell;
use crate::drag::start_file_drag;
use crate::renderer::{FilterHit, FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP, MIN_WINDOW_WIDTH_DIP};
use std::time::Instant;
use tomoyo_protocol::IconSpec;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetDoubleClickTime, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE,
    TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetClientRect, GetCursorPos, LoadCursorW, SetCursor, HTBOTTOM, HTBOTTOMLEFT,
    HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_HAND,
    IDC_SIZEWE,
};

pub(super) fn unpack_xy(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32)
}

pub(super) fn hit_test_window_chrome(hwnd: HWND, app: &AppShell, lparam: LPARAM) -> LRESULT {
    let (sx, sy) = unpack_xy(lparam);
    let mut pt = POINT { x: sx, y: sy };
    let mut rc = RECT::default();
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
    let grip = (6.0 * app.dpi_scale.max(1.0)).round() as i32;
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
            let header_px = (HEADER_HEIGHT_DIP as f32 * app.dpi_scale.max(1.0)) as i32;
            let footer_px = (FOOTER_HEIGHT_DIP as f32 * app.dpi_scale.max(1.0)) as i32;
            if app.hit_test_update_badge(pt.x, pt.y) || app.hit_test_logo_icon(pt.x, pt.y) {
                HTCLIENT
            } else if pt.y < header_px || pt.y >= h - footer_px {
                HTCAPTION
            } else {
                HTCLIENT
            }
        }
    };
    LRESULT(hit as isize)
}

pub(super) fn handle_set_cursor(
    hwnd: HWND,
    app: &AppShell,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(hwnd, &mut pt);
        }
        if (app.inter.dragging_divider || app.is_over_preview_divider(pt.x, pt.y))
            && let Ok(cur) = unsafe { LoadCursorW(None, IDC_SIZEWE) }
        {
            unsafe {
                SetCursor(Some(cur));
            }
            return LRESULT(1);
        }
        if (app.hit_test_update_badge(pt.x, pt.y) || app.hit_test_logo_icon(pt.x, pt.y))
            && let Ok(cur) = unsafe { LoadCursorW(None, IDC_HAND) }
        {
            unsafe {
                SetCursor(Some(cur));
            }
            return LRESULT(1);
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub(super) fn handle_rbutton_up(app: &mut AppShell, lparam: LPARAM) -> LRESULT {
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

pub(super) fn handle_mouse_down(hwnd: HWND, app: &mut AppShell, lparam: LPARAM) -> LRESULT {
    let (x, y) = unpack_xy(lparam);
    if app.hit_test_logo_icon(x, y) {
        app.inter.pressed_logo = true;
        return LRESULT(0);
    }
    if app.hit_test_update_badge(x, y) {
        app.set_query_text(":update");
        return LRESULT(0);
    }
    if app.is_over_preview_divider(x, y) {
        app.inter.dragging_divider = true;
        unsafe {
            SetCapture(hwnd);
        }
        return LRESULT(0);
    }
    if let Some(hit) = app.hit_test_filter_target(x, y) {
        match hit {
            FilterHit::Tab(f) => {
                app.set_active_filter(f);
                unsafe {
                    let _ = SetFocus(Some(app.edit_hwnd));
                }
            }
            FilterHit::Overflow => app.show_filter_overflow_menu(),
        }
        return LRESULT(0);
    }
    if let Some(idx) = app.hit_test_row(x, y) {
        let dbl_ms = unsafe { GetDoubleClickTime() }.clamp(300, 550) as u128;
        if let Some((prev_idx, prev_at)) = app.inter.last_click
            && prev_idx == idx
            && prev_at.elapsed().as_millis() <= dbl_ms
        {
            // Row-aware double-click tolerance: even if hand jitter exceeded Windows' 4px SM_CXDOUBLECLK box
            // inside the same result row, treat it identically to WM_LBUTTONDBLCLK and launch immediately.
            app.inter.last_click = None;
            app.inter.drag_origin = None;
            app.selected = idx;
            app.execute_selected(0);
            return LRESULT(0);
        }
        app.inter.last_click = Some((idx, Instant::now()));
        app.action_menu_open = false;
        let row_changed = app.selected != idx;
        app.selected = idx;
        if row_changed {
            app.cancel_pending_launch_hide();
            app.refresh_selected_preview();
            app.resize_to_fit_results();
        }
        app.invalidate();
        app.inter.drag_origin = match app.results.get(idx).map(|r| r.icon.clone()) {
            Some(IconSpec::ShellPath(path)) => Some((x, y, path)),
            _ => None,
        };
    } else {
        app.inter.last_click = None;
    }
    LRESULT(0)
}

pub(super) fn handle_mouse_move(
    hwnd: HWND,
    app: &mut AppShell,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let (x, y) = unpack_xy(lparam);
    if !app.inter.mouse_tracked {
        app.inter.mouse_tracked = true;
        let mut tme = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        unsafe {
            let _ = TrackMouseEvent(&mut tme);
        }
    }
    if app.inter.dragging_divider {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rc);
        }
        let scale = app.dpi_scale.max(1.0);
        let w_dip = (rc.right - rc.left) as f32 / scale;
        let clamped = (w_dip - x as f32 / scale)
            .round()
            .clamp(220.0, (w_dip - 380.0).clamp(220.0, 620.0)) as i32;
        if clamped != app.geom.preview_width_dip {
            app.geom.preview_width_dip = clamped;
            app.geom.list_width_dip = (w_dip as i32 - clamped).clamp(MIN_WINDOW_WIDTH_DIP, 1400);
            app.invalidate();
        }
        return LRESULT(0);
    }
    let lbtn_down = (wparam.0 & 0x0001) != 0;
    let drag_thresh = (12.0 * app.dpi_scale.max(1.0)).round() as i32;
    if lbtn_down
        && !app.has_in_flight_launch()
        && let Some((ox, oy, path)) = app.inter.drag_origin.clone()
        && ((x - ox).abs() > drag_thresh || (y - oy).abs() > drag_thresh)
    {
        app.inter.drag_origin = None;
        app.inter.last_click = None;
        {
            let _modal = super::enter_modal_overlay();
            let _ = start_file_drag(&path);
        }
        return LRESULT(0);
    }
    let new_logo_hover = app.hit_test_logo_icon(x, y);
    let filter_hit = app.hit_test_filter_target(x, y);
    let new_filter_hover = match filter_hit {
        Some(FilterHit::Tab(f)) => Some(f),
        _ => None,
    };
    let new_overflow_hover = matches!(filter_hit, Some(FilterHit::Overflow));
    let new_hover = if filter_hit.is_some() {
        None
    } else {
        app.hit_test_row(x, y)
    };
    if new_hover != app.inter.hovered
        || new_filter_hover != app.inter.hovered_filter
        || new_overflow_hover != app.inter.hovered_overflow
        || new_logo_hover != app.inter.hovered_logo
    {
        app.inter.hovered = new_hover;
        app.inter.hovered_filter = new_filter_hover;
        app.inter.hovered_overflow = new_overflow_hover;
        app.inter.hovered_logo = new_logo_hover;
        app.invalidate();
    }
    LRESULT(0)
}

pub(super) fn handle_mouse_leave(app: &mut AppShell) -> LRESULT {
    app.inter.mouse_tracked = false;
    app.inter.pressed_logo = false;
    let changed = app.inter.hovered.take().is_some()
        | app.inter.hovered_filter.take().is_some()
        | std::mem::replace(&mut app.inter.hovered_overflow, false)
        | std::mem::replace(&mut app.inter.hovered_logo, false);
    if changed {
        app.invalidate();
    }
    LRESULT(0)
}

pub(super) fn handle_lbutton_up(app: &mut AppShell) -> LRESULT {
    app.inter.drag_origin = None;
    if app.inter.pressed_logo {
        app.inter.pressed_logo = false;
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(app.hwnd, &mut pt);
        }
        if app.hit_test_logo_icon(pt.x, pt.y) {
            app.open_settings_center();
            return LRESULT(0);
        }
    }
    if app.inter.dragging_divider {
        app.inter.dragging_divider = false;
        unsafe {
            let _ = ReleaseCapture();
        }
        app.persist_window_geometry();
    }
    LRESULT(0)
}

pub(super) fn handle_lbutton_dblclk(app: &mut AppShell, lparam: LPARAM) -> LRESULT {
    app.inter.drag_origin = None;
    app.inter.last_click = None;
    let (x, y) = unpack_xy(lparam);
    if let Some(idx) = app.hit_test_row(x, y) {
        app.selected = idx;
        app.execute_selected(0);
    }
    LRESULT(0)
}

pub(super) fn handle_mouse_wheel(app: &mut AppShell, wparam: WPARAM) -> LRESULT {
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
