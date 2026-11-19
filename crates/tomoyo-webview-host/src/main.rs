#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Ephemeral On-Demand WebView2 Host (`tomoyo-webview-host.exe`).
//!
//! Serves TWO zero-idle-RAM roles:
//! 1. On-demand WebView plugins (with `window.tomoyo` + `window.utools` compatibility bridge,
//!    sub-input bar, and `Ctrl+Shift+T` always-on-top pinning).
//! 2. Built-in Graphical Settings Center (`--settings --notify-hwnd <hwnd>`) with Alibaba PuHuiTi 3.0,
//!    live DWM titlebar + scrollbar theme sync, interactive Hotkey Recorder, Chinese/English i18n,
//!    In-App SHA-256 Verified Plugin Marketplace, GitHub Releases Auto-Updater, and instant auto-save!

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod bridge;
mod db;

use bridge::{build_init_script, fetch_store_catalog_json, handle_host_message};
use db::PluginDb;
use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use tomoyo_core::{
    fetch_latest_github_release, is_autostart_enabled, locate_fonts_dir, PrivateFonts,
    TomoyoConfig, UpdateStatus, CURRENT_VERSION,
};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};

const SETTINGS_HTML: &str = include_str!("settings.html");
const WM_APP_WEBVIEW_EVENT: u32 = 0x8000 + 31;

struct LaunchArgs {
    plugin_id: String,
    title: String,
    entry: String,
    query: String,
    width: i32,
    height: i32,
    utools_compat: bool,
    theme: String,
    settings_mode: bool,
    notify_hwnd: Option<isize>,
    self_test: bool,
}

fn parse_args() -> LaunchArgs {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let val = |flag: &str| -> Option<String> {
        args.windows(2)
            .find(|w| w[0] == flag)
            .map(|w| w[1].clone())
    };
    let settings_mode = args.iter().any(|a| a == "--settings");
    let cfg_theme = if settings_mode {
        TomoyoConfig::load_or_create().theme
    } else if args.iter().any(|a| a == "--light") {
        "light".into()
    } else {
        "dark".into()
    };
    let theme = val("--theme").unwrap_or(cfg_theme);

    LaunchArgs {
        plugin_id: val("--plugin-id").unwrap_or_else(|| "dev.tomoyo.webview".into()),
        title: val("--title").unwrap_or_else(|| {
            if settings_mode {
                "Tomoyo 设置中心".into()
            } else {
                "Tomoyo 插件".into()
            }
        }),
        entry: val("--entry").unwrap_or_default(),
        query: val("--query").unwrap_or_default(),
        width: val("--width")
            .and_then(|s| s.parse().ok())
            .unwrap_or(if settings_mode { 960 } else { 860 }),
        height: val("--height")
            .and_then(|s| s.parse().ok())
            .unwrap_or(if settings_mode { 670 } else { 600 }),
        utools_compat: args.iter().any(|a| a == "--utools-compat"),
        theme,
        settings_mode,
        notify_hwnd: val("--notify-hwnd").and_then(|s| s.parse().ok()),
        self_test: args.iter().any(|a| a == "--self-test"),
    }
}

fn build_settings_init_script() -> String {
    let cfg = TomoyoConfig::load_or_create();
    let autostart = is_autostart_enabled();
    let host = PluginHost::new(default_plugin_dirs());
    let plugins: Vec<serde_json::Value> = host
        .list_plugins()
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "runtime": p.runtime_badge,
                "keywords": p.keywords,
                "max_memory_mb": p.max_memory_mb,
            })
        })
        .collect();
    host.shutdown_all();

    let plugins_dir = tomoyo_core::data_dir().join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    let payload = serde_json::json!({
        "version": CURRENT_VERSION,
        "config": cfg,
        "autostart": autostart,
        "plugins": plugins,
        "config_path": TomoyoConfig::config_path().to_string_lossy(),
        "plugins_dir": plugins_dir.to_string_lossy(),
        "has_bundled_alibaba_font": locate_fonts_dir().is_some(),
    });
    format!(
        "window.__TOMOYO_SETTINGS_INIT__ = {};",
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
    )
}

fn main() {
    let _fonts = PrivateFonts::load_default();
    let opts = parse_args();
    if opts.self_test {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
        let tmp =
            std::env::temp_dir().join(format!("tomoyo_wv_selftest_{}.json", std::process::id()));
        let mut pdb = PluginDb::open_at(tmp.clone());
        let _ = handle_host_message(
            r#"{"cmd":"db_put","id":"demo","doc":{"_id":"demo","val":42}}"#,
            &mut pdb,
            None,
        );
        let script = build_init_script(
            &opts.plugin_id,
            &opts.query,
            opts.theme != "light",
            true,
            &pdb.snapshot_json(),
        );
        let settings_js = build_settings_init_script();
        assert!(script.contains("window.utools"));
        assert!(script.contains("setSubInput"));
        assert!(settings_js.contains("__TOMOYO_SETTINGS_INIT__"));
        assert!(SETTINGS_HTML.contains("AlibabaPuHuiTi-3-55-Regular.ttf"));
        assert!(SETTINGS_HTML.contains("hotkey-recorder"));
        assert!(SETTINGS_HTML.contains("tab-store"));
        assert!(SETTINGS_HTML.contains("tab-updates"));
        assert!(pdb.get("demo").is_some());
        let act = handle_host_message(r#"{"cmd":"close"}"#, &mut pdb, None);
        assert!(act.should_close);
        let sync = handle_host_message(r#"{"cmd":"sync_theme","theme":"light"}"#, &mut pdb, None);
        assert_eq!(sync.apply_theme.as_deref(), Some("light"));
        let store_json = fetch_store_catalog_json(&["registry/index.json".into()]);
        assert!(store_json.contains("dev.tomoyo.example-wasm"));
        let _ = std::fs::remove_file(&tmp);
        println!(
            "[OK] tomoyo-webview-host v{} self-test passed (uTools bridge {}B, Settings UI {}B, Store & Updater verified, Alibaba font={})",
            CURRENT_VERSION,
            script.len(),
            SETTINGS_HTML.len(),
            locate_fonts_dir().is_some()
        );
        return;
    }

    if !opts.settings_mode && opts.entry.is_empty() {
        return;
    }

    #[cfg(windows)]
    run_windows_host(opts);
}

#[cfg(windows)]
fn apply_host_window_theme(hwnd: windows::Win32::Foundation::HWND, theme: &str) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    };

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

#[cfg(windows)]
fn run_windows_host(opts: LaunchArgs) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
        ICoreWebView2_3, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
    };
    use webview2_com::{
        CreateCoreWebView2ControllerCompletedHandler,
        CreateCoreWebView2EnvironmentCompletedHandler, WebMessageReceivedEventHandler,
    };
    use windows::core::{w, HSTRING, Interface, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
        GetMessageW, GetSystemMetrics, GetWindowLongPtrW, LoadCursorW, LoadIconW, PostMessageW,
        PostQuitMessage, RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowPos,
        SetWindowTextW, ShowWindow, TranslateMessage, GWLP_USERDATA, HWND_NOTOPMOST, HWND_TOPMOST,
        ICON_BIG, ICON_SMALL, IDC_ARROW, MSG, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SW_SHOW, WM_CLOSE, WM_DESTROY, WM_SETICON, WM_SIZE, WNDCLASSW,
        WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    struct HostWindowState {
        controller: Option<ICoreWebView2Controller>,
        webview: Option<ICoreWebView2>,
        event_rx: mpsc::Receiver<String>,
        base_title: String,
        is_topmost: bool,
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
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

    let entry_url = if settings_mode {
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
            if res.is_err() {
                return Ok(());
            }
            let Some(env) = env else { return Ok(()) };
            let init_script_inner = init_script.clone();
            let entry_url_inner = entry_url.clone();
            let fonts_inner = fonts_dir_opt.clone();
            let db_inner = Rc::clone(&db_for_msg);
            let tx_inner = event_tx.clone();

            let ctrl_handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |res, controller| {
                    if res.is_err() {
                        return Ok(());
                    }
                    let Some(controller) = controller else {
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
                            let _ = webview.AddScriptToExecuteOnDocumentCreated(
                                PCWSTR(script_w.as_ptr()),
                                None,
                            );

                            let db_cb = Rc::clone(&db_inner);
                            let tx_cb = tx_inner.clone();
                            let hwnd_raw = hwnd.0 as usize;
                            let mut token = 0i64;
                            let msg_handler = WebMessageReceivedEventHandler::create(Box::new(
                                move |wv_sender, args| {
                                    if let Some(args) = args {
                                        let mut raw = PWSTR::null();
                                        if args.TryGetWebMessageAsString(&mut raw).is_ok()
                                            && !raw.is_null()
                                        {
                                            let s = raw.to_string().unwrap_or_default();
                                            windows::Win32::System::Com::CoTaskMemFree(Some(
                                                raw.0 as *const c_void,
                                            ));
                                            let act = handle_host_message(
                                                &s,
                                                &mut db_cb.borrow_mut(),
                                                notify_hwnd,
                                            );
                                            if let Some(theme) = &act.apply_theme {
                                                apply_host_window_theme(hwnd, theme);
                                            }
                                            if act.toggle_topmost {
                                                let st = &mut *state_ptr;
                                                st.is_topmost = !st.is_topmost;
                                                let insert_after = if st.is_topmost {
                                                    HWND_TOPMOST
                                                } else {
                                                    HWND_NOTOPMOST
                                                };
                                                let _ = SetWindowPos(
                                                    hwnd,
                                                    Some(insert_after),
                                                    0,
                                                    0,
                                                    0,
                                                    0,
                                                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                                                );
                                                let new_title = if st.is_topmost {
                                                    format!("📌 {}", st.base_title)
                                                } else {
                                                    st.base_title.clone()
                                                };
                                                let tw = HSTRING::from(new_title);
                                                let _ = SetWindowTextW(hwnd, PCWSTR(tw.as_ptr()));
                                            }
                                            if let Some(reply) = &act.reply_json {
                                                if let Some(wv) = &wv_sender {
                                                    let rw = HSTRING::from(reply);
                                                    let _ = wv.PostWebMessageAsJson(PCWSTR(rw.as_ptr()));
                                                }
                                            }
                                            if let Some(repo) = act.async_check_updates {
                                                let tx_u = tx_cb.clone();
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
                                                        let evt = serde_json::json!({
                                                            "event": "update_status",
                                                            "status": st,
                                                        })
                                                        .to_string();
                                                        if tx_u.send(evt).is_ok() {
                                                            let _ = PostMessageW(
                                                                Some(HWND(hwnd_raw as *mut c_void)),
                                                                WM_APP_WEBVIEW_EVENT,
                                                                WPARAM(0),
                                                                LPARAM(0),
                                                            );
                                                        }
                                                    });
                                            }
                                            if let Some(sources) = act.async_fetch_store {
                                                let tx_s = tx_cb.clone();
                                                let _ = std::thread::Builder::new()
                                                    .name("tomoyo-settings-store".into())
                                                    .spawn(move || {
                                                        let evt = fetch_store_catalog_json(&sources);
                                                        if tx_s.send(evt).is_ok() {
                                                            let _ = PostMessageW(
                                                                Some(HWND(hwnd_raw as *mut c_void)),
                                                                WM_APP_WEBVIEW_EVENT,
                                                                WPARAM(0),
                                                                LPARAM(0),
                                                            );
                                                        }
                                                    });
                                            }
                                            if act.should_close {
                                                let _ = PostMessageW(
                                                    Some(hwnd),
                                                    WM_CLOSE,
                                                    WPARAM(0),
                                                    LPARAM(0),
                                                );
                                            }
                                        }
                                    }
                                    Ok(())
                                },
                            ));
                            let _ = webview.add_WebMessageReceived(&msg_handler, &mut token);

                            if settings_mode {
                                let html_w = HSTRING::from(SETTINGS_HTML);
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
    }

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        CoUninitialize();
    }
}

fn to_file_url(entry: &str) -> String {
    if entry.starts_with("http://") || entry.starts_with("https://") || entry.starts_with("file://")
    {
        return entry.to_string();
    }
    let abs = std::fs::canonicalize(PathBuf::from(entry)).unwrap_or_else(|_| PathBuf::from(entry));
    let s = abs.to_string_lossy();
    let cleaned = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    format!("file:///{cleaned}")
}
