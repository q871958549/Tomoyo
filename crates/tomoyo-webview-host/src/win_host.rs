//! Native Win32 WebView2 container window, DWM titlebar theme sync, topmost pin & async worker relay.

use crate::clip_view::is_clip_manager_entry;
use crate::db::PluginDb;
use crate::hotkey_capture::HotkeyCaptureController;
use crate::inline_dock::{
    clear_inline_hwnd_marker, create_inline_dock_window, inline_dock_overlay_js,
    prepare_host_bootstrap, write_inline_hwnd_marker,
};
use crate::single_instance::{
    acquire_or_activate_clipboard, acquire_or_activate_settings, acquire_or_activate_translator,
    CLIPBOARD_WINDOW_CLASS, EPHEMERAL_WINDOW_CLASS, SETTINGS_WINDOW_CLASS, TRANSLATOR_WINDOW_CLASS,
    WM_APP_ACTIVATE_CLIPBOARD, WM_APP_ACTIVATE_SETTINGS, WM_APP_ACTIVATE_TRANSLATOR,
};
use crate::tr_view::{is_translator_entry, read_latest_translate_req_event};
use crate::win_msg::{dispatch_webview_message, pick_builtin_html};
use crate::win_theme::{
    configure_controller_visuals, create_themed_host_window, destroy_owned_icons, load_window_icons,
    map_vdir, reveal_host_window_and_notify,
};
use crate::LaunchArgs;
use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tomoyo_core::locate_fonts_dir;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
    ICoreWebView2_3,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    WebMessageReceivedEventHandler,
};
use windows::core::{w, HSTRING, Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, IsIconic, IsWindow, PostQuitMessage, SetForegroundWindow,
    SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage, GWLP_USERDATA, MSG, SW_RESTORE,
    SW_SHOW, WM_ACTIVATE, WM_CLOSE, WM_DESTROY, WM_SIZE, WM_TIMER,
};

pub(crate) const WM_APP_WEBVIEW_EVENT: u32 = 0x8000 + 31;
pub(crate) const WM_APP_PICK_FOLDER: u32 = 0x8000 + 33;
pub(crate) const TIMER_REVEAL_FALLBACK: usize = 901;

pub(crate) struct HostWindowState {
    pub controller: Option<ICoreWebView2Controller>,
    pub webview: Option<ICoreWebView2>,
    pub event_tx: mpsc::Sender<String>,
    pub event_rx: mpsc::Receiver<String>,
    pub base_title: String,
    pub plugin_id: String,
    pub theme: String,
    pub pref_w: i32,
    pub pref_h: i32,
    pub icons: (Option<windows::Win32::UI::WindowsAndMessaging::HICON>, Option<windows::Win32::UI::WindowsAndMessaging::HICON>, bool),
    pub is_topmost: bool,
    pub picking_folder: bool,
    pub pending_plugin_path: Option<(String, String, bool)>,
    pub settings_mode: bool,
    pub window_revealed: bool,
    pub notify_hwnd: Option<isize>,
    pub inline_owner: Option<isize>,
    pub last_activate_probe: Instant,
    pub hk_capture: HotkeyCaptureController,
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut HostWindowState;
    match msg {
        WM_SIZE => {
            if !ptr.is_null() {
                let st = unsafe { &*ptr };
                if let Some(ctrl) = &st.controller {
                    let mut rc = RECT::default();
                    let _ = unsafe { GetClientRect(hwnd, &mut rc) };
                    let _ = unsafe { ctrl.SetBounds(rc) };
                }
            }
            LRESULT(0)
        }
        WM_APP_WEBVIEW_EVENT => {
            if !ptr.is_null() {
                let st = unsafe { &mut *ptr };
                while let Ok(json) = st.event_rx.try_recv() {
                    if let Some(wv) = &st.webview {
                        let jw = HSTRING::from(&json);
                        let _ = unsafe { wv.PostWebMessageAsJson(PCWSTR(jw.as_ptr())) };
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_PICK_FOLDER => {
            if !ptr.is_null() {
                let st = unsafe { &mut *ptr };
                if !st.picking_folder {
                    st.picking_folder = true;
                    let reply = if wparam.0 == 4 {
                        if let Some((pid, key, is_file)) = st.pending_plugin_path.take() {
                            crate::folder_picker::pick_plugin_path(hwnd, &pid, &key, is_file)
                        } else {
                            serde_json::json!({ "event": "plugin_path_picked", "ok": false, "cancelled": true }).to_string()
                        }
                    } else if wparam.0 == 3 {
                        let (r, changed) = crate::folder_picker::pick_sync_folder(hwnd);
                        if changed
                            && let Some(t) = st.notify_hwnd { crate::bridge_os::notify_parent_mode(t, 1); }
                        r
                    } else if wparam.0 == 2 {
                        crate::folder_picker::pick_capture_save_folder(hwnd)
                    } else {
                        crate::folder_picker::pick_scan_folder(hwnd)
                    };
                    st.picking_folder = false;
                    if let Some(wv) = &st.webview {
                        let rw = HSTRING::from(&reply);
                        let _ = unsafe { wv.PostWebMessageAsJson(PCWSTR(rw.as_ptr())) };
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_ACTIVATE_SETTINGS | WM_APP_ACTIVATE_CLIPBOARD | WM_APP_ACTIVATE_TRANSLATOR => {
            unsafe {
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
                if msg == WM_APP_ACTIVATE_TRANSLATOR && !ptr.is_null() {
                    let st = &*ptr;
                    if let Some(wv) = &st.webview {
                        let evt = HSTRING::from(read_latest_translate_req_event());
                        let _ = wv.PostWebMessageAsJson(PCWSTR(evt.as_ptr()));
                    }
                }
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if !ptr.is_null() && (wparam.0 & 0xFFFF) != 0 {
                let st = unsafe { &mut *ptr };
                if st.settings_mode
                    && !st.hk_capture.is_active()
                    && st.webview.is_some()
                    && st.last_activate_probe.elapsed() >= Duration::from_millis(1200)
                {
                    st.last_activate_probe = Instant::now();
                    if tomoyo_core::load_hotkey_health().has_unresolved_conflicts()
                        && let Some(p) = st.notify_hwnd
                    {
                        crate::bridge_os::notify_parent_mode(p, 4);
                    }
                    crate::win_workers::spawn_hotkey_health_worker(
                        st.event_tx.clone(),
                        hwnd.0 as usize,
                        WM_APP_WEBVIEW_EVENT,
                    );
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_TIMER => {
            if wparam.0 == TIMER_REVEAL_FALLBACK && !ptr.is_null() {
                let st = unsafe { &mut *ptr };
                if !st.window_revealed {
                    st.window_revealed = true;
                    unsafe { reveal_host_window_and_notify(hwnd, TIMER_REVEAL_FALLBACK, st.notify_hwnd); }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            crate::plugin_rpc::shutdown_rpc_host();
            if !ptr.is_null() {
                let st = unsafe { &mut *ptr };
                st.hk_capture.stop();
                st.webview = None;
                if let Some(ctrl) = st.controller.take() {
                    let _ = unsafe { ctrl.Close() };
                }
            }
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                clear_inline_hwnd_marker(hwnd);
                if !ptr.is_null() {
                    let st = &*ptr;
                    destroy_owned_icons(st.icons.0, st.icons.1, st.icons.2);
                }
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn run_windows_host(opts: LaunchArgs) {
    let clip_mode = is_clip_manager_entry(&opts.entry);
    let tr_mode = is_translator_entry(&opts.entry);
    let _singleton_guard = if opts.settings_mode {
        let Some(g) = acquire_or_activate_settings() else { return };
        Some(g)
    } else if clip_mode {
        let Some(g) = acquire_or_activate_clipboard() else { return };
        Some(g)
    } else if tr_mode {
        let Some(g) = acquire_or_activate_translator() else { return };
        Some(g)
    } else {
        None
    };

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let cls = match (opts.settings_mode, clip_mode, tr_mode) {
        (true, _, _) => SETTINGS_WINDOW_CLASS,
        (_, true, _) => CLIPBOARD_WINDOW_CLASS,
        (_, _, true) => TRANSLATOR_WINDOW_CLASS,
        _ => EPHEMERAL_WINDOW_CLASS,
    };
    let hinst = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap() };
    let icons = load_window_icons(hinst, opts.icon.as_deref());
    let docked_inline = !opts.settings_mode && opts.inline_hwnd.is_some() && opts.dock_rect.is_some();
    let hwnd = unsafe {
        if let (Some(owner), Some(rect)) = (opts.inline_hwnd, opts.dock_rect) && docked_inline {
            let h = create_inline_dock_window(cls, Some(wndproc), &opts.title, &opts.theme, owner, rect, (icons.0, icons.1));
            write_inline_hwnd_marker(h, std::process::id(), &opts.plugin_id);
            h
        } else {
            create_themed_host_window(cls, Some(wndproc), &opts.title, &opts.theme, opts.width, opts.height, !opts.settings_mode, (icons.0, icons.1))
        }
    };

    let (event_tx, event_rx) = mpsc::channel::<String>();
    let mut state = Box::new(HostWindowState {
        controller: None,
        webview: None,
        event_tx: event_tx.clone(),
        event_rx,
        base_title: opts.title.clone(),
        plugin_id: opts.plugin_id.clone(),
        theme: opts.theme.clone(),
        pref_w: opts.width,
        pref_h: opts.height,
        icons,
        is_topmost: false,
        picking_folder: false,
        pending_plugin_path: None,
        settings_mode: opts.settings_mode,
        window_revealed: !opts.settings_mode,
        notify_hwnd: opts.notify_hwnd,
        inline_owner: opts.inline_hwnd,
        last_activate_probe: Instant::now(),
        hk_capture: HotkeyCaptureController::new(opts.notify_hwnd),
    });
    let state_ptr = &mut *state as *mut HostWindowState;
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        if opts.settings_mode {
            let _ = SetTimer(Some(hwnd), TIMER_REVEAL_FALLBACK, 360, None);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
    }

    let plugin_db = Rc::new(RefCell::new(PluginDb::open(&opts.plugin_id)));
    let settings_mode = opts.settings_mode;
    let notify_hwnd = opts.notify_hwnd;
    let boot = prepare_host_bootstrap(&opts, &plugin_db.borrow());
    let (init_script, clip_img_dir, entry_url) = (
        boot.init_script,
        boot.clip_img_dir,
        boot.entry_url,
    );

    let user_data_dir = tomoyo_core::data_dir().join("webview-cache");
    let _ = std::fs::create_dir_all(&user_data_dir);
    let user_data_w = HSTRING::from(user_data_dir.to_string_lossy().as_ref());
    let fonts_dir_opt = locate_fonts_dir();
    let db_for_msg = Rc::clone(&plugin_db);
    let theme_clone = opts.theme.clone();
    let devtools_on = opts.devtools;

    let env_handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |res, env| {
            let Some(env) = env.filter(|_| res.is_ok()) else {
                tracing::error!("WebView2 environment creation failed: {res:?}");
                return Ok(());
            };
            let init_script_inner = init_script.clone();
            let entry_url_inner = entry_url.clone();
            let fonts_inner = fonts_dir_opt.clone();
            let clip_dir_inner = clip_img_dir.clone();
            let db_inner = Rc::clone(&db_for_msg);
            let tx_inner = event_tx.clone();
            let theme_inner = theme_clone.clone();
            let devtools_inner = devtools_on;

            let ctrl_handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |res, controller| {
                    let Some(controller) = controller.filter(|_| res.is_ok()) else {
                        if !unsafe { IsWindow(Some(hwnd)).as_bool() } {
                            tracing::debug!("WebView2 controller aborted: host window closed early");
                        } else {
                            tracing::error!("WebView2 controller creation failed: {res:?}");
                        }
                        return Ok(());
                    };
                    if !unsafe { IsWindow(Some(hwnd)).as_bool() } {
                        let _ = unsafe { controller.Close() };
                        return Ok(());
                    }
                    unsafe {
                        let mut rc = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rc);
                        let _ = controller.SetBounds(rc);
                        let _ = controller.SetIsVisible(true);
                        configure_controller_visuals(&controller, &theme_inner);

                        if let Ok(webview) = controller.CoreWebView2() {
                            if let Ok(wv3) = webview.cast::<ICoreWebView2_3>() {
                                map_vdir(&wv3, w!("tomoyo-fonts.local"), fonts_inner.as_ref());
                                map_vdir(&wv3, w!("tomoyo-clip-img.local"), clip_dir_inner.as_ref());
                            }
                            let script_w = HSTRING::from(&init_script_inner);
                            let _ = webview.AddScriptToExecuteOnDocumentCreated(PCWSTR(script_w.as_ptr()), None);
                            if docked_inline {
                                let dock_w = HSTRING::from(inline_dock_overlay_js(&theme_inner));
                                let _ = webview.AddScriptToExecuteOnDocumentCreated(PCWSTR(dock_w.as_ptr()), None);
                            }

                            let db_cb = Rc::clone(&db_inner);
                            let tx_cb = tx_inner.clone();
                            let mut token = 0i64;
                            let msg_handler = WebMessageReceivedEventHandler::create(Box::new(
                                move |wv_sender, args| {
                                    if let Some(args) = args {
                                        let mut raw = PWSTR::null();
                                        if args.TryGetWebMessageAsString(&mut raw).is_ok() && !raw.is_null() {
                                            let s = raw.to_string().unwrap_or_default();
                                            windows::Win32::System::Com::CoTaskMemFree(Some(raw.0 as *const c_void));
                                            dispatch_webview_message(&s, &mut db_cb.borrow_mut(), state_ptr, hwnd, notify_hwnd, wv_sender.as_ref(), &tx_cb);
                                        }
                                    }
                                    Ok(())
                                },
                            ));
                            let _ = webview.add_WebMessageReceived(&msg_handler, &mut token);

                            if let Some(html) = pick_builtin_html(settings_mode, clip_mode, tr_mode) {
                                let hw = HSTRING::from(html);
                                let _ = webview.NavigateToString(PCWSTR(hw.as_ptr()));
                            } else {
                                let url_w = HSTRING::from(&entry_url_inner);
                                let _ = webview.Navigate(PCWSTR(url_w.as_ptr()));
                            }
                            if devtools_inner {
                                if let Ok(s) = webview.Settings() {
                                    let _ = s.SetAreDevToolsEnabled(true);
                                }
                                let _ = webview.OpenDevToolsWindow();
                            }
                            (*state_ptr).webview = Some(webview);
                        }
                        (*state_ptr).controller = Some(controller);
                    }
                    Ok(())
                },
            ));

            unsafe {
                if !IsWindow(Some(hwnd)).as_bool() {
                    tracing::debug!("Skipping WebView2 controller creation: host window already closed");
                    return Ok(());
                }
                let _ = env.CreateCoreWebView2Controller(hwnd, &ctrl_handler);
            }
            Ok(())
        },
    ));

    unsafe {
        let _ = CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(user_data_w.as_ptr()),
            None,
            &env_handler,
        );
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        CoUninitialize();
    }
}
