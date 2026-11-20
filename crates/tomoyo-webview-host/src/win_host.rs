//! Native Win32 WebView2 container window, DWM titlebar theme sync, topmost pin & async worker relay.

use crate::bridge::handle_host_message;
use crate::db::PluginDb;
use crate::shim_js::build_init_script;
use crate::store::fetch_store_catalog_json;
use crate::{assemble_settings_html, build_settings_init_script, LaunchArgs};
use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use tomoyo_core::{
    download_and_verify_release_asset, fetch_latest_github_release,
    launch_verified_update_installer, locate_fonts_dir, ReleaseInfo, TomoyoConfig, UpdateStatus,
    CURRENT_VERSION,
};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
    ICoreWebView2_3, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    WebMessageReceivedEventHandler,
};
use windows::core::{w, HSTRING, Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, LoadCursorW, LoadIconW, PostMessageW, PostQuitMessage,
    RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    TranslateMessage, GWLP_USERDATA, HWND_NOTOPMOST, HWND_TOPMOST, ICON_BIG, ICON_SMALL, IDC_ARROW,
    MSG, SM_CXSCREEN, SM_CYSCREEN, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_SHOW, WM_CLOSE, WM_DESTROY, WM_SETICON, WM_SIZE, WNDCLASSW,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

const WM_APP_WEBVIEW_EVENT: u32 = 0x8000 + 31;

struct HostWindowState {
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    event_rx: mpsc::Receiver<String>,
    base_title: String,
    is_topmost: bool,
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
        WM_CLOSE => {
            if !ptr.is_null() {
                let st = unsafe { &mut *ptr };
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
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let hinst = unsafe { GetModuleHandleW(None).unwrap() };
    let embedded_icon = unsafe { LoadIconW(Some(hinst.into()), PCWSTR(1 as *const u16)).ok() };
    let cls = w!("TomoyoEphemeralWebViewHost");
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
            Default::default(),
            cls,
            PCWSTR(title_w.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            x,
            y,
            opts.width,
            opts.height,
            None,
            None,
            Some(hinst.into()),
            None,
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
        event_rx,
        base_title: opts.title.clone(),
        is_topmost: false,
    });
    let state_ptr = &mut *state as *mut HostWindowState;
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    let plugin_db = Rc::new(RefCell::new(PluginDb::open(&opts.plugin_id)));
    let settings_mode = opts.settings_mode;
    let notify_hwnd = opts.notify_hwnd;
    let init_script = if settings_mode {
        build_settings_init_script()
    } else {
        build_init_script(
            &opts.plugin_id,
            &opts.query,
            opts.theme != "light",
            opts.utools_compat,
            &plugin_db.borrow().snapshot_json(),
        )
    };
    let entry_url = if settings_mode { String::new() } else { to_file_url(&opts.entry) };

    let user_data_dir = tomoyo_core::data_dir().join("webview-cache");
    let _ = std::fs::create_dir_all(&user_data_dir);
    let user_data_w = HSTRING::from(user_data_dir.to_string_lossy().as_ref());
    let fonts_dir_opt = locate_fonts_dir();
    let db_for_msg = Rc::clone(&plugin_db);

    let env_handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |res, env| {
            let Some(env) = env.filter(|_| res.is_ok()) else {
                return Ok(());
            };
            let init_script_inner = init_script.clone();
            let entry_url_inner = entry_url.clone();
            let fonts_inner = fonts_dir_opt.clone();
            let db_inner = Rc::clone(&db_for_msg);
            let tx_inner = event_tx.clone();

            let ctrl_handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |res, controller| {
                    let Some(controller) = controller.filter(|_| res.is_ok()) else {
                        return Ok(());
                    };
                    unsafe {
                        let mut rc = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rc);
                        let _ = controller.SetBounds(rc);
                        let _ = controller.SetIsVisible(true);

                        if let Ok(webview) = controller.CoreWebView2() {
                            if let Some(fdir) = &fonts_inner {
                                if let Ok(wv3) = webview.cast::<ICoreWebView2_3>() {
                                    let dir_w = HSTRING::from(fdir.to_string_lossy().as_ref());
                                    let _ = wv3.SetVirtualHostNameToFolderMapping(
                                        w!("tomoyo-fonts.local"),
                                        PCWSTR(dir_w.as_ptr()),
                                        COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
                                    );
                                }
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
                                            if let Some(reply) = &act.reply_json {
                                                if let Some(wv) = &wv_sender {
                                                    let rw = HSTRING::from(reply);
                                                    let _ = wv.PostWebMessageAsJson(PCWSTR(rw.as_ptr()));
                                                }
                                            }
                                            if let Some(repo) = act.async_check_updates {
                                                spawn_update_worker(repo, tx_cb.clone(), hwnd_raw);
                                            }
                                            if let Some(rel) = act.async_install_update {
                                                spawn_install_update_worker(rel, tx_cb.clone(), hwnd_raw);
                                            }
                                            if let Some(sources) = act.async_fetch_store {
                                                spawn_store_worker(sources, tx_cb.clone(), hwnd_raw);
                                            }
                                            if act.should_close {
                                                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                                            }
                                        }
                                    }
                                    Ok(())
                                },
                            ));
                            let _ = webview.add_WebMessageReceived(&msg_handler, &mut token);

                            if settings_mode {
                                let html = assemble_settings_html();
                                let html_w = HSTRING::from(html);
                                let _ = webview.NavigateToString(PCWSTR(html_w.as_ptr()));
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

fn spawn_update_worker(repo: String, tx: mpsc::Sender<String>, hwnd_raw: usize) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-settings-updater".into())
        .spawn(move || {
            let st = fetch_latest_github_release(&repo, CURRENT_VERSION);
            if matches!(st, UpdateStatus::UpToDate { .. } | UpdateStatus::Available { .. }) {
                let mut cfg = TomoyoConfig::load_or_create();
                cfg.update.last_checked_unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = cfg.save();
            }
            let evt = serde_json::json!({ "event": "update_status", "status": st }).to_string();
            if tx.send(evt).is_ok() {
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd_raw as *mut c_void)), WM_APP_WEBVIEW_EVENT, WPARAM(0), LPARAM(0));
                }
            }
        });
}

fn spawn_install_update_worker(rel: ReleaseInfo, tx: mpsc::Sender<String>, hwnd_raw: usize) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-settings-installer".into())
        .spawn(move || {
            let res = download_and_verify_release_asset(&rel)
                .and_then(|path| launch_verified_update_installer(&path));
            let evt = match res {
                Ok(started_exe) => serde_json::json!({
                    "event": "update_install_result",
                    "ok": true,
                    "launched_installer": started_exe,
                    "message": if started_exe {
                        "SHA-256 校验通过，正在启动静默覆盖安装并重启更新..."
                    } else {
                        "SHA-256 校验通过，已在资源管理器选中便携包"
                    }
                }),
                Err(e) => serde_json::json!({
                    "event": "update_install_result",
                    "ok": false,
                    "launched_installer": false,
                    "message": e
                }),
            }
            .to_string();
            if tx.send(evt).is_ok() {
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd_raw as *mut c_void)), WM_APP_WEBVIEW_EVENT, WPARAM(0), LPARAM(0));
                }
            }
        });
}

fn spawn_store_worker(sources: Vec<String>, tx: mpsc::Sender<String>, hwnd_raw: usize) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-settings-store".into())
        .spawn(move || {
            let evt = fetch_store_catalog_json(&sources);
            if tx.send(evt).is_ok() {
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd_raw as *mut c_void)), WM_APP_WEBVIEW_EVENT, WPARAM(0), LPARAM(0));
                }
            }
        });
}

fn to_file_url(entry: &str) -> String {
    if entry.starts_with("http://") || entry.starts_with("https://") || entry.starts_with("file://") {
        return entry.to_string();
    }
    let abs = std::fs::canonicalize(PathBuf::from(entry)).unwrap_or_else(|_| PathBuf::from(entry));
    let s = abs.to_string_lossy();
    let cleaned = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    format!("file:///{cleaned}")
}
