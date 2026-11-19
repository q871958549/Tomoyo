//! Main Win32 launcher window: native Chinese IME Edit control (Alibaba PuHuiTi 3.0),
//! config-driven global hotkey, free window dragging & edge/corner resizing, draggable
//! preview divider, hover highlight, right-click menus on results & Modern White logo,
//! live multi-theme & zh/en localization, clipboard auto-paste, OLE file drag-out, and hibernation.

use crate::actions::{
    clipboard_has_rich_attachment, execute_action, kill_process_by_pid,
    paste_into_previous_window_async, read_clipboard_files, read_clipboard_image_png,
    read_clipboard_text, set_clipboard_text, shell_open, spawn_settings_window,
};
use crate::drag::start_file_drag;
use crate::renderer::{
    FluentRenderer, RenderState, ThemePalette, DEFAULT_PREVIEW_WIDTH_DIP, DEFAULT_WINDOW_WIDTH_DIP,
    FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP, MAX_WINDOW_WIDTH_DIP, MIN_WINDOW_WIDTH_DIP,
    ROW_HEIGHT_DIP,
};
use crate::tray::{
    TrayIcon, MENU_ID_CONFIG, MENU_ID_GC, MENU_ID_QUIT, MENU_ID_RELOAD, MENU_ID_SETTINGS,
    MENU_ID_SHOW, MENU_ID_STATS, MENU_ID_UPDATE, WM_TRAY_CALLBACK,
};
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, RwLock};
use tomoyo_core::{
    is_autostart_enabled, parse_hotkey, read_file_preview, read_image_thumbnail,
    set_autostart_enabled, ExternalPluginBridge, Lang, QueryRouter, ResolvedPreview, TomoyoConfig,
    UpdateManager, UpdateStatus,
};
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{
    ActionKind, AttachedContext, ClipboardSnapshot, IconSpec, InternalCommandKind, PreviewSpec,
    ResultItem,
};
use tomoyo_providers::ClipboardProvider;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, GetMonitorInfoW,
    InvalidateRect, MonitorFromPoint, ScreenToClient, SetBkColor, SetTextColor, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FW_NORMAL, HBRUSH, HDC,
    HFONT, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT,
};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, RegisterHotKey, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent,
    UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, TME_LEAVE,
    TRACKMOUSEEVENT, VK_BACK, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageW,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    KillTimer, LoadCursorW, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW,
    SendMessageW, SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos,
    SetWindowTextW, ShowWindow, TrackPopupMenu, TranslateMessage, CW_USEDEFAULT, EN_CHANGE,
    ES_AUTOHSCROLL, ES_LEFT, GWLP_USERDATA, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION,
    HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, HWND_TOPMOST, IDC_ARROW, IDC_SIZEWE,
    MF_CHECKED, MF_SEPARATOR, MF_STRING, MINMAXINFO, MSG, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOW, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TPM_TOPALIGN, WM_ACTIVATE, WM_APP,
    WM_CLIPBOARDUPDATE, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY, WM_ENTERSIZEMOVE, WM_ERASEBKGND,
    WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST, WM_NCRBUTTONUP, WM_PAINT,
    WM_RBUTTONUP, WM_SETCURSOR, WM_SETFONT, WM_SIZE, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

const HOTKEY_ID: i32 = 1;
const EDIT_CTRL_ID: usize = 101;
const EDIT_SUBCLASS_ID: usize = 1;
const WM_APP_RESULTS_READY: u32 = WM_APP + 1;
const WM_APP_EDIT_KEY: u32 = WM_APP + 2;
const WM_APP_PASTE_INTERCEPT: u32 = WM_APP + 3;
const WM_APP_UPDATE_READY: u32 = WM_APP + 4;
const WM_APP_CONFIG_SAVED: u32 = WM_APP + 20;
const TIMER_HIBERNATE: usize = 1001;
const TIMER_CLEAR_TOAST: usize = 1002;
const EM_SETCUEBANNER: u32 = 0x1501;
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;

struct QueryJob {
    generation: u64,
    query: String,
    clipboard: Option<ClipboardSnapshot>,
    attached: Option<AttachedContext>,
}

struct QueryOutcome {
    generation: u64,
    items: Vec<ResultItem>,
}

pub struct AppShell {
    hwnd: HWND,
    edit_hwnd: HWND,
    edit_font: HFONT,
    edit_bg_brush: HBRUSH,
    tray: Option<TrayIcon>,
    renderer: FluentRenderer,
    config: Arc<RwLock<TomoyoConfig>>,
    lang: Lang,
    theme: ThemePalette,
    theme_name: String,
    router: Arc<QueryRouter>,
    plugin_host: Arc<PluginHost>,
    clipboard: Arc<ClipboardProvider>,
    updater: Arc<UpdateManager>,
    query_tx: Sender<QueryJob>,
    result_rx: Receiver<QueryOutcome>,
    current_query: String,
    attached: Option<AttachedContext>,
    suppress_next_clip_img: bool,
    results: Vec<ResultItem>,
    selected: usize,
    hovered: Option<usize>,
    mouse_tracked: bool,
    scroll_offset: usize,
    action_menu_open: bool,
    selected_action: usize,
    preview_enabled: bool,
    cached_preview_id: Option<String>,
    cached_preview: Option<ResolvedPreview>,
    list_width_dip: i32,
    max_visible_rows: usize,
    preview_width_dip: i32,
    custom_pos: Option<(i32, i32)>,
    in_size_move: bool,
    dragging_divider: bool,
    drag_origin: Option<(i32, i32, String)>,
    prev_foreground_hwnd: isize,
    toast: Option<String>,
    everything_status: Option<&'static str>,
    hotkey_label: String,
    dpi_scale: f32,
    suppress_next_deactivate: bool,
}

impl AppShell {
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
        let (query_tx, query_rx) = mpsc::channel::<QueryJob>();
        let (result_tx, result_rx) = mpsc::channel::<QueryOutcome>();

        let (
            initial_lang,
            initial_theme_name,
            initial_hotkey,
            initial_auto_preview,
            list_width_dip,
            max_visible_rows,
            preview_width_dip,
            custom_pos,
        ) = config
            .read()
            .map(|c| {
                let pos = c.window.custom_x.zip(c.window.custom_y);
                (
                    c.lang(),
                    c.theme.clone(),
                    c.hotkey.clone(),
                    c.auto_preview,
                    c.window.width_dip.clamp(MIN_WINDOW_WIDTH_DIP, 1400),
                    c.window.max_visible_rows.clamp(4, 18),
                    c.window.preview_width_dip.clamp(220, 620),
                    pos,
                )
            })
            .unwrap_or_else(|_| {
                (
                    Lang::Zh,
                    "dark".into(),
                    "Alt+Space".into(),
                    true,
                    DEFAULT_WINDOW_WIDTH_DIP,
                    8,
                    DEFAULT_PREVIEW_WIDTH_DIP,
                    None,
                )
            });
        let theme = ThemePalette::from_name(&initial_theme_name);

        let class_name = w!("TomoyoLauncherWindowClass");
        let hinstance = unsafe { GetModuleHandleW(None)? };
        unsafe {
            let wc = WNDCLASSW {
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
                list_width_dip,
                HEADER_HEIGHT_DIP + FOOTER_HEIGHT_DIP,
                None,
                None,
                Some(hinstance.into()),
                None,
            )?
        };

        apply_dwm_attributes(hwnd, theme.is_dark);

        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let dpi_scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };

        let edit_font = create_edit_font(dpi_scale, font_regular);
        let edit_bg_brush = unsafe { CreateSolidBrush(theme.bg.to_colorref()) };

        let edit_hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD
                    | WS_VISIBLE
                    | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                        (ES_LEFT | ES_AUTOHSCROLL) as u32,
                    ),
                (62.0 * dpi_scale) as i32,
                (18.0 * dpi_scale) as i32,
                ((list_width_dip - 190) as f32 * dpi_scale) as i32,
                (28.0 * dpi_scale) as i32,
                Some(hwnd),
                Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                    EDIT_CTRL_ID as *mut c_void,
                )),
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

        let mut hotkey_label = initial_hotkey.clone();
        let tray = if install_tray_and_hotkey {
            unsafe {
                let _ = AddClipboardFormatListener(hwnd);
            }
            hotkey_label = register_configured_hotkey(hwnd, &initial_hotkey);
            Some(TrayIcon::install(hwnd, &hotkey_label))
        } else {
            None
        };

        let worker_router = Arc::clone(&router);
        let hwnd_raw = hwnd.0 as usize;
        let _ = std::thread::Builder::new()
            .name("tomoyo-query-worker".into())
            .spawn(move || {
                while let Ok(mut job) = query_rx.recv() {
                    while let Ok(newer) = query_rx.try_recv() {
                        job = newer;
                    }
                    if worker_router.is_stale(job.generation) {
                        continue;
                    }
                    let items = worker_router.search(
                        &job.query,
                        job.generation,
                        30,
                        job.clipboard,
                        job.attached,
                    );
                    if worker_router.is_stale(job.generation) {
                        continue;
                    }
                    if result_tx
                        .send(QueryOutcome {
                            generation: job.generation,
                            items,
                        })
                        .is_ok()
                    {
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
            });

        let mut app = Box::new(Self {
            hwnd,
            edit_hwnd,
            edit_font,
            edit_bg_brush,
            tray,
            renderer,
            config,
            lang: initial_lang,
            theme,
            theme_name: initial_theme_name,
            router,
            plugin_host,
            clipboard,
            updater,
            query_tx,
            result_rx,
            current_query: String::new(),
            attached: None,
            suppress_next_clip_img: false,
            results: Vec::new(),
            selected: 0,
            hovered: None,
            mouse_tracked: false,
            scroll_offset: 0,
            action_menu_open: false,
            selected_action: 0,
            preview_enabled: initial_auto_preview,
            cached_preview_id: None,
            cached_preview: None,
            list_width_dip,
            max_visible_rows,
            preview_width_dip,
            custom_pos,
            in_size_move: false,
            dragging_divider: false,
            drag_origin: None,
            prev_foreground_hwnd: 0,
            toast: None,
            everything_status: detect_everything_label(),
            hotkey_label,
            dpi_scale,
            suppress_next_deactivate: false,
        });

        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *app as *mut AppShell as isize);
        }

        if let Some(txt) = read_clipboard_text() {
            app.clipboard.push_text(txt);
        }

        Ok(app)
    }

    pub fn is_renderer_hibernated(&self) -> bool {
        self.renderer.is_hibernated()
    }

    pub fn hotkey_label(&self) -> &str {
        &self.hotkey_label
    }

    pub fn apply_theme(&mut self, theme_name: &str, persist: bool) {
        let new_theme = ThemePalette::from_name(theme_name);
        self.theme = new_theme;
        self.theme_name = theme_name.to_string();
        apply_dwm_attributes(self.hwnd, new_theme.is_dark);

        unsafe {
            if !self.edit_bg_brush.is_invalid() {
                let _ = DeleteObject(self.edit_bg_brush.into());
            }
            self.edit_bg_brush = CreateSolidBrush(new_theme.bg.to_colorref());
            let _ = InvalidateRect(Some(self.edit_hwnd), None, true);
        }

        if persist {
            if let Ok(mut cfg) = self.config.write() {
                cfg.theme = theme_name.to_string();
                let _ = cfg.save();
            }
        }
        self.invalidate();
    }

    pub fn apply_language(&mut self, lang_code: &str, persist: bool) {
        let new_lang = Lang::from_code(lang_code);
        self.lang = new_lang;
        unsafe {
            apply_cue_banner(self.edit_hwnd, new_lang);
        }
        if persist {
            if let Ok(mut cfg) = self.config.write() {
                cfg.language = new_lang.code().to_string();
                let _ = cfg.save();
            }
        }
        self.dispatch_query();
        self.invalidate();
    }

    pub fn open_settings_center(&mut self) {
        match spawn_settings_window(self.hwnd.0 as isize, &self.theme_name) {
            Ok(()) => self.hide_launcher(),
            Err(e) => self.show_toast(&format!("设置中心启动失败: {e}")),
        }
    }

    pub fn reset_window_geometry(&mut self) {
        self.list_width_dip = DEFAULT_WINDOW_WIDTH_DIP;
        self.max_visible_rows = 8;
        self.preview_width_dip = DEFAULT_PREVIEW_WIDTH_DIP;
        self.custom_pos = None;
        if let Ok(mut cfg) = self.config.write() {
            cfg.window = Default::default();
            let _ = cfg.save();
        }
        self.position_on_active_monitor();
        self.show_toast(self.lang.pick(
            "已重置窗口位置与默认尺寸",
            "Window position & size reset to defaults",
        ));
    }

    pub fn show_launcher(&mut self) {
        // Remember previously active foreground window for Clipboard Auto-Paste (`Enter`)!
        unsafe {
            let fg = GetForegroundWindow();
            if !fg.is_invalid() && fg != self.hwnd {
                self.prev_foreground_hwnd = fg.0 as isize;
            }
            let _ = KillTimer(Some(self.hwnd), TIMER_HIBERNATE);
        }
        self.everything_status = detect_everything_label();
        self.action_menu_open = false;
        self.selected_action = 0;
        self.hovered = None;

        self.position_on_active_monitor();
        self.dispatch_query();

        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.edit_hwnd));
            SendMessageW(
                self.edit_hwnd,
                EM_SETSEL,
                Some(WPARAM(0)),
                Some(LPARAM(-1)),
            );
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn hide_launcher(&mut self) {
        self.action_menu_open = false;
        self.selected_action = 0;
        self.hovered = None;
        self.toast = None;
        self.drag_origin = None;
        self.dragging_divider = false;
        let delay_ms = self
            .config
            .read()
            .map(|c| c.hibernate_delay_secs.clamp(5, 300) * 1000)
            .unwrap_or(15_000);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = SetWindowTextW(self.edit_hwnd, w!(""));
            let _ = KillTimer(Some(self.hwnd), TIMER_HIBERNATE);
            let _ = SetTimer(Some(self.hwnd), TIMER_HIBERNATE, delay_ms, None);
        }
        self.current_query.clear();
        self.attached = None;
        self.results.clear();
        self.cached_preview_id = None;
        self.cached_preview = None;
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn toggle_launcher(&mut self) {
        let visible = unsafe { IsWindowVisible(self.hwnd).as_bool() };
        if visible {
            self.hide_launcher();
        } else {
            self.show_launcher();
        }
    }

    pub fn run_hibernate(&mut self) -> u64 {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_HIBERNATE);
        }
        self.renderer.hibernate_release();
        self.results.clear();
        self.results.shrink_to_fit();
        self.cached_preview_id = None;
        self.cached_preview = None;
        self.router.hibernate_trim()
    }

    pub fn set_query_text(&mut self, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = SetWindowTextW(self.edit_hwnd, PCWSTR(wide.as_ptr()));
            let len = text.encode_utf16().count() as isize;
            SendMessageW(
                self.edit_hwnd,
                EM_SETSEL,
                Some(WPARAM(len as usize)),
                Some(LPARAM(len)),
            );
        }
        self.current_query = text.to_string();
        self.dispatch_query();
    }

    pub fn run_message_loop(&mut self) {
        let mut msg = MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    pub fn self_test_paint_and_hibernate(&mut self, query: &str) -> usize {
        self.current_query = query.to_string();
        let generation = self.router.next_generation();
        self.results = self.router.search(
            query,
            generation,
            20,
            self.clipboard.latest_snapshot(),
            self.attached.clone(),
        );
        self.selected = 0;
        self.hovered = if self.results.len() > 1 { Some(1) } else { None };
        self.scroll_offset = 0;
        self.refresh_selected_preview();
        let count = self.results.len();

        let hdc = unsafe { windows::Win32::Graphics::Gdi::GetDC(Some(self.hwnd)) };
        let w = (self.desired_width_dip() as f32 * self.dpi_scale) as i32;
        let h = (self.desired_height_dip() as f32 * self.dpi_scale) as i32;
        self.render_into_hdc(hdc, w, h);
        unsafe {
            windows::Win32::Graphics::Gdi::ReleaseDC(Some(self.hwnd), hdc);
        }
        count
    }

    pub fn self_test_attached_paint(&mut self, ctx: AttachedContext) -> usize {
        self.attached = Some(ctx);
        let hits = self.self_test_paint_and_hibernate("");
        self.attached = None;
        hits
    }

    fn reload_config_and_all(&mut self) {
        let fresh = TomoyoConfig::load_or_create();
        let new_lang = fresh.lang();
        let new_theme = fresh.theme.clone();
        let new_hotkey = fresh.hotkey.clone();
        self.preview_enabled = fresh.auto_preview;
        self.list_width_dip = fresh.window.width_dip.clamp(MIN_WINDOW_WIDTH_DIP, 1400);
        self.max_visible_rows = fresh.window.max_visible_rows.clamp(4, 18);
        self.preview_width_dip = fresh.window.preview_width_dip.clamp(220, 620);
        self.custom_pos = fresh.window.custom_x.zip(fresh.window.custom_y);

        if let Ok(mut cfg) = self.config.write() {
            *cfg = fresh;
        }
        self.lang = new_lang;
        unsafe {
            apply_cue_banner(self.edit_hwnd, new_lang);
        }
        self.apply_theme(&new_theme, false);
        if self.tray.is_some() {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ID);
            }
            self.hotkey_label = register_configured_hotkey(self.hwnd, &new_hotkey);
        }
        self.router.reload_all();
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.position_on_active_monitor();
            self.dispatch_query();
            self.invalidate();
        }
    }

    fn persist_window_geometry(&mut self) {
        let mut rect = RECT::default();
        unsafe {
            if GetWindowRect(self.hwnd, &mut rect).is_err() {
                return;
            }
        }
        self.custom_pos = Some((rect.left, rect.top));
        if let Ok(mut cfg) = self.config.write() {
            cfg.window.width_dip = self.list_width_dip;
            cfg.window.max_visible_rows = self.max_visible_rows;
            cfg.window.preview_width_dip = self.preview_width_dip;
            cfg.window.custom_x = Some(rect.left);
            cfg.window.custom_y = Some(rect.top);
            let _ = cfg.save();
        }
    }

    fn refresh_selected_preview(&mut self) {
        if !self.preview_enabled {
            self.cached_preview_id = None;
            self.cached_preview = None;
            return;
        }
        let Some(item) = self.results.get(self.selected) else {
            self.cached_preview_id = None;
            self.cached_preview = None;
            return;
        };
        if self.cached_preview_id.as_deref() == Some(&item.id) {
            return;
        }
        self.cached_preview_id = Some(item.id.clone());
        self.cached_preview = item.preview.as_ref().map(|spec| match spec {
            PreviewSpec::Text { title, body } => ResolvedPreview {
                title: title.clone(),
                meta_line: match self.lang {
                    Lang::Zh => format!("{} 字符 · {} 行", body.chars().count(), body.lines().count().max(1)),
                    Lang::En => format!("{} chars · {} lines", body.chars().count(), body.lines().count().max(1)),
                },
                body: body.clone(),
                is_monospace: body.contains('{') || body.contains('\n'),
                image_rgba: None,
            },
            PreviewSpec::KeyValue { title, entries } => {
                let body = entries
                    .iter()
                    .map(|(k, v)| format!("{k:<14} {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                ResolvedPreview {
                    title: title.clone(),
                    meta_line: match self.lang {
                        Lang::Zh => format!("{} 项属性", entries.len()),
                        Lang::En => format!("{} properties", entries.len()),
                    },
                    body,
                    is_monospace: true,
                    image_rgba: None,
                }
            }
            PreviewSpec::FilePreview { path } => read_file_preview(path),
            PreviewSpec::ImagePreview { path, .. } => read_image_thumbnail(path, 280, 200),
        });
    }

    fn position_on_active_monitor(&mut self) {
        unsafe {
            let dpi = GetDpiForWindow(self.hwnd);
            if dpi > 0 {
                self.dpi_scale = dpi as f32 / 96.0;
            }
            let width = (self.desired_width_dip() as f32 * self.dpi_scale) as i32;
            let height = (self.desired_height_dip() as f32 * self.dpi_scale) as i32;

            if let Some((cx, cy)) = self.custom_pos {
                let test_pt = POINT {
                    x: cx + width / 2,
                    y: cy + 30,
                };
                if !MonitorFromPoint(test_pt, MONITOR_DEFAULTTONULL).is_invalid() {
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(HWND_TOPMOST),
                        cx,
                        cy,
                        width,
                        height,
                        SWP_NOACTIVATE,
                    );
                    self.layout_edit_control(width);
                    return;
                }
            }

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(hmon, &mut mi);

            let work = mi.rcWork;
            let x = work.left + ((work.right - work.left) - width) / 2;
            let y = work.top + ((work.bottom - work.top) / 5).max(40);
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
            self.layout_edit_control(width);
        }
    }

    fn resize_to_fit_results(&mut self) {
        if self.in_size_move || self.dragging_divider {
            return;
        }
        let mut rect = RECT::default();
        unsafe {
            if GetWindowRect(self.hwnd, &mut rect).is_err() {
                return;
            }
        }
        let width = (self.desired_width_dip() as f32 * self.dpi_scale) as i32;
        let height = (self.desired_height_dip() as f32 * self.dpi_scale) as i32;
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                rect.left,
                rect.top,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        self.layout_edit_control(width);
    }

    fn layout_edit_control(&self, window_width_px: i32) {
        let left = (62.0 * self.dpi_scale) as i32;
        let top = (18.0 * self.dpi_scale) as i32;
        let mut right_pad_dip = 24.0f32;
        if self.active_mode_badge().is_some() {
            right_pad_dip += 132.0;
        }
        if self.attached.is_some() {
            right_pad_dip += 196.0;
        }
        let right_pad = (right_pad_dip * self.dpi_scale) as i32;
        let w = (window_width_px - left - right_pad).max(120);
        let h = (28.0 * self.dpi_scale) as i32;
        unsafe {
            let _ = MoveWindow(self.edit_hwnd, left, top, w, h, true);
        }
    }

    fn is_preview_visible(&self) -> bool {
        self.cached_preview.is_some() && !self.results.is_empty()
    }

    fn desired_width_dip(&self) -> i32 {
        if self.is_preview_visible() {
            (self.list_width_dip + self.preview_width_dip).min(MAX_WINDOW_WIDTH_DIP)
        } else {
            self.list_width_dip
        }
    }

    fn desired_height_dip(&self) -> i32 {
        let rows = self.results.len().min(self.max_visible_rows) as i32;
        if rows == 0 {
            HEADER_HEIGHT_DIP + FOOTER_HEIGHT_DIP
        } else {
            let min_preview_rows = if self.cached_preview.is_some() { 5 } else { 0 };
            let eff_rows = rows.max(min_preview_rows);
            HEADER_HEIGHT_DIP + 8 + eff_rows * ROW_HEIGHT_DIP + FOOTER_HEIGHT_DIP
        }
    }

    fn active_mode_badge(&self) -> Option<&'static str> {
        let t = self.current_query.trim();
        let l = self.lang;
        if t.starts_with(':') {
            return Some(l.pick("命令", "COMMAND"));
        }
        let low = t.to_lowercase();
        if low.starts_with("f ") || low.starts_with("ev ") {
            return Some(l.pick("文件搜索", "EVERYTHING"));
        }
        if low == "cb" || low.starts_with("cb ") {
            return Some(l.pick("剪贴板", "CLIPBOARD"));
        }
        if low == "w" || low.starts_with("w ") {
            return Some(l.pick("窗口切换", "WINDOWS"));
        }
        if low == "kill" || low.starts_with("kill ") || low == "ps" || low.starts_with("ps ") {
            return Some(l.pick("进程管理", "PROCESS"));
        }
        if low.starts_with("= ") || low.starts_with("b64 ") || low.starts_with("b64d ") {
            return Some(l.pick("计算", "CALC"));
        }
        if let Some(first) = low.split_whitespace().next() {
            if self.plugin_host.matches_keyword(first) {
                return Some(l.pick("插件", "PLUGIN"));
            }
            if self
                .config
                .read()
                .map(|c| {
                    c.web_searches
                        .iter()
                        .any(|w| w.keyword.eq_ignore_ascii_case(first))
                })
                .unwrap_or(false)
                && low.contains(' ')
            {
                return Some(l.pick("网页搜索", "WEB"));
            }
        }
        None
    }

    fn attached_capsule_label(&self) -> Option<String> {
        let l = self.lang;
        match self.attached.as_ref()? {
            AttachedContext::Image { width, height, .. } => Some(match l {
                Lang::Zh => format!("📎 图片 {width}×{height}"),
                Lang::En => format!("📎 Img {width}×{height}"),
            }),
            AttachedContext::Files { paths } => Some(match l {
                Lang::Zh => format!("📎 {} 个文件", paths.len()),
                Lang::En => format!("📎 {} file(s)", paths.len()),
            }),
        }
    }

    fn dispatch_query(&mut self) {
        let generation = self.router.next_generation();
        let _ = self.query_tx.send(QueryJob {
            generation,
            query: self.current_query.clone(),
            clipboard: self.clipboard.latest_snapshot(),
            attached: self.attached.clone(),
        });
    }

    fn trigger_update_check_async(&mut self) {
        let repo = self
            .config
            .read()
            .map(|c| c.update.repo.clone())
            .unwrap_or_else(|_| "q871958549/Tomoyo".into());
        self.show_toast(self.lang.pick(
            "正在连接 GitHub Releases 检查最新版本...",
            "Checking GitHub Releases for updates...",
        ));
        let updater = Arc::clone(&self.updater);
        let cfg = Arc::clone(&self.config);
        let hwnd_raw = self.hwnd.0 as usize;
        let _ = std::thread::Builder::new()
            .name("tomoyo-manual-update".into())
            .spawn(move || {
                let res = updater.check_now_sync(&repo);
                if matches!(res, UpdateStatus::UpToDate { .. } | UpdateStatus::Available { .. }) {
                    if let Ok(mut w) = cfg.write() {
                        w.update.last_checked_unix = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let _ = w.save();
                    }
                }
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut c_void)),
                        WM_APP_UPDATE_READY,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            });
    }

    fn on_update_ready(&mut self) {
        let msg = match self.updater.snapshot() {
            UpdateStatus::Available { release, .. } => match self.lang {
                Lang::Zh => format!("发现新版本 v{}！输入 :update 查看更新日志并下载", release.version),
                Lang::En => format!("New version v{} available! Type :update to download", release.version),
            },
            UpdateStatus::UpToDate { current, note, .. } => {
                if note.is_empty() {
                    match self.lang {
                        Lang::Zh => format!("当前已是最新版本 v{current}"),
                        Lang::En => format!("Already on latest version v{current}"),
                    }
                } else {
                    note
                }
            }
            UpdateStatus::Error { message, .. } => match self.lang {
                Lang::Zh => format!("检查更新失败：{message}"),
                Lang::En => format!("Update check failed: {message}"),
            },
            _ => return,
        };
        self.show_toast(&msg);
        if self.current_query.trim().starts_with(':') {
            self.dispatch_query();
        }
    }

    fn on_paste_intercept(&mut self) {
        if let Some(files) = read_clipboard_files() {
            let n = files.len();
            self.attached = Some(AttachedContext::Files { paths: files });
            self.show_toast(&match self.lang {
                Lang::Zh => format!("已附加 {n} 个文件胶囊（Super Match）· 按 Backspace 清除"),
                Lang::En => format!("Attached {n} file(s) capsule · Press Backspace to clear"),
            });
            self.dispatch_query();
            self.invalidate();
            return;
        }
        if let Some((path, w, h, digest)) = read_clipboard_image_png() {
            self.clipboard.push_image(path.clone(), w, h, digest);
            self.attached = Some(AttachedContext::Image {
                path,
                width: w,
                height: h,
            });
            self.show_toast(&match self.lang {
                Lang::Zh => format!("已附加 {w}×{h} 图片胶囊（Super Match）· 按 Backspace 清除"),
                Lang::En => format!("Attached {w}×{h} image capsule · Press Backspace to clear"),
            });
            self.dispatch_query();
            self.invalidate();
        }
    }

    fn on_results_ready(&mut self) {
        let mut latest = None;
        while let Ok(out) = self.result_rx.try_recv() {
            latest = Some(out);
        }
        let Some(out) = latest else { return };
        if self.router.is_stale(out.generation) {
            return;
        }
        self.results = out.items;
        self.selected = 0;
        self.hovered = None;
        self.scroll_offset = 0;
        self.action_menu_open = false;
        self.selected_action = 0;
        self.cached_preview_id = None;
        self.refresh_selected_preview();

        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.resize_to_fit_results();
            self.invalidate();
        }
    }

    fn on_edit_changed(&mut self) {
        self.current_query = read_edit_text(self.edit_hwnd);
        self.dispatch_query();
    }

    fn is_over_preview_divider(&self, x_px: i32, y_px: i32) -> bool {
        if !self.is_preview_visible() {
            return false;
        }
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let scale = self.dpi_scale.max(1.0);
        let w_dip = (rc.right - rc.left) as f32 / scale;
        let h_dip = (rc.bottom - rc.top) as f32 / scale;
        if w_dip < 680.0 {
            return false;
        }
        let y_dip = y_px as f32 / scale;
        if y_dip < HEADER_HEIGHT_DIP as f32 || y_dip > h_dip - FOOTER_HEIGHT_DIP as f32 {
            return false;
        }
        let pw = (self.preview_width_dip as f32).clamp(220.0, (w_dip * 0.48).max(220.0));
        let div_x_dip = (w_dip - pw).max(380.0);
        let x_dip = x_px as f32 / scale;
        (x_dip - div_x_dip).abs() <= 5.0
    }

    fn show_logo_context_menu(&mut self) {
        const CMD_SETTINGS: u32 = 101;
        const CMD_THEME_LIGHT: u32 = 102;
        const CMD_THEME_DARK: u32 = 103;
        const CMD_THEME_MIDNIGHT: u32 = 104;
        const CMD_LANG_ZH: u32 = 105;
        const CMD_LANG_EN: u32 = 106;
        const CMD_PREVIEW: u32 = 107;
        const CMD_RESET_WIN: u32 = 108;
        const CMD_STATS: u32 = 109;
        const CMD_RELOAD: u32 = 110;
        const CMD_GC: u32 = 111;
        const CMD_CONFIG: u32 = 112;
        const CMD_QUIT: u32 = 113;
        const CMD_UPDATE: u32 = 114;

        let l = self.lang;
        self.suppress_next_deactivate = true;
        let chosen = unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                self.suppress_next_deactivate = false;
                return;
            };
            append_menu_item(menu, CMD_SETTINGS, l.pick("设置中心...\tCtrl+,", "Settings...\tCtrl+,"), false);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

            let is_light = self.theme_name.eq_ignore_ascii_case("light");
            let is_midnight = self.theme_name.eq_ignore_ascii_case("midnight");
            let is_dark = !is_light && !is_midnight;

            append_menu_item(menu, CMD_THEME_LIGHT, l.pick("主题：现代白瓷光", "Theme: Modern White"), is_light);
            append_menu_item(menu, CMD_THEME_DARK, l.pick("主题：深空黑曜石", "Theme: Obsidian Dark"), is_dark);
            append_menu_item(menu, CMD_THEME_MIDNIGHT, l.pick("主题：深邃午夜蓝", "Theme: Deep Midnight"), is_midnight);

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_menu_item(menu, CMD_LANG_ZH, "语言：简体中文", l == Lang::Zh);
            append_menu_item(menu, CMD_LANG_EN, "Language: English", l == Lang::En);

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_menu_item(menu, CMD_PREVIEW, l.pick("右侧实时预览\tCtrl+P", "Live Preview Pane\tCtrl+P"), self.preview_enabled);
            append_menu_item(menu, CMD_RESET_WIN, l.pick("重置窗口位置与尺寸", "Reset Window Position & Size"), false);

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_menu_item(menu, CMD_UPDATE, l.pick("检查更新 (:update)", "Check for Updates (:update)"), false);
            append_menu_item(menu, CMD_STATS, l.pick("内存与插件监控 (:stats)", "Memory & Plugin Stats (:stats)"), false);
            append_menu_item(menu, CMD_RELOAD, l.pick("热重载全部 (:reload)", "Hot-Reload All (:reload)"), false);
            append_menu_item(menu, CMD_GC, l.pick("立即休眠整理内存 (:gc)", "Hibernate & Trim RAM (:gc)"), false);
            append_menu_item(menu, CMD_CONFIG, l.pick("打开 config.toml", "Open config.toml"), false);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_menu_item(menu, CMD_QUIT, l.pick("退出 Tomoyo", "Exit Tomoyo"), false);

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
                pt.x,
                pt.y,
                Some(0),
                self.hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
            cmd.0 as u32
        };
        self.suppress_next_deactivate = false;

        match chosen {
            CMD_SETTINGS => self.open_settings_center(),
            CMD_THEME_LIGHT => self.apply_theme("light", true),
            CMD_THEME_DARK => self.apply_theme("dark", true),
            CMD_THEME_MIDNIGHT => self.apply_theme("midnight", true),
            CMD_LANG_ZH => self.apply_language("zh", true),
            CMD_LANG_EN => self.apply_language("en", true),
            CMD_PREVIEW => {
                self.preview_enabled = !self.preview_enabled;
                self.cached_preview_id = None;
                self.refresh_selected_preview();
                self.resize_to_fit_results();
                self.invalidate();
            }
            CMD_RESET_WIN => self.reset_window_geometry(),
            CMD_UPDATE => {
                self.set_query_text(":update");
                self.trigger_update_check_async();
            }
            CMD_STATS => self.set_query_text(":stats"),
            CMD_RELOAD => {
                self.reload_config_and_all();
                self.show_toast(self.lang.pick("已热重载配置、应用与插件", "Hot-reloaded all"));
            }
            CMD_GC => {
                let c = self.run_hibernate();
                self.show_toast(&format!("内存整理完成 (#{c})"));
            }
            CMD_CONFIG => {
                let path = TomoyoConfig::config_path().to_string_lossy().into_owned();
                let _ = shell_open(&path, None, None);
                self.hide_launcher();
            }
            CMD_QUIT => {
                self.plugin_host.shutdown_all();
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            _ => {}
        }
    }

    fn show_result_context_menu(&mut self, idx: usize) {
        let Some(item) = self.results.get(idx).cloned() else {
            return;
        };
        const CMD_COPY_TITLE: u32 = 9001;
        const CMD_TOGGLE_PREVIEW: u32 = 9002;
        let l = self.lang;

        self.suppress_next_deactivate = true;
        let chosen = unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                self.suppress_next_deactivate = false;
                return;
            };

            for (ai, act) in item.actions.iter().enumerate() {
                let label = match &act.shortcut {
                    Some(sc) => format!("{}\t{sc}", act.label),
                    None => act.label.clone(),
                };
                append_menu_item(menu, (ai + 1) as u32, &label, false);
            }

            if !item.actions.is_empty() {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            append_menu_item(menu, CMD_COPY_TITLE, l.pick("复制文本内容", "Copy Text"), false);
            append_menu_item(
                menu,
                CMD_TOGGLE_PREVIEW,
                l.pick("右侧实时预览\tCtrl+P", "Live Preview Pane\tCtrl+P"),
                self.preview_enabled,
            );

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
                pt.x,
                pt.y,
                Some(0),
                self.hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
            cmd.0 as u32
        };
        self.suppress_next_deactivate = false;

        if chosen == 0 {
            return;
        }
        if chosen == CMD_COPY_TITLE {
            let text = item.copy_text.as_deref().unwrap_or(&item.title);
            if set_clipboard_text(text) {
                self.show_toast(self.lang.pick("已复制到剪贴板", "Copied to clipboard"));
            }
            return;
        }
        if chosen == CMD_TOGGLE_PREVIEW {
            self.preview_enabled = !self.preview_enabled;
            self.cached_preview_id = None;
            self.refresh_selected_preview();
            self.resize_to_fit_results();
            self.invalidate();
            return;
        }
        let action_idx = (chosen - 1) as usize;
        if action_idx < item.actions.len() {
            self.selected = idx;
            self.execute_selected(action_idx);
        }
    }

    fn handle_nav_key(&mut self, vk: u16, ctrl_down: bool, alt_down: bool) {
        if alt_down && (b'1' as u16..=b'8' as u16).contains(&vk) {
            let slot = (vk - b'1' as u16) as usize;
            let idx = self.scroll_offset + slot;
            if idx < self.results.len() {
                self.selected = idx;
                self.execute_selected(0);
            }
            return;
        }

        if ctrl_down && vk == b'P' as u16 {
            self.preview_enabled = !self.preview_enabled;
            self.cached_preview_id = None;
            self.refresh_selected_preview();
            self.resize_to_fit_results();
            self.show_toast(if self.preview_enabled {
                self.lang.pick("已开启右侧实时预览 (Ctrl+P)", "Live Preview enabled (Ctrl+P)")
            } else {
                self.lang.pick("已关闭右侧实时预览 (Ctrl+P)", "Live Preview hidden (Ctrl+P)")
            });
            return;
        }

        if ctrl_down && vk == 0xBC {
            self.open_settings_center();
            return;
        }

        if ctrl_down && vk == b'C' as u16 {
            let mut start = 0u32;
            let mut end = 0u32;
            unsafe {
                SendMessageW(
                    self.edit_hwnd,
                    EM_GETSEL,
                    Some(WPARAM(&mut start as *mut _ as usize)),
                    Some(LPARAM(&mut end as *mut _ as isize)),
                );
            }
            if start == end {
                if let Some(item) = self.results.get(self.selected) {
                    if let Some(txt) = &item.copy_text {
                        if set_clipboard_text(txt) {
                            self.show_toast(self.lang.pick("已复制选中项内容", "Copied selected result"));
                        }
                    }
                }
            }
            return;
        }

        match vk {
            x if x == VK_BACK.0 => {
                if self.attached.is_some() && self.current_query.is_empty() {
                    self.attached = None;
                    self.show_toast(self.lang.pick("已清除粘贴胶囊", "Cleared attached capsule"));
                    self.dispatch_query();
                    self.invalidate();
                }
            }
            x if x == VK_ESCAPE.0 => {
                if self.action_menu_open {
                    self.action_menu_open = false;
                    self.selected_action = 0;
                    self.invalidate();
                } else if self.attached.is_some() {
                    self.attached = None;
                    self.dispatch_query();
                    self.invalidate();
                } else {
                    self.hide_launcher();
                }
            }
            x if x == VK_TAB.0 => {
                if let Some(item) = self.results.get(self.selected) {
                    if !item.actions.is_empty() {
                        self.action_menu_open = !self.action_menu_open;
                        self.selected_action = 0;
                        self.invalidate();
                    }
                }
            }
            x if x == VK_DOWN.0 => {
                if self.action_menu_open {
                    if let Some(item) = self.results.get(self.selected) {
                        if !item.actions.is_empty() {
                            self.selected_action = (self.selected_action + 1) % item.actions.len();
                            self.invalidate();
                        }
                    }
                } else if !self.results.is_empty() {
                    self.selected = (self.selected + 1) % self.results.len();
                    self.ensure_selected_visible();
                    self.refresh_selected_preview();
                    self.resize_to_fit_results();
                    self.invalidate();
                }
            }
            x if x == VK_UP.0 => {
                if self.action_menu_open {
                    if let Some(item) = self.results.get(self.selected) {
                        if !item.actions.is_empty() {
                            self.selected_action = if self.selected_action == 0 {
                                item.actions.len() - 1
                            } else {
                                self.selected_action - 1
                            };
                            self.invalidate();
                        }
                    }
                } else if !self.results.is_empty() {
                    self.selected = if self.selected == 0 {
                        self.results.len() - 1
                    } else {
                        self.selected - 1
                    };
                    self.ensure_selected_visible();
                    self.refresh_selected_preview();
                    self.resize_to_fit_results();
                    self.invalidate();
                }
            }
            x if x == VK_RETURN.0 => {
                if self.action_menu_open {
                    self.execute_selected(self.selected_action);
                } else if ctrl_down {
                    self.execute_selected(1);
                } else {
                    self.execute_selected(0);
                }
            }
            _ => {}
        }
    }

    fn execute_selected(&mut self, action_idx: usize) {
        let Some(item) = self.results.get(self.selected).cloned() else {
            return;
        };
        let Some(action) = item.actions.get(action_idx).or_else(|| item.actions.first()) else {
            return;
        };

        self.router.record_execution(&item.id);
        if matches!(action.kind, ActionKind::CopyImage { .. }) {
            self.suppress_next_clip_img = true;
        }
        let outcome = execute_action(&action.kind, &item.id, &self.plugin_host, self.theme.is_dark);

        if let Some(internal) = outcome.internal {
            match internal {
                InternalCommandKind::TriggerHibernateGc => {
                    let cycles = self.run_hibernate();
                    self.show_toast(&format!("已完成休眠内存整理 (#{cycles})"));
                    self.set_query_text(":stats");
                    return;
                }
                InternalCommandKind::ReloadAll => {
                    self.reload_config_and_all();
                    self.show_toast(self.lang.pick("已热重载配置、应用与插件", "Hot-reloaded all"));
                    self.dispatch_query();
                    return;
                }
                InternalCommandKind::QuitApp => {
                    self.plugin_host.shutdown_all();
                    unsafe {
                        let _ = DestroyWindow(self.hwnd);
                    }
                    return;
                }
                InternalCommandKind::RefreshQuery => {
                    self.dispatch_query();
                    return;
                }
                InternalCommandKind::OpenConfigFile => {
                    let path = TomoyoConfig::config_path().to_string_lossy().into_owned();
                    let _ = shell_open(&path, None, None);
                    self.hide_launcher();
                    return;
                }
                InternalCommandKind::OpenSettingsWindow => {
                    self.open_settings_center();
                    return;
                }
                InternalCommandKind::ResetWindowGeometry => {
                    self.reset_window_geometry();
                    return;
                }
                InternalCommandKind::SetTheme { theme } => {
                    self.apply_theme(&theme, true);
                    self.show_toast(self.lang.pick("已实时切换主题并保存", "Theme updated & saved"));
                    self.dispatch_query();
                    return;
                }
                InternalCommandKind::SetLanguage { lang } => {
                    self.apply_language(&lang, true);
                    self.show_toast(self.lang.pick("已切换界面语言", "Display language updated"));
                    return;
                }
                InternalCommandKind::ToggleClipboardPin { id_key } => {
                    let pinned = self.clipboard.toggle_pin(&id_key);
                    self.show_toast(if pinned {
                        self.lang.pick("已置顶该剪贴板记录", "Clipboard entry pinned")
                    } else {
                        self.lang.pick("已取消置顶", "Clipboard entry unpinned")
                    });
                    self.dispatch_query();
                    return;
                }
                InternalCommandKind::CheckForUpdates => {
                    self.trigger_update_check_async();
                    return;
                }
                InternalCommandKind::ClearAttachedContext => {
                    self.attached = None;
                    self.dispatch_query();
                    self.invalidate();
                    return;
                }
                InternalCommandKind::KillProcess { pid, name } => {
                    match kill_process_by_pid(pid) {
                        Ok(()) => self.show_toast(&match self.lang {
                            Lang::Zh => format!("已结束进程 {name} (PID {pid})"),
                            Lang::En => format!("Terminated {name} (PID {pid})"),
                        }),
                        Err(e) => self.show_toast(&e),
                    }
                    self.dispatch_query();
                    return;
                }
                InternalCommandKind::ToggleAutostart => {
                    let target = !is_autostart_enabled();
                    match set_autostart_enabled(target) {
                        Ok(()) => self.show_toast(if target {
                            self.lang.pick("已开启开机静默自启", "Launch at Windows login ENABLED")
                        } else {
                            self.lang.pick("已关闭开机自启", "Launch at Windows login DISABLED")
                        }),
                        Err(e) => self.show_toast(&format!("Autostart error: {e}")),
                    }
                    self.dispatch_query();
                    return;
                }
                InternalCommandKind::TogglePreviewPane => {
                    self.preview_enabled = !self.preview_enabled;
                    self.cached_preview_id = None;
                    self.refresh_selected_preview();
                    self.resize_to_fit_results();
                    self.invalidate();
                    return;
                }
            }
        }

        if let Some(fill) = outcome.fill_query {
            self.action_menu_open = false;
            self.set_query_text(&fill);
            return;
        }

        if let Some(toast) = outcome.toast {
            self.show_toast(&toast);
        }

        if outcome.hide_window {
            let prev = self.prev_foreground_hwnd;
            self.hide_launcher();
            if outcome.paste_after_hide {
                paste_into_previous_window_async(prev);
            }
        } else {
            self.action_menu_open = false;
            self.invalidate();
        }
    }

    fn ensure_selected_visible(&mut self) {
        let cap = self.max_visible_rows.max(1);
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + cap {
            self.scroll_offset = self.selected + 1 - cap;
        }
    }

    fn show_toast(&mut self, msg: &str) {
        self.toast = Some(msg.to_string());
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CLEAR_TOAST);
            let _ = SetTimer(Some(self.hwnd), TIMER_CLEAR_TOAST, 3000, None);
        }
        self.invalidate();
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn hit_test_row(&self, x_px: i32, y_px: i32) -> Option<usize> {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let scale = self.dpi_scale.max(1.0);
        let w_dip = (rc.right - rc.left) as f32 / scale;
        let x_dip = x_px as f32 / scale;
        let y_dip = y_px as f32 / scale;

        let list_w_dip = if self.is_preview_visible() && w_dip >= 680.0 {
            let pw = (self.preview_width_dip as f32).clamp(220.0, (w_dip * 0.48).max(220.0));
            (w_dip - pw).max(380.0)
        } else {
            w_dip
        };
        if x_dip > list_w_dip {
            return None;
        }
        let list_top = HEADER_HEIGHT_DIP as f32 + 4.0;
        if y_dip < list_top {
            return None;
        }
        let slot = ((y_dip - list_top) / ROW_HEIGHT_DIP as f32) as usize;
        if slot >= self.max_visible_rows.max(1) {
            return None;
        }
        let idx = self.scroll_offset + slot;
        (idx < self.results.len()).then_some(idx)
    }

    fn render_into_hdc(&mut self, hdc: HDC, width: i32, height: i32) {
        let mode = self.active_mode_badge();
        let attached_label = self.attached_capsule_label();
        let update_label = self
            .updater
            .available_release()
            .map(|r| format!("↑ v{}", r.version));
        let state = RenderState {
            lang: self.lang,
            theme: self.theme,
            mode_badge: mode,
            attached_capsule: attached_label.as_deref(),
            update_badge: update_label.as_deref(),
            items: &self.results,
            selected: self.selected,
            hovered: self.hovered,
            scroll_offset: self.scroll_offset,
            max_visible_rows: self.max_visible_rows,
            preview_width_dip: self.preview_width_dip,
            action_menu_open: self.action_menu_open,
            selected_action: self.selected_action,
            preview: self.cached_preview.as_ref(),
            everything_status: self.everything_status,
            toast: self.toast.as_deref(),
            dpi_scale: self.dpi_scale,
        };
        self.renderer.paint(hdc, width, height, &state);
    }
}

impl Drop for AppShell {
    fn drop(&mut self) {
        self.plugin_host.shutdown_all();
        self.renderer.hibernate_release();
        self.tray = None;
        unsafe {
            if !self.edit_font.is_invalid() {
                let _ = DeleteObject(self.edit_font.into());
            }
            if !self.edit_bg_brush.is_invalid() {
                let _ = DeleteObject(self.edit_bg_brush.into());
            }
        }
    }
}

unsafe fn append_menu_item(
    menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    id: u32,
    text: &str,
    checked: bool,
) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let flags = MF_STRING | if checked { MF_CHECKED } else { Default::default() };
    let _ = unsafe { AppendMenuW(menu, flags, id as usize, PCWSTR(wide.as_ptr())) };
}

unsafe fn apply_cue_banner(edit_hwnd: HWND, lang: Lang) {
    let text = lang.pick(
        "搜索应用(支持拼音)、Everything 文件 (f)、剪贴板 (cb)、网页 (g)、插件或 :settings...",
        "Search apps, Everything (f), clipboard (cb), web (g), wasm, json, :settings...",
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

fn apply_dwm_attributes(hwnd: HWND, is_dark: bool) {
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

fn register_configured_hotkey(hwnd: HWND, spec: &str) -> String {
    if let Some(parsed) = parse_hotkey(spec) {
        unsafe {
            if RegisterHotKey(
                Some(hwnd),
                HOTKEY_ID,
                HOT_KEY_MODIFIERS(parsed.modifiers),
                parsed.vk,
            )
            .is_ok()
            {
                return spec.to_string();
            }
        }
    }
    unsafe {
        let _ = RegisterHotKey(
            Some(hwnd),
            HOTKEY_ID,
            HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0),
            VK_SPACE.0 as u32,
        );
    }
    "Ctrl+Shift+Space".to_string()
}

unsafe extern "system" fn main_wndproc(
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
        WM_NCHITTEST => {
            let mut pt = POINT {
                x: (lparam.0 & 0xFFFF) as i16 as i32,
                y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
            };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }
            let mut rc = RECT::default();
            unsafe {
                let _ = GetClientRect(hwnd, &mut rc);
            }
            let w = rc.right - rc.left;
            let h = rc.bottom - rc.top;
            let grip = (6.0 * app.dpi_scale.max(1.0)).round() as i32;

            let left = pt.x < grip;
            let right = pt.x >= w - grip;
            let top = pt.y < grip;
            let bottom = pt.y >= h - grip;

            let hit = match (left, right, top, bottom) {
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
                    if pt.y < header_px || pt.y >= h - footer_px {
                        HTCAPTION
                    } else {
                        HTCLIENT
                    }
                }
            };
            LRESULT(hit as isize)
        }
        WM_SETCURSOR => {
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(hwnd, &mut pt);
                }
                if app.dragging_divider || app.is_over_preview_divider(pt.x, pt.y) {
                    if let Ok(cur) = unsafe { LoadCursorW(None, IDC_SIZEWE) } {
                        unsafe {
                            SetCursor(Some(cur));
                        }
                        return LRESULT(1);
                    }
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_ENTERSIZEMOVE => {
            app.in_size_move = true;
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            app.in_size_move = false;
            app.persist_window_geometry();
            LRESULT(0)
        }
        WM_SIZE => {
            let new_w = (lparam.0 & 0xFFFF) as i32;
            let new_h = ((lparam.0 >> 16) & 0xFFFF) as i32;
            if new_w > 0 && new_h > 0 {
                app.layout_edit_control(new_w);
                if app.in_size_move {
                    let scale = app.dpi_scale.max(1.0);
                    let w_dip = (new_w as f32 / scale).round() as i32;
                    let h_dip = (new_h as f32 / scale).round() as i32;
                    if app.is_preview_visible() {
                        app.list_width_dip =
                            (w_dip - app.preview_width_dip).clamp(MIN_WINDOW_WIDTH_DIP, 1400);
                    } else {
                        app.list_width_dip = w_dip.clamp(MIN_WINDOW_WIDTH_DIP, 1400);
                    }
                    if !app.results.is_empty() {
                        let avail_h = (h_dip - HEADER_HEIGHT_DIP - FOOTER_HEIGHT_DIP - 8).max(0);
                        let rows = (avail_h / ROW_HEIGHT_DIP).clamp(4, 18) as usize;
                        app.max_visible_rows = rows;
                    }
                }
                app.invalidate();
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
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as usize;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            if id == EDIT_CTRL_ID && code == EN_CHANGE {
                app.on_edit_changed();
                return LRESULT(0);
            }
            match id {
                MENU_ID_SHOW => app.show_launcher(),
                MENU_ID_SETTINGS => app.open_settings_center(),
                MENU_ID_UPDATE => {
                    app.suppress_next_deactivate = true;
                    app.show_launcher();
                    app.set_query_text(":update");
                    app.trigger_update_check_async();
                }
                MENU_ID_STATS => {
                    app.suppress_next_deactivate = true;
                    app.show_launcher();
                    app.set_query_text(":stats");
                }
                MENU_ID_CONFIG => {
                    let path = TomoyoConfig::config_path().to_string_lossy().into_owned();
                    let _ = shell_open(&path, None, None);
                }
                MENU_ID_RELOAD => {
                    app.reload_config_and_all();
                    app.show_launcher();
                    app.show_toast(app.lang.pick("已热重载配置、应用与插件", "Hot-reloaded all"));
                }
                MENU_ID_GC => {
                    let _ = app.run_hibernate();
                }
                MENU_ID_QUIT => {
                    app.plugin_host.shutdown_all();
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
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
                app.suppress_next_clip_img = false;
                app.clipboard.push_text(txt);
            } else if app.suppress_next_clip_img {
                app.suppress_next_clip_img = false;
            } else if let Some((path, w, h, digest)) = read_clipboard_image_png() {
                app.clipboard.push_image(path, w, h, digest);
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            let active = (wparam.0 & 0xFFFF) != 0;
            if !active {
                if app.suppress_next_deactivate {
                    app.suppress_next_deactivate = false;
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
                        unsafe {
                            let _ = KillTimer(Some(hwnd), TIMER_HIBERNATE);
                        }
                    }
                }
                TIMER_CLEAR_TOAST => {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), TIMER_CLEAR_TOAST);
                    }
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
        WM_APP_EDIT_KEY => {
            let vk = (wparam.0 & 0xFFFF) as u16;
            let ctrl = (lparam.0 & 1) != 0;
            let alt = (lparam.0 & 2) != 0;
            app.handle_nav_key(vk, ctrl, alt);
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
        WM_NCRBUTTONUP => {
            if wparam.0 as u32 == HTCAPTION {
                app.show_logo_context_menu();
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_RBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
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
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            if app.is_over_preview_divider(x, y) {
                app.dragging_divider = true;
                unsafe {
                    SetCapture(hwnd);
                }
                return LRESULT(0);
            }
            if let Some(idx) = app.hit_test_row(x, y) {
                app.selected = idx;
                app.refresh_selected_preview();
                app.resize_to_fit_results();
                app.invalidate();

                if let Some(IconSpec::ShellPath(path)) =
                    app.results.get(idx).map(|r| r.icon.clone())
                {
                    app.drag_origin = Some((x, y, path));
                } else {
                    app.drag_origin = None;
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

            if !app.mouse_tracked {
                app.mouse_tracked = true;
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

            if app.dragging_divider {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rc);
                }
                let scale = app.dpi_scale.max(1.0);
                let w_dip = (rc.right - rc.left) as f32 / scale;
                let x_dip = x as f32 / scale;
                let new_preview = (w_dip - x_dip).round() as i32;
                let clamped = new_preview.clamp(220, ((w_dip - 380.0) as i32).max(220).min(620));
                if clamped != app.preview_width_dip {
                    app.preview_width_dip = clamped;
                    app.list_width_dip = (w_dip as i32 - clamped).clamp(MIN_WINDOW_WIDTH_DIP, 1400);
                    app.invalidate();
                }
                return LRESULT(0);
            }

            if let Some((ox, oy, path)) = app.drag_origin.clone() {
                if (x - ox).abs() > 6 || (y - oy).abs() > 6 {
                    app.drag_origin = None;
                    app.suppress_next_deactivate = true;
                    let _ = start_file_drag(&path);
                    return LRESULT(0);
                }
            }

            let new_hover = app.hit_test_row(x, y);
            if new_hover != app.hovered {
                app.hovered = new_hover;
                app.invalidate();
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            app.mouse_tracked = false;
            if app.hovered.is_some() {
                app.hovered = None;
                app.invalidate();
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            app.drag_origin = None;
            if app.dragging_divider {
                app.dragging_divider = false;
                unsafe {
                    let _ = ReleaseCapture();
                }
                app.persist_window_geometry();
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            app.drag_origin = None;
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
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
                if let Some(tray) = &app.tray {
                    app.suppress_next_deactivate = true;
                    tray.show_context_menu(app.lang);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
                let _ = RemoveClipboardFormatListener(hwnd);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn edit_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid_subclass: usize,
    ref_data: usize,
) -> LRESULT {
    let parent = HWND(ref_data as *mut c_void);
    match msg {
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            let ctrl = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
            let intercept = matches!(
                vk,
                x if x == VK_UP.0
                    || x == VK_DOWN.0
                    || x == VK_RETURN.0
                    || x == VK_ESCAPE.0
                    || x == VK_TAB.0
            );
            if intercept {
                let flags = if ctrl { 1isize } else { 0 };
                unsafe {
                    let _ = PostMessageW(
                        Some(parent),
                        WM_APP_EDIT_KEY,
                        WPARAM(vk as usize),
                        LPARAM(flags),
                    );
                }
                return LRESULT(0);
            }
            if ctrl && vk == b'V' as u16 && clipboard_has_rich_attachment() {
                unsafe {
                    let _ = PostMessageW(
                        Some(parent),
                        WM_APP_PASTE_INTERCEPT,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
                return LRESULT(0);
            }
            if vk == VK_BACK.0 && unsafe { GetWindowTextLengthW(hwnd) } == 0 {
                unsafe {
                    let _ = PostMessageW(
                        Some(parent),
                        WM_APP_EDIT_KEY,
                        WPARAM(vk as usize),
                        LPARAM(0),
                    );
                }
            }
            if ctrl && (vk == b'P' as u16 || vk == 0xBC) {
                unsafe {
                    let _ = PostMessageW(
                        Some(parent),
                        WM_APP_EDIT_KEY,
                        WPARAM(vk as usize),
                        LPARAM(1),
                    );
                }
                return LRESULT(0);
            }
            if ctrl && vk == b'C' as u16 {
                let mut start = 0u32;
                let mut end = 0u32;
                unsafe {
                    SendMessageW(
                        hwnd,
                        EM_GETSEL,
                        Some(WPARAM(&mut start as *mut _ as usize)),
                        Some(LPARAM(&mut end as *mut _ as isize)),
                    );
                }
                if start == end {
                    unsafe {
                        let _ = PostMessageW(
                            Some(parent),
                            WM_APP_EDIT_KEY,
                            WPARAM(vk as usize),
                            LPARAM(1),
                        );
                    }
                    return LRESULT(0);
                }
            }
        }
        WM_SYSKEYDOWN => {
            let vk = wparam.0 as u16;
            if (b'1' as u16..=b'8' as u16).contains(&vk) {
                unsafe {
                    let _ = PostMessageW(
                        Some(parent),
                        WM_APP_EDIT_KEY,
                        WPARAM(vk as usize),
                        LPARAM(2),
                    );
                }
                return LRESULT(0);
            }
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

fn create_edit_font(dpi_scale: f32, face: &str) -> HFONT {
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

fn read_edit_text(edit: HWND) -> String {
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

fn detect_everything_label() -> Option<&'static str> {
    match tomoyo_everything::detect_running() {
        Some(tomoyo_everything::EverythingVersion::V15Alpha) => Some("Everything 1.5a"),
        Some(tomoyo_everything::EverythingVersion::V14) => Some("Everything 1.4"),
        None => None,
    }
}
