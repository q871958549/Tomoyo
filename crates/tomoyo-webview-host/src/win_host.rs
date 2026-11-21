//! Native Win32 WebView2 container window, DWM titlebar theme sync, topmost pin & async worker relay.

use crate::bridge::handle_host_message;
use crate::clip_view::{assemble_clip_manager_html, is_clip_manager_entry, load_clip_manager_bootstrap};
use crate::db::PluginDb;
use crate::ocr_view::{assemble_ocr_viewer_html, is_ocr_viewer_entry, load_ocr_viewer_bootstrap};
use crate::shim_js::build_init_script;
use crate::single_instance::{
    acquire_or_activate_clipboard, acquire_or_activate_settings, acquire_or_activate_translator,
    CLIPBOARD_WINDOW_CLASS, EPHEMERAL_WINDOW_CLASS, SETTINGS_WINDOW_CLASS, TRANSLATOR_WINDOW_CLASS,
    WM_APP_ACTIVATE_CLIPBOARD, WM_APP_ACTIVATE_SETTINGS, WM_APP_ACTIVATE_TRANSLATOR,
};
use crate::tr_view::{
    assemble_translator_html, is_translator_entry, load_translator_bootstrap,
    read_latest_translate_req_event,
};
use crate::hotkey_capture::HotkeyCaptureController;
use crate::win_theme::{apply_host_window_theme, map_vdir, to_file_url};
use crate::win_workers::dispatch_async_workers;
use crate::{assemble_settings_html, build_settings_init_script, LaunchArgs};
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
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, IsIconic, LoadCursorW, LoadIconW, PostMessageW,
    PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, GWLP_USERDATA, HWND_NOTOPMOST,
    HWND_TOPMOST, ICON_BIG, ICON_SMALL, IDC_ARROW, MSG, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, SW_SHOW, WM_ACTIVATE, WM_CLOSE, WM_DESTROY, WM_SETICON,
    WM_SIZE, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

const WM_APP_WEBVIEW_EVENT: u32 = 0x8000 + 31;
const WM_APP_PICK_FOLDER: u32 = 0x8000 + 33;

struct HostWindowState {
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    event_tx: mpsc::Sender<String>,
    event_rx: mpsc::Receiver<String>,
    base_title: String,
    is_topmost: bool,
    picking_folder: bool,
    settings_mode: bool,
    notify_hwnd: Option<isize>,
    last_activate_probe: Instant,
    hk_capture: HotkeyCaptureController,
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
                    let reply = if wparam.0 == 3 {
                        let (r, changed) = crate::folder_picker::pick_sync_folder(hwnd);
                        if changed {
                            if let Some(t) = st.notify_hwnd { crate::bridge_os::notify_parent_mode(t, 1); }
                        }
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
                    if let Some(p) = st.notify_hwnd {
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
        WM_CLOSE => {
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
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn run_windows_host(opts: LaunchArgs) {
    let clip_mode = is_clip_manager_entry(&opts.entry);
    let tr_mode = is_translator_entry(&opts.entry);
    let _singleton_guard = if opts.settings_mode {
        match acquire_or_activate_settings() {
            Some(guard) => Some(guard),
            None => return,
        }
    } else if clip_mode {
        match acquire_or_activate_clipboard() {
            Some(guard) => Some(guard),
            None => return,
        }
    } else if tr_mode {
        match acquire_or_activate_translator() {
            Some(guard) => Some(guard),
            None => return,
        }
    } else {
        None
    };

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let hinst = unsafe { GetModuleHandleW(None).unwrap() };
    let embedded_icon = unsafe { LoadIconW(Some(hinst.into()), PCWSTR(1 as *const u16)).ok() };
    let cls = if opts.settings_mode {
        SETTINGS_WINDOW_CLASS
    } else if clip_mode {
        CLIPBOARD_WINDOW_CLASS
    } else if tr_mode {
        TRANSLATOR_WINDOW_CLASS
    } else {
        EPHEMERAL_WINDOW_CLASS
    };
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            hIcon: embedded_icon.unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: cls,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
    }

    let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let x = ((sw - opts.width) / 2).max(40);
    let y = ((sh - opts.height) / 2).max(40);
    let title_w = HSTRING::from(opts.title.clone());

    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(), cls, PCWSTR(title_w.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE, x, y, opts.width, opts.height,
            None, None, Some(hinst.into()), None,
        )
        .expect("CreateWindowExW failed")
    };

    if let Some(ic) = embedded_icon {
        unsafe {
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(ic.0 as isize)));
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(ic.0 as isize)));
        }
    }
    apply_host_window_theme(hwnd, &opts.theme);

    let (event_tx, event_rx) = mpsc::channel::<String>();
    let mut state = Box::new(HostWindowState {
        controller: None,
        webview: None,
        event_tx: event_tx.clone(),
        event_rx,
        base_title: opts.title.clone(),
        is_topmost: false,
        picking_folder: false,
        settings_mode: opts.settings_mode,
        notify_hwnd: opts.notify_hwnd,
        last_activate_probe: Instant::now(),
        hk_capture: HotkeyCaptureController::new(opts.notify_hwnd),
    });
    let state_ptr = &mut *state as *mut HostWindowState;
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    let plugin_db = Rc::new(RefCell::new(PluginDb::open(&opts.plugin_id)));
    let settings_mode = opts.settings_mode;
    let ocr_mode = is_ocr_viewer_entry(&opts.entry);
    let notify_hwnd = opts.notify_hwnd;
    let (init_script, ocr_img_dir, clip_img_dir) = if settings_mode {
        (build_settings_init_script(), None, None)
    } else if ocr_mode {
        let boot = load_ocr_viewer_bootstrap(&opts.theme, &opts.query);
        (boot.init_script, boot.image_dir, None)
    } else if clip_mode {
        let boot = load_clip_manager_bootstrap(&opts.theme, &opts.query);
        (boot.init_script, None, Some(boot.images_dir))
    } else if tr_mode {
        (load_translator_bootstrap(&opts.theme), None, None)
    } else {
        (build_init_script(&opts.plugin_id, &opts.query, opts.theme != "light", opts.utools_compat, &plugin_db.borrow().snapshot_json()), None, None)
    };
    let entry_url = if settings_mode || ocr_mode || clip_mode || tr_mode {
        String::new()
    } else {
        to_file_url(&opts.entry)
    };

    let user_data_dir = tomoyo_core::data_dir().join("webview-cache");
    let _ = std::fs::create_dir_all(&user_data_dir);
    let user_data_w = HSTRING::from(user_data_dir.to_string_lossy().as_ref());
    let fonts_dir_opt = locate_fonts_dir();
    let db_for_msg = Rc::clone(&plugin_db);

    let env_handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |res, env| {
            let Some(env) = env.filter(|_| res.is_ok()) else {
                tracing::error!("WebView2 environment creation failed: {res:?}");
                return Ok(());
            };
            let init_script_inner = init_script.clone();
            let entry_url_inner = entry_url.clone();
            let fonts_inner = fonts_dir_opt.clone();
            let ocr_dir_inner = ocr_img_dir.clone();
            let clip_dir_inner = clip_img_dir.clone();
            let db_inner = Rc::clone(&db_for_msg);
            let tx_inner = event_tx.clone();

            let ctrl_handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |res, controller| {
                    let Some(controller) = controller.filter(|_| res.is_ok()) else {
                        tracing::error!("WebView2 controller creation failed: {res:?}");
                        return Ok(());
                    };
                    unsafe {
                        let mut rc = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rc);
                        let _ = controller.SetBounds(rc);
                        let _ = controller.SetIsVisible(true);

                        if let Ok(webview) = controller.CoreWebView2() {
                            if let Ok(wv3) = webview.cast::<ICoreWebView2_3>() {
                                map_vdir(&wv3, w!("tomoyo-fonts.local"), fonts_inner.as_ref());
                                map_vdir(&wv3, w!("tomoyo-ocr-img.local"), ocr_dir_inner.as_ref());
                                map_vdir(&wv3, w!("tomoyo-clip-img.local"), clip_dir_inner.as_ref());
                            }
                            let script_w = HSTRING::from(&init_script_inner);
                            let _ = webview.AddScriptToExecuteOnDocumentCreated(PCWSTR(script_w.as_ptr()), None);

                            let db_cb = Rc::clone(&db_inner);
                            let tx_cb = tx_inner.clone();
                            let hwnd_raw = hwnd.0 as usize;
                            let mut token = 0i64;
                            let msg_handler = WebMessageReceivedEventHandler::create(Box::new(
                                move |wv_sender, args| {
                                    if let Some(args) = args {
                                        let mut raw = PWSTR::null();
                                        if args.TryGetWebMessageAsString(&mut raw).is_ok() && !raw.is_null() {
                                            let s = raw.to_string().unwrap_or_default();
                                            windows::Win32::System::Com::CoTaskMemFree(Some(raw.0 as *const c_void));
                                            let act = handle_host_message(&s, &mut db_cb.borrow_mut(), notify_hwnd);
                                            if let Some(theme) = &act.apply_theme {
                                                apply_host_window_theme(hwnd, theme);
                                            }
                                            if act.toggle_topmost {
                                                toggle_pin_topmost(hwnd, &mut *state_ptr);
                                            }
                                            if act.pick_folder_mode != 0 {
                                                // Defer modal IFileOpenDialog out of WebMessageReceived to avoid WebView2 COM reentrancy.
                                                let _ = PostMessageW(Some(hwnd), WM_APP_PICK_FOLDER, WPARAM(act.pick_folder_mode as usize), LPARAM(0));
                                            }
                                            if let Some(reply) = &act.reply_json {
                                                if let Some(wv) = &wv_sender {
                                                    let rw = HSTRING::from(reply);
                                                    let _ = wv.PostWebMessageAsJson(PCWSTR(rw.as_ptr()));
                                                }
                                            }
                                            if act.hotkey_capture_cmd > 0 {
                                                (*state_ptr).hk_capture.start(hwnd_raw, tx_cb.clone(), WM_APP_WEBVIEW_EVENT);
                                            } else if act.hotkey_capture_cmd < 0 {
                                                (*state_ptr).hk_capture.stop();
                                            }
                                            let should_close = act.should_close;
                                            dispatch_async_workers(act, notify_hwnd, &tx_cb, hwnd_raw, WM_APP_WEBVIEW_EVENT);
                                            if should_close {
                                                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                                            }
                                        }
                                    }
                                    Ok(())
                                },
                            ));
                            let _ = webview.add_WebMessageReceived(&msg_handler, &mut token);

                            if settings_mode {
                                let html = HSTRING::from(assemble_settings_html());
                                let _ = webview.NavigateToString(PCWSTR(html.as_ptr()));
                            } else if ocr_mode {
                                let html = HSTRING::from(assemble_ocr_viewer_html());
                                let _ = webview.NavigateToString(PCWSTR(html.as_ptr()));
                            } else if clip_mode {
                                let html = HSTRING::from(assemble_clip_manager_html());
                                let _ = webview.NavigateToString(PCWSTR(html.as_ptr()));
                            } else if tr_mode {
                                let html = HSTRING::from(assemble_translator_html());
                                let _ = webview.NavigateToString(PCWSTR(html.as_ptr()));
                            } else {
                                let url_w = HSTRING::from(&entry_url_inner);
                                let _ = webview.Navigate(PCWSTR(url_w.as_ptr()));
                            }
                            (*state_ptr).webview = Some(webview);
                        }
                        (*state_ptr).controller = Some(controller);
                    }
                    Ok(())
                },
            ));

            unsafe {
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

unsafe fn toggle_pin_topmost(hwnd: HWND, st: &mut HostWindowState) {
    st.is_topmost = !st.is_topmost;
    let insert_after = if st.is_topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    unsafe {
        let _ = SetWindowPos(hwnd, Some(insert_after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        let new_title = if st.is_topmost {
            format!("📌 {}", st.base_title)
        } else {
            st.base_title.clone()
        };
        let tw = HSTRING::from(new_title);
        let _ = SetWindowTextW(hwnd, PCWSTR(tw.as_ptr()));
    }
}
