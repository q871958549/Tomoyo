//! Win32 window bootstrap, EDIT control creation, and two-phase async query worker setup.

use super::input::register_all_hotkeys;
use super::input_edit::edit_subclass_proc;
use super::wndproc::main_wndproc;
use super::{
    AppShell, InteractionState, QueryJob, QueryOutcome, WindowGeometry, EDIT_CTRL_ID,
    EDIT_SUBCLASS_ID, EM_SETCUEBANNER, WM_APP_RESULTS_READY,
};
use crate::actions::read_clipboard_text;
use crate::preview_loader::AsyncPreviewLoader;
use crate::renderer::{
    FluentRenderer, ThemePalette, FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP, MIN_WINDOW_WIDTH_DIP,
};
use crate::tray::TrayIcon;
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tomoyo_core::{Lang, QueryRouter, TomoyoConfig, UpdateManager};
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::SearchFilter;
use tomoyo_providers::ClipboardProvider;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreateSolidBrush, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, FF_DONTCARE, FW_NORMAL, HFONT, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::DataExchange::AddClipboardFormatListener;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetWindowTextLengthW, GetWindowTextW, LoadCursorW, PostMessageW,
    RegisterClassW, RegisterWindowMessageW, SendMessageW, SetWindowLongPtrW, CS_DBLCLKS,
    CW_USEDEFAULT, ES_AUTOHSCROLL, ES_LEFT, GWLP_USERDATA, HMENU, IDC_ARROW, WINDOW_STYLE,
    WM_SETFONT, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    WS_VISIBLE,
};

impl AppShell {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        config: Arc<RwLock<TomoyoConfig>>,
        router: Arc<QueryRouter>,
        plugin_host: Arc<PluginHost>,
        clipboard: Arc<ClipboardProvider>,
        updater: Arc<UpdateManager>,
        font_regular: &'static str,
        font_medium: &'static str,
        install_tray_and_hotkey: bool,
    ) -> windows::core::Result<Box<Self>> {
        let renderer = FluentRenderer::new(font_regular, font_medium)?;
        let preview_loader = AsyncPreviewLoader::new();
        let (query_tx, query_rx) = mpsc::channel::<QueryJob>();
        let (result_tx, result_rx) = mpsc::channel::<QueryOutcome>();

        let cfg_snap = config.read().map(|c| c.clone()).unwrap_or_default();
        let initial_lang = cfg_snap.lang();
        let theme = ThemePalette::from_name(&cfg_snap.theme);
        let geom = WindowGeometry {
            list_width_dip: cfg_snap.window.width_dip.clamp(MIN_WINDOW_WIDTH_DIP, 1400),
            max_visible_rows: cfg_snap.window.max_visible_rows.clamp(4, 18),
            preview_width_dip: cfg_snap.window.preview_width_dip.clamp(220, 620),
            custom_pos: cfg_snap.window.custom_x.zip(cfg_snap.window.custom_y),
        };

        let class_name = w!("TomoyoLauncherWindowClass");
        let hinstance = unsafe { GetModuleHandleW(None)? };
        unsafe {
            let wc = WNDCLASSW {
                style: CS_DBLCLKS,
                lpfnWndProc: Some(main_wndproc),
                hInstance: hinstance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);
        }

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name,
                w!("Tomoyo"),
                WS_POPUP | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                geom.list_width_dip,
                HEADER_HEIGHT_DIP + FOOTER_HEIGHT_DIP,
                None,
                None,
                Some(hinstance.into()),
                None,
            )?
        };
        apply_dwm_attributes(hwnd, theme.is_dark);
        renderer.set_notify_hwnd(hwnd.0 as isize);
        preview_loader.set_notify_hwnd(hwnd.0 as isize);

        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let dpi_scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
        let edit_font = create_edit_font(dpi_scale, font_regular);
        let edit_bg_brush = unsafe { CreateSolidBrush(theme.bg.to_colorref()) };

        let edit_hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE((ES_LEFT | ES_AUTOHSCROLL) as u32),
                (62.0 * dpi_scale) as i32,
                (18.0 * dpi_scale) as i32,
                ((geom.list_width_dip - 190) as f32 * dpi_scale) as i32,
                (28.0 * dpi_scale) as i32,
                Some(hwnd),
                Some(HMENU(EDIT_CTRL_ID as *mut c_void)),
                Some(hinstance.into()),
                None,
            )?
        };
        unsafe {
            SendMessageW(
                edit_hwnd,
                WM_SETFONT,
                Some(WPARAM(edit_font.0 as usize)),
                Some(LPARAM(1)),
            );
            apply_cue_banner(edit_hwnd, initial_lang);
            let _ = SetWindowSubclass(
                edit_hwnd,
                Some(edit_subclass_proc),
                EDIT_SUBCLASS_ID,
                hwnd.0 as usize,
            );
        }

        let mut hotkey_label = cfg_snap.hotkey.clone();
        let mut hotkey_conflict = None;
        let mut plugin_hotkeys = Vec::new();
        let mut flow_hotkeys = Vec::new();
        let tray = if install_tray_and_hotkey {
            if cfg_snap.is_plugin_enabled(tomoyo_core::PLUGIN_CLIPBOARD) {
                unsafe { let _ = AddClipboardFormatListener(hwnd); }
            }
            let (active, conflict, plug_hks, flow_hks) = register_all_hotkeys(hwnd, &cfg_snap, &plugin_host);
            hotkey_label = active;
            hotkey_conflict = conflict;
            plugin_hotkeys = plug_hks;
            flow_hotkeys = flow_hks;
            Some(TrayIcon::install(hwnd, &hotkey_label))
        } else {
            None
        };

        let taskbar_created_msg = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };

        spawn_query_worker(Arc::clone(&router), hwnd.0 as usize, query_rx, result_tx);

        let mut app = Box::new(Self {
            hwnd,
            edit_hwnd,
            edit_font,
            edit_bg_brush,
            tray,
            renderer,
            preview_loader,
            font_regular,
            font_medium,
            config,
            lang: initial_lang,
            theme,
            theme_name: cfg_snap.theme,
            router,
            plugin_host,
            clipboard,
            updater,
            query_tx,
            result_rx,
            current_query: String::new(),
            attached: None,
            active_filter: SearchFilter::All,
            show_filter_bar: cfg_snap.show_filter_bar,
            last_result_gen: 0,
            results: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            action_menu_open: false,
            selected_action: 0,
            preview_enabled: cfg_snap.auto_preview,
            cached_preview_id: None,
            cached_preview: None,
            geom,
            inter: InteractionState::default(),
            toast: None,
            everything_status: detect_everything_label(),
            hotkey_label,
            hotkey_conflict,
            plugin_hotkeys,
            flow_hotkeys,
            pending_flow_result: Arc::new(Mutex::new(None)),
            pending_launch_result: Arc::new(Mutex::new(None)),
            pending_update_install: Arc::new(Mutex::new(Default::default())),
            dpi_scale,
            taskbar_created_msg,
            suppress_clipboard_capture: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutting_down: false,
        });

        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *app as *mut AppShell as isize);
        }
        if app.config.read().map(|c| c.is_plugin_enabled(tomoyo_core::PLUGIN_CLIPBOARD)).unwrap_or(true)
            && let Some(txt) = read_clipboard_text() {
                app.clipboard.push_text(txt);
            }
        Ok(app)
    }
}

fn spawn_query_worker(
    worker_router: Arc<QueryRouter>,
    hwnd_raw: usize,
    query_rx: Receiver<QueryJob>,
    result_tx: Sender<QueryOutcome>,
) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-query-worker".into())
        .spawn(move || {
            let mut next_pending: Option<QueryJob> = None;
            loop {
                let mut job = match next_pending.take() {
                    Some(j) => j,
                    None => match query_rx.recv() {
                        Ok(j) => j,
                        Err(_) => break,
                    },
                };
                while let Ok(newer) = query_rx.try_recv() {
                    job = newer;
                }
                if worker_router.is_stale(job.generation) {
                    continue;
                }

                // Phase 1: Instant in-memory search (Apps/Calc/Web/System) -> post immediately
                let batch = worker_router.search_fast(
                    &job.query,
                    job.generation,
                    30,
                    job.clipboard,
                    job.attached,
                    job.filter,
                );
                if worker_router.is_stale(job.generation) {
                    continue;
                }

                if !batch.run_slow_ipc || !batch.items.is_empty() {
                    post_query_outcome(
                        hwnd_raw,
                        &result_tx,
                        QueryOutcome {
                            generation: job.generation,
                            items: batch.items.clone(),
                            is_partial: batch.run_slow_ipc,
                        },
                    );
                }

                if !batch.run_slow_ipc {
                    continue;
                }

                // Debounce slow IPC: 85ms for unprefixed global typing bursts, 25ms for dedicated `f ` / file tabs.
                let debounce_ms = if batch.effective_ctx.keyword.is_some()
                    || batch.effective_ctx.filter.is_file_only()
                {
                    25
                } else {
                    85
                };
                match query_rx.recv_timeout(Duration::from_millis(debounce_ms)) {
                    Ok(newer) => {
                        next_pending = Some(newer);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }

                if worker_router.is_stale(job.generation) {
                    continue;
                }
                let merged = worker_router.search_slow_and_merge(batch, 30);
                if worker_router.is_stale(job.generation) {
                    continue;
                }
                post_query_outcome(
                    hwnd_raw,
                    &result_tx,
                    QueryOutcome {
                        generation: job.generation,
                        items: merged,
                        is_partial: false,
                    },
                );
            }
        });
}

fn post_query_outcome(hwnd_raw: usize, tx: &Sender<QueryOutcome>, out: QueryOutcome) {
    if tx.send(out).is_ok() {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd_raw as *mut c_void)),
                WM_APP_RESULTS_READY,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub(super) fn detect_everything_label() -> Option<&'static str> {
    match tomoyo_everything::detect_running() {
        Some(tomoyo_everything::EverythingVersion::V15Alpha) => Some("Everything 1.5a"),
        Some(tomoyo_everything::EverythingVersion::V14) => Some("Everything 1.4"),
        None => None,
    }
}

pub(super) unsafe fn apply_cue_banner(edit_hwnd: HWND, lang: Lang) {
    let text = lang.pick(
        "搜应用(支持拼音)、Everything (f)、@pdf/@excel、代码片段 (snip)、剪贴板 (cb) 或 :settings...",
        "Search apps, Everything (f), @pdf/@excel, snippets (snip), cb, kill, or :settings...",
    );
    let cue: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        SendMessageW(
            edit_hwnd,
            EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(cue.as_ptr() as isize)),
        );
    }
}

pub(super) fn apply_dwm_attributes(hwnd: HWND, is_dark: bool) {
    unsafe {
        let dark: i32 = if is_dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const c_void,
            std::mem::size_of_val(&corner) as u32,
        );
    }
}

pub(super) fn create_edit_font(dpi_scale: f32, face: &str) -> HFONT {
    let height = -(20.5 * dpi_scale).round() as i32;
    let face_w: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(face_w.as_ptr()),
        )
    }
}

pub(super) fn read_edit_text(edit: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(edit);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(edit, &mut buf);
        String::from_utf16_lossy(&buf[..copied as usize])
    }
}
