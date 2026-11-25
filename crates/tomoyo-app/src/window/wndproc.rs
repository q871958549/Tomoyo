//! Pure Win32 `main_wndproc` message dispatcher delegating into cohesive `AppShell` methods.

use super::commands::{dispatch_menu_id, dispatch_wm_command};
use super::input::{register_all_hotkeys, unregister_all_hotkeys};
use super::super_panel_ctrl::{
    TIMER_SP_HOLD, WM_APP_SP_ARM, WM_APP_SP_EXEC, WM_APP_SP_FIRE, WM_APP_SP_PROBED,
};
use super::updater_ctrl::WM_APP_UPDATE_PROGRESS;
use super::wndproc_mouse::{
    handle_capture_changed, handle_lbutton_dblclk, handle_lbutton_up, handle_mouse_down,
    handle_mouse_leave, handle_mouse_move, handle_mouse_wheel, handle_rbutton_up,
    handle_set_cursor, hit_test_window_chrome,
};
use super::{
    AppShell, HOTKEY_ID, TIMER_CLEAR_TOAST, TIMER_FADE_HIDE, TIMER_FOCUS_GUARD, TIMER_HIBERNATE,
    TIMER_LAUNCH_HIDE, TIMER_LAUNCH_TICK, TIMER_SETTINGS_HIDE, WM_APP_CONFIG_SAVED,
    WM_APP_EDIT_KEY, WM_APP_FLOW_DONE, WM_APP_LAUNCH_DONE, WM_APP_PASTE_INTERCEPT,
    WM_APP_PLUGIN_EVENT_DONE, WM_APP_RESULTS_READY, WM_APP_SHOW_CONTEXT_MENU,
    WM_APP_UPDATE_INSTALLED, WM_APP_UPDATE_READY, WM_APP_WAKE_EXISTING,
};
use crate::icons::WM_APP_ICON_READY;
use crate::preview_loader::WM_APP_PREVIEW_READY;
use crate::renderer::{
    FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP, MAX_WINDOW_WIDTH_DIP, MIN_WINDOW_WIDTH_DIP,
};
use crate::tray::WM_TRAY_CALLBACK;
use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, SetBkColor, SetTextColor, HDC, PAINTSTRUCT,
};
use windows::Win32::System::DataExchange::RemoveClipboardFormatListener;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetClientRect, GetForegroundWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsWindowVisible, KillTimer, PostQuitMessage, GWLP_USERDATA,
    HTCAPTION, MINMAXINFO, WM_ACTIVATE, WM_ACTIVATEAPP, WM_CAPTURECHANGED, WM_CLIPBOARDUPDATE,
    WM_COMMAND,
    WM_CTLCOLOREDIT, WM_DESTROY, WM_DPICHANGED, WM_ENDSESSION, WM_ENTERSIZEMOVE, WM_ERASEBKGND,
    WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST, WM_NCRBUTTONUP, WM_PAINT, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SIZE, WM_TIMER,
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
        WM_NCHITTEST => hit_test_window_chrome(hwnd, app, lparam),
        WM_SETCURSOR => handle_set_cursor(hwnd, app, msg, wparam, lparam),
        WM_ENTERSIZEMOVE => {
            app.inter.in_size_move = true;
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            app.inter.in_size_move = false;
            app.sync_inline_dock_bounds();
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
            match wparam.0 {
                7 => app.on_inline_host_detached(),
                6 => app.on_settings_window_ready(),
                5 => {
                    tracing::info!("Updater requested graceful shutdown; exiting Tomoyo cleanly");
                    app.shutdown_clean();
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                    }
                }
                4 => {
                    let cfg = app.config.read().unwrap().clone();
                    unregister_all_hotkeys(hwnd);
                    let (active, _, plug_hks, flow_hks) =
                        register_all_hotkeys(hwnd, &cfg, &app.plugin_host);
                    app.plugin_hotkeys = plug_hks;
                    app.flow_hotkeys = flow_hks;
                    if let Some(t) = app.tray.as_mut() {
                        t.update_tooltip(&active);
                    }
                }
                3 => unregister_all_hotkeys(hwnd),
                2 => app.on_clipboard_db_external_change(),
                1 => {
                    app.reload_config_and_all();
                    app.show_toast(app.lang.pick("设置与插件已同步生效", "Settings & plugins synced"));
                }
                _ => {
                    app.reload_config_from_settings();
                    app.show_toast(app.lang.pick("设置已实时同步生效", "Settings synced live"));
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            if super::hotkey_avoid::should_avoid_hotkey(hwnd) {
                return LRESULT(0);
            }
            match wparam.0 as i32 {
                HOTKEY_ID => app.toggle_launcher(),
                id if id >= super::plugin_hotkeys::FLOW_HOTKEY_BASE => {
                    super::plugin_hotkeys::handle_flow_hotkey(app, id);
                }
                id if id >= super::plugin_hotkeys::PLUGIN_HOTKEY_BASE => {
                    super::plugin_hotkeys::handle_plugin_hotkey(app, id);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            app.on_clipboard_update();
            LRESULT(0)
        }
        WM_ACTIVATE => {
            let in_grace = app.inter.last_shown_at.is_some_and(|t| t.elapsed().as_millis() < 140);
            let to_inline = lparam.0 != 0 && app.is_inline_child_hwnd(HWND(lparam.0 as *mut c_void));
            if (wparam.0 & 0xFFFF) == 0
                && !in_grace
                && !to_inline
                && !super::is_modal_overlay_active()
                && unsafe { IsWindowVisible(hwnd).as_bool() }
            {
                app.hide_launcher();
            }
            LRESULT(0)
        }
        WM_ACTIVATEAPP => {
            let in_grace = app.inter.last_shown_at.is_some_and(|t| t.elapsed().as_millis() < 140);
            let to_inline = app.is_inline_child_hwnd(unsafe { GetForegroundWindow() });
            if wparam.0 == 0
                && !in_grace
                && !to_inline
                && !super::is_modal_overlay_active()
                && unsafe { IsWindowVisible(hwnd).as_bool() }
            {
                app.hide_launcher();
            }
            LRESULT(0)
        }
        WM_TIMER => handle_timer(hwnd, app, wparam.0),
        WM_APP_RESULTS_READY => { app.on_results_ready(); LRESULT(0) }
        WM_APP_ICON_READY => { app.on_icons_ready(); LRESULT(0) }
        WM_APP_PREVIEW_READY => { app.on_preview_ready(); LRESULT(0) }
        WM_APP_EDIT_KEY => {
            let vk = (wparam.0 & 0xFFFF) as u16;
            app.handle_nav_key(vk, (lparam.0 & 1) != 0, (lparam.0 & 2) != 0, (lparam.0 & 4) != 0);
            LRESULT(0)
        }
        WM_APP_PASTE_INTERCEPT => { app.on_paste_intercept(); LRESULT(0) }
        WM_APP_UPDATE_READY => { app.on_update_ready(); LRESULT(0) }
        WM_APP_UPDATE_PROGRESS => { app.on_update_progress(); LRESULT(0) }
        WM_APP_UPDATE_INSTALLED => { app.on_update_installed(); LRESULT(0) }
        WM_APP_FLOW_DONE => { app.on_async_flow_done(); LRESULT(0) }
        WM_APP_LAUNCH_DONE => { app.on_async_launch_done(); LRESULT(0) }
        WM_APP_SHOW_CONTEXT_MENU => { app.show_launcher_context_menu(wparam.0 != 0); LRESULT(0) }
        WM_APP_PLUGIN_EVENT_DONE => { super::plugin_hotkeys::on_async_plugin_event_done(app); LRESULT(0) }
        WM_APP_SP_ARM => { app.on_super_panel_arm(wparam.0 as u32, lparam.0 as u8); LRESULT(0) }
        WM_APP_SP_FIRE => { app.on_super_panel_fire_click(); LRESULT(0) }
        WM_APP_SP_PROBED => { app.on_super_panel_probed(); LRESULT(0) }
        WM_APP_SP_EXEC => { app.on_super_panel_exec(); LRESULT(0) }
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xFFFF) as u32;
            let suggested = (lparam.0 != 0).then(|| unsafe { &*(lparam.0 as *const RECT) });
            app.handle_dpi_changed(new_dpi, suggested);
            LRESULT(0)
        }
        WM_APP_WAKE_EXISTING => { app.show_launcher(); LRESULT(0) }
        WM_NCRBUTTONUP => {
            if wparam.0 as u32 == HTCAPTION {
                app.show_logo_context_menu();
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_RBUTTONUP => handle_rbutton_up(app, lparam),
        WM_LBUTTONDOWN => handle_mouse_down(hwnd, app, lparam),
        WM_MOUSEMOVE => handle_mouse_move(hwnd, app, wparam, lparam),
        WM_MOUSELEAVE => handle_mouse_leave(app),
        WM_CAPTURECHANGED => handle_capture_changed(app),
        WM_LBUTTONUP => handle_lbutton_up(app),
        WM_LBUTTONDBLCLK => handle_lbutton_dblclk(app, lparam),
        WM_MOUSEWHEEL => handle_mouse_wheel(app, wparam),
        WM_TRAY_CALLBACK => handle_tray_callback(hwnd, app, lparam),
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                tracing::info!("Windows session ending; performing clean shutdown checkpoint");
                app.shutdown_clean();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            app.shutdown_clean();
            unregister_all_hotkeys(hwnd);
            unsafe {
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

fn handle_timer(hwnd: HWND, app: &mut AppShell, timer_id: usize) -> LRESULT {
    match timer_id {
        TIMER_FOCUS_GUARD => {
            if !unsafe { IsWindowVisible(hwnd).as_bool() } {
                unsafe { let _ = KillTimer(Some(hwnd), TIMER_FOCUS_GUARD); }
            } else if !super::is_modal_overlay_active() && !app.inter.in_size_move {
                let fg = unsafe { GetForegroundWindow() };
                let mut fg_pid = 0u32;
                if !fg.is_invalid() {
                    unsafe { GetWindowThreadProcessId(fg, Some(&mut fg_pid)); }
                }
                if fg != hwnd && fg_pid != std::process::id() && !app.is_inline_child_hwnd(fg) {
                    if app.inter.last_shown_at.is_some_and(|t| t.elapsed().as_millis() < 420) {
                        super::hotkey_override::force_foreground_window(hwnd);
                        unsafe { let _ = SetFocus(Some(app.edit_hwnd)); }
                    } else {
                        app.hide_launcher();
                    }
                }
            }
        }
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
        TIMER_LAUNCH_HIDE => {
            unsafe { let _ = KillTimer(Some(hwnd), TIMER_LAUNCH_HIDE); }
            if unsafe { IsWindowVisible(hwnd).as_bool() } {
                app.hide_launcher();
            }
        }
        TIMER_LAUNCH_TICK => app.on_launch_tick(),
        TIMER_SP_HOLD => app.on_super_panel_hold_timer(),
        TIMER_SETTINGS_HIDE => app.on_settings_window_ready(),
        TIMER_FADE_HIDE => app.step_fade_hide(),
        _ => {}
    }
    LRESULT(0)
}

fn handle_tray_callback(hwnd: HWND, app: &mut AppShell, lparam: LPARAM) -> LRESULT {
    let ev = (lparam.0 & 0xFFFF) as u32;
    if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
        app.toggle_launcher();
    } else if ev == WM_RBUTTONUP {
        app.tray_plugin_actions = app.plugin_host.active_tray_actions();
        let chosen = app.tray.as_ref().and_then(|tray| {
            tray.show_context_menu(
                app.lang, app.theme, app.dpi_scale, app.font_regular, app.font_medium,
                &app.tray_plugin_actions,
            )
        });
        if let Some(id) = chosen {
            dispatch_menu_id(hwnd, app, id);
        }
    }
    LRESULT(0)
}
