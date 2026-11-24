//! WebView2 incoming JSON message dispatcher and built-in HTML bundle router.

use crate::bridge::handle_host_message;
use crate::clip_view::assemble_clip_manager_html;
use crate::db::PluginDb;
use crate::inline_dock::promote_inline_to_window;
use crate::ocr_view::assemble_ocr_viewer_html;
use crate::tr_view::assemble_translator_html;
use crate::win_host::{HostWindowState, TIMER_REVEAL_FALLBACK, WM_APP_PICK_FOLDER, WM_APP_WEBVIEW_EVENT};
use crate::win_theme::{
    apply_host_window_theme, reveal_host_window_and_notify, toggle_pin_topmost_window,
};
use crate::win_workers::dispatch_async_workers;
use crate::assemble_settings_html;
use std::sync::mpsc;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

pub(crate) fn pick_builtin_html(
    settings_mode: bool,
    ocr_mode: bool,
    clip_mode: bool,
    tr_mode: bool,
) -> Option<String> {
    if settings_mode {
        Some(assemble_settings_html())
    } else if ocr_mode {
        Some(assemble_ocr_viewer_html())
    } else if clip_mode {
        Some(assemble_clip_manager_html())
    } else if tr_mode {
        Some(assemble_translator_html())
    } else {
        None
    }
}

pub(crate) unsafe fn dispatch_webview_message(
    msg_str: &str,
    db: &mut PluginDb,
    state_ptr: *mut HostWindowState,
    hwnd: HWND,
    notify_hwnd: Option<isize>,
    wv_sender: Option<&ICoreWebView2>,
    tx_cb: &mpsc::Sender<String>,
) {
    let act = handle_host_message(msg_str, db, notify_hwnd);
    let st = unsafe { &mut *state_ptr };
    if let Some(theme) = &act.apply_theme {
        st.theme = theme.clone();
        apply_host_window_theme(hwnd, theme);
    }
    if act.pop_out_window {
        unsafe {
            promote_inline_to_window(
                hwnd,
                st.pref_w,
                st.pref_h,
                &st.base_title,
                &st.theme,
                (st.icons.0, st.icons.1),
                st.inline_owner.or(notify_hwnd),
            );
        }
        st.inline_owner = None;
    }
    if act.window_ready && !st.window_revealed {
        st.window_revealed = true;
        unsafe {
            reveal_host_window_and_notify(hwnd, TIMER_REVEAL_FALLBACK, notify_hwnd);
        }
    }
    if act.toggle_topmost {
        let bt = st.base_title.clone();
        unsafe {
            toggle_pin_topmost_window(hwnd, &bt, &mut st.is_topmost);
        }
    }
    if act.pick_folder_mode != 0 {
        // Defer modal IFileOpenDialog out of WebMessageReceived to avoid WebView2 COM reentrancy.
        let _ = unsafe {
            PostMessageW(Some(hwnd), WM_APP_PICK_FOLDER, WPARAM(act.pick_folder_mode as usize), LPARAM(0))
        };
    } else if let Some(req) = act.pick_plugin_path.clone() {
        st.pending_plugin_path = Some(req);
        let _ = unsafe {
            PostMessageW(Some(hwnd), WM_APP_PICK_FOLDER, WPARAM(4), LPARAM(0))
        };
    }
    if let Some(reply) = &act.reply_json
        && let Some(wv) = wv_sender
    {
        let rw = HSTRING::from(reply);
        let _ = unsafe { wv.PostWebMessageAsJson(PCWSTR(rw.as_ptr())) };
    }
    let hwnd_raw = hwnd.0 as usize;
    if act.hotkey_capture_cmd > 0 {
        st.hk_capture.start(hwnd_raw, tx_cb.clone(), WM_APP_WEBVIEW_EVENT);
    } else if act.hotkey_capture_cmd < 0 {
        st.hk_capture.stop();
    }
    let should_close = act.should_close;
    let plugin_id = st.plugin_id.clone();
    dispatch_async_workers(act, &plugin_id, notify_hwnd, tx_cb, hwnd_raw, WM_APP_WEBVIEW_EVENT);
    if should_close {
        let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }
}
