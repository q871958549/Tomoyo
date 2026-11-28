//! Linux GTK3/Cairo/Pango `AppShell` core state, lifecycle, query worker, layout & self-test.

use crate::linux::hotkeys::{HotkeyAction, HotkeyManager};
use crate::linux::ipc::{schedule_on_ui, start_ipc_listeners};
use crate::linux::render_pango::LinuxRenderer;
use crate::linux::shell_query::{spawn_query_worker, QueryJob};
use crate::linux::{
    ensure_tray_icon_png, mi_collect, tomoyo_linux_add_timeout, tomoyo_linux_remove_source,
    tomoyo_linux_tray_setup, tomoyo_linux_ui_create, tomoyo_linux_ui_destroy,
    tomoyo_linux_ui_get_entry_text, tomoyo_linux_ui_hide, tomoyo_linux_ui_is_visible,
    tomoyo_linux_ui_queue_draw, tomoyo_linux_ui_resize, tomoyo_linux_ui_set_entry_bounds,
    tomoyo_linux_ui_set_entry_text, tomoyo_linux_ui_set_placeholder, tomoyo_linux_ui_set_theme,
    tomoyo_linux_ui_show, tomoyo_wl_clip_start, tomoyo_wl_clip_stop,
};
use crate::linux::shell_events;
use crate::linux::sidebar_ctrl::SidebarState;
use crate::preview_loader::AsyncPreviewLoader;
use crate::renderer::{FilterCounts, ThemePalette, MAX_WINDOW_WIDTH_DIP, MIN_WINDOW_WIDTH_DIP};
use std::cell::RefCell;
use std::ffi::{c_int, c_void, CStr, CString};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tomoyo_core::{
    trim_working_set, Lang, QueryRouter, ResolvedPreview, TomoyoConfig, TypographyConfig, UpdateManager,
};
use tomoyo_plugin_host::{ActivePluginTrayAction, PluginHost};
use tomoyo_protocol::{AttachedContext, ClipboardSnapshot, ResultItem, SearchFilter};
use tomoyo_providers::everything_linux::is_linux_file_search_available;

thread_local! {
    static CURRENT_CORE: RefCell<Option<Weak<RefCell<AppCore>>>> = const { RefCell::new(None) };
}

pub(super) fn with_current_core<F: FnOnce(&mut AppCore)>(f: F) {
    let weak = CURRENT_CORE.with(|c| c.borrow().clone());
    if let Some(w) = weak
        && let Some(rc) = w.upgrade()
        && let Ok(mut guard) = rc.try_borrow_mut()
    {
        f(&mut guard);
    }
}

pub struct AppCore {
    pub(super) ui: *mut c_void,
    pub(super) config: Arc<RwLock<TomoyoConfig>>,
    pub(super) router: Arc<QueryRouter>,
    pub(super) plugin_host: Arc<PluginHost>,
    pub(super) last_clip_snapshot: Option<ClipboardSnapshot>,
    pub(super) last_rich_clip_at: Option<Instant>,
    pub(super) wl_clip_active: bool,
    pub(super) updater: Arc<UpdateManager>,
    pub(super) renderer: LinuxRenderer,
    pub(super) preview_loader: AsyncPreviewLoader,
    pub(super) hotkeys: HotkeyManager,
    pub(super) lang: Lang,
    pub(super) theme_name: String,
    pub(super) theme: ThemePalette,
    pub(super) current_query: String,
    pub(super) results: Vec<ResultItem>,
    pub(super) selected: usize,
    pub(super) scroll_offset: usize,
    pub(super) hovered: Option<usize>,
    pub(super) filter_counts: Option<FilterCounts>,
    pub(super) counts_query: String,
    pub(super) cached_all_query: String,
    pub(super) cached_all_results: Vec<ResultItem>,
    pub(super) active_filter: SearchFilter,
    pub(super) hovered_filter: Option<SearchFilter>,
    pub(super) hovered_logo: bool,
    pub(super) sidebar: SidebarState,
    pub(super) show_filter_bar: bool,
    pub(super) typography: TypographyConfig,
    pub(super) preview_enabled: bool,
    pub(super) preview_width_dip: i32,
    pub(super) list_width_dip: i32,
    pub(super) max_visible_rows: usize,
    pub(super) custom_height_dip: Option<i32>,
    pub(super) last_applied_height_dip: i32,
    pub(super) last_geom_change_at: Option<Instant>,
    pub(super) geom_save_source: u32,
    pub(super) kwin_pos: *mut c_void,
    pub(super) dpi_scale: f32,
    pub(super) custom_pos: Option<(i32, i32)>,
    pub(super) dragging_divider: bool,
    pub(super) scrollbar: crate::linux::scrollbar_ctrl::ScrollbarState,
    pub(super) action_menu_open: bool,
    pub(super) selected_action: usize,
    pub(super) menu_open: bool,
    pub(super) menu_closed_at: Option<Instant>,
    pub(super) menu_refocus: bool,
    pub(super) launching_row: Option<(usize, bool)>,
    pub(super) toast: Option<(String, Instant)>,
    pub(super) attached: Option<AttachedContext>,
    pub(super) cached_preview_id: Option<String>,
    pub(super) cached_preview: Option<ResolvedPreview>,
    pub(super) query_tx: Sender<QueryJob>,
    pub(super) query_in_flight: Arc<AtomicUsize>,
    pub(super) suppress_clipboard: Arc<AtomicBool>,
    pub(super) alive: Arc<AtomicBool>,
    pub(super) shutting_down: bool,
    pub(super) shown_at: Instant,
    hibernate_source: u32,
    pub(super) everything_ok: bool,
    pub(super) tray_plugin_actions: Vec<ActivePluginTrayAction>,
}

impl AppCore {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn create_rc(
        config: Arc<RwLock<TomoyoConfig>>,
        router: Arc<QueryRouter>,
        plugin_host: Arc<PluginHost>,
        updater: Arc<UpdateManager>,
        font_regular: &'static str,
        install_tray: bool,
    ) -> Result<Rc<RefCell<Self>>, String> {
        let cfg = config.read().map(|c| c.clone()).unwrap_or_default();
        let (query_tx, query_rx) = mpsc::channel::<QueryJob>();
        let query_in_flight = Arc::new(AtomicUsize::new(0));
        let alive = Arc::new(AtomicBool::new(true));
        let theme = ThemePalette::from_name(&cfg.theme);

        let core = Rc::new(RefCell::new(AppCore {
            ui: std::ptr::null_mut(),
            config: Arc::clone(&config),
            router: Arc::clone(&router),
            plugin_host: Arc::clone(&plugin_host),
            last_clip_snapshot: None,
            last_rich_clip_at: None,
            wl_clip_active: false,
            updater,
            renderer: LinuxRenderer::new(font_regular),
            preview_loader: AsyncPreviewLoader::new(),
            hotkeys: if install_tray { HotkeyManager::new() } else { HotkeyManager::new_inactive() },
            lang: cfg.lang(),
            theme_name: cfg.theme.clone(),
            theme,
            current_query: String::new(),
            results: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            hovered: None,
            filter_counts: None,
            counts_query: String::new(),
            cached_all_query: String::new(),
            cached_all_results: Vec::new(),
            active_filter: SearchFilter::All,
            hovered_filter: None,
            hovered_logo: false,
            sidebar: SidebarState::default(),
            show_filter_bar: cfg.show_filter_bar,
            typography: cfg.typography.clone(),
            preview_enabled: cfg.auto_preview,
            preview_width_dip: cfg.window.preview_width_dip.clamp(220, 620),
            list_width_dip: cfg.window.width_dip.clamp(MIN_WINDOW_WIDTH_DIP, MAX_WINDOW_WIDTH_DIP),
            max_visible_rows: cfg.window.max_visible_rows.clamp(4, 18),
            custom_height_dip: cfg.window.custom_height_dip.map(|v| v.clamp(92, 1080)),
            last_applied_height_dip: 0,
            last_geom_change_at: None,
            geom_save_source: 0,
            kwin_pos: std::ptr::null_mut(),
            dpi_scale: 1.0,
            custom_pos: cfg.window.custom_x.zip(cfg.window.custom_y),
            dragging_divider: false,
            scrollbar: Default::default(),
            action_menu_open: false,
            selected_action: 0,
            menu_open: false,
            menu_closed_at: None,
            menu_refocus: false,
            launching_row: None,
            toast: None,
            attached: None,
            cached_preview_id: None,
            cached_preview: None,
            query_tx,
            query_in_flight: Arc::clone(&query_in_flight),
            suppress_clipboard: Arc::new(AtomicBool::new(false)),
            alive: Arc::clone(&alive),
            shutting_down: false,
            shown_at: Instant::now(),
            hibernate_source: 0,
            everything_ok: is_linux_file_search_available(),
            tray_plugin_actions: Vec::new(),
        }));

        CURRENT_CORE.with(|c| *c.borrow_mut() = Some(Rc::downgrade(&core)));
        spawn_query_worker(router, query_rx, query_in_flight);

        if install_tray {
            let core_ptr = Rc::as_ptr(&core) as *mut c_void;
            let ui = unsafe {
                tomoyo_linux_ui_create(
                    core_ptr,
                    shell_events::on_draw_cb,
                    shell_events::on_key_cb,
                    shell_events::on_mouse_cb,
                    shell_events::on_configure_cb,
                    shell_events::on_text_changed_cb,
                    shell_events::on_paste_cb,
                    shell_events::on_focus_out_cb,
                    shell_events::on_clip_change_cb,
                    shell_events::on_tray_cmd_cb,
                )
            };
            if ui.is_null() {
                return Err("GTK3 初始化失败 (无可用 Wayland/X11 显示服务器)".into());
            }
            {
                let mut c = core.borrow_mut();
                c.ui = ui;
                c.init_kwin_pos_bridge(core_ptr);
                c.apply_entry_theme_and_placeholder();
                c.hotkeys.register_all(&cfg, &plugin_host);
                c.hotkeys.set_handler(|act| {
                    with_current_core(|st| match act {
                        HotkeyAction::ToggleMain => st.toggle_launcher(),
                        HotkeyAction::SuperPanel => st.open_super_panel(),
                        HotkeyAction::Plugin(hk) => st.handle_plugin_hotkey(hk),
                        HotkeyAction::Flow(id) => st.execute_workflow(&id, ""),
                    });
                });
                if let Some(tray_icon) = ensure_tray_icon_png()
                    && let Ok(ip) = CString::new(tray_icon.to_string_lossy().as_ref())
                    && let Ok(hl) = CString::new(format!("Tomoyo ({})", c.hotkeys.active_label))
                {
                    unsafe { tomoyo_linux_tray_setup(ui, ip.as_ptr(), hl.as_ptr()); }
                    c.rebuild_tray_menu();
                }
                c.preview_loader.set_notify_callback(|| {
                    schedule_on_ui(|| with_current_core(|st| {
                        st.refresh_selected_preview();
                        st.invalidate();
                    }));
                });
                if let Some(msg) = c.hotkeys.conflict_msg.clone() {
                    tracing::warn!("{msg}");
                }
            }
            start_ipc_listeners(alive, |line| {
                with_current_core(|st| st.handle_ipc_line(&line));
            });
            core.borrow_mut().sync_clipboard_watcher();
        }

        Ok(core)
    }

    pub(super) fn sync_clipboard_watcher(&mut self) {
        if !self.wl_clip_active {
            unsafe { tomoyo_wl_clip_start(shell_events::on_wl_clip_cb); }
            self.wl_clip_active = true;
        }
    }

    pub(super) fn apply_entry_theme_and_placeholder(&self) {
        if self.ui.is_null() { return; }
        let (t, b, ac) = (self.theme.text_primary, self.theme.bg, self.theme.accent);
        let rgb = ((t.0 as u32) << 16) | ((t.1 as u32) << 8) | (t.2 as u32);
        let bgrgb = ((b.0 as u32) << 16) | ((b.1 as u32) << 8) | (b.2 as u32);
        let acrgb = ((ac.0 as u32) << 16) | ((ac.1 as u32) << 8) | (ac.2 as u32);
        let fam_css = tomoyo_core::format_css_font_stack(&self.typography.font_family, "sans-serif");
        let font_px = (self.typography.title_size as i32).clamp(15, 18);
        if let Ok(cf) = CString::new(fam_css) { unsafe { tomoyo_linux_ui_set_theme(self.ui, rgb, bgrgb, acrgb, cf.as_ptr(), font_px); } }
        unsafe { tomoyo_linux_ui_set_placeholder(self.ui, c"".as_ptr()); }
    }

    pub(super) fn apply_font_family(&mut self, family: &str, label: &str) {
        self.typography.font_family = family.to_string();
        tomoyo_core::sanitize_typography(&mut self.typography);
        self.apply_entry_theme_and_placeholder();
        if let Ok(mut cfg) = self.config.write() { cfg.typography = self.typography.clone(); let _ = cfg.save(); }
        let msg = match self.lang { Lang::Zh => format!("已切换字体：{label}"), Lang::En => format!("Font switched: {label}") };
        self.show_toast(&msg);
        self.dispatch_query();
        self.invalidate();
    }

    pub(super) fn apply_theme(&mut self, name: &str, persist: bool) {
        self.theme = ThemePalette::from_name(name);
        self.theme_name = name.to_string();
        self.apply_entry_theme_and_placeholder();
        if persist && let Ok(mut cfg) = self.config.write() { cfg.theme = name.to_string(); let _ = cfg.save(); }
        self.invalidate();
    }

    pub(super) fn apply_language(&mut self, code: &str, persist: bool) {
        self.lang = Lang::from_code(code);
        self.apply_entry_theme_and_placeholder();
        if persist && let Ok(mut cfg) = self.config.write() { cfg.language = self.lang.code().to_string(); let _ = cfg.save(); }
        self.dispatch_query();
        self.invalidate();
    }

    pub(super) fn is_visible(&self) -> bool {
        !self.ui.is_null() && unsafe { tomoyo_linux_ui_is_visible(self.ui) != 0 }
    }

    pub(super) fn show_launcher_with_query(&mut self, query: Option<&str>) {
        self.cancel_hibernate_timer();
        self.menu_open = false;
        self.menu_closed_at = None;
        self.menu_refocus = false;
        if let Some(q) = query {
            self.set_query_text(q);
        } else if !self.ui.is_null() {
            let ptr = unsafe { tomoyo_linux_ui_get_entry_text(self.ui) };
            if !ptr.is_null() {
                self.current_query = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
            }
        }
        self.dispatch_query();
        self.shown_at = Instant::now();
        if !self.ui.is_null() {
            let (x, y, w, h) = self.compute_show_geometry();
            unsafe { tomoyo_linux_ui_show(self.ui, x, y, w, h); }
            self.layout_entry(w);
            self.invalidate();
        }
    }

    pub(super) fn toggle_launcher(&mut self) {
        if self.is_visible() { self.hide_launcher(); } else { self.show_launcher_with_query(None); }
    }

    pub(super) fn hide_launcher(&mut self) {
        self.flush_geometry_on_hide();
        self.action_menu_open = false;
        self.menu_open = false;
        self.menu_closed_at = None;
        self.menu_refocus = false;
        self.dragging_divider = false;
        self.scrollbar = Default::default();
        self.hovered = None;
        self.hovered_filter = None;
        self.hovered_logo = false;
        self.sidebar.drag = None;
        self.sidebar.hovered_add_tag = false;
        self.sidebar.modal_open = false;
        self.active_filter = SearchFilter::All;
        if !self.ui.is_null() {
            unsafe { tomoyo_linux_ui_hide(self.ui); }
        }
        self.schedule_hibernate_timer();
    }

    pub(super) fn layout_entry(&self, w_dip: i32) {
        if self.ui.is_null() { return; }
        let mut right_margin = 16;
        if self.active_mode_badge().is_some() { right_margin += 96; }
        if self.attached_capsule_label().is_some() { right_margin += 130; }
        let ew = (w_dip - 62 - right_margin).max(140);
        unsafe { tomoyo_linux_ui_set_entry_bounds(self.ui, 62, 14, ew, 34); }
    }

    pub(super) fn resize_to_fit(&mut self) {
        if !self.is_visible() { return; }
        let w = self.desired_width_dip();
        let h = self.desired_height_dip();
        self.last_applied_height_dip = h;
        unsafe { tomoyo_linux_ui_resize(self.ui, w, h); }
        self.layout_entry(w);
    }

    pub(super) fn set_query_text(&mut self, q: &str) {
        self.current_query = q.to_string();
        if !self.ui.is_null()
            && let Ok(cs) = CString::new(q)
        {
            unsafe { tomoyo_linux_ui_set_entry_text(self.ui, cs.as_ptr()); }
        }
        self.dispatch_query();
    }

    pub(super) fn invalidate(&self) {
        if !self.ui.is_null() { unsafe { tomoyo_linux_ui_queue_draw(self.ui); } }
    }

    pub(super) fn show_toast(&mut self, msg: &str) {
        self.toast = Some((msg.to_string(), Instant::now() + Duration::from_secs(3)));
        self.invalidate();
    }

    pub(super) fn run_hibernate(&mut self) -> u64 {
        self.cancel_hibernate_timer();
        self.preview_loader.purge_worker();
        self.renderer.hibernate_release();
        self.results.clear();
        self.results.shrink_to_fit();
        self.cached_all_results.clear();
        self.cached_all_results.shrink_to_fit();
        self.cached_all_query.clear();
        self.cached_preview_id = None;
        self.cached_preview = None;
        let cycles = self.router.hibernate_trim();
        trim_working_set();
        unsafe { mi_collect(true); }
        cycles
    }

    fn schedule_hibernate_timer(&mut self) {
        self.cancel_hibernate_timer();
        let secs = self.config.read().map(|c| c.hibernate_delay_secs).unwrap_or(5).max(1);
        unsafe extern "C" fn on_hib_timer(_d: *mut c_void) -> c_int {
            with_current_core(|st| {
                st.hibernate_source = 0;
                if !st.is_visible() { let _ = st.run_hibernate(); }
            });
            0
        }
        self.hibernate_source = unsafe { tomoyo_linux_add_timeout(secs * 1000, on_hib_timer, std::ptr::null_mut()) };
    }

    fn cancel_hibernate_timer(&mut self) {
        if self.hibernate_source != 0 {
            unsafe { tomoyo_linux_remove_source(self.hibernate_source); }
            self.hibernate_source = 0;
        }
    }

    pub(super) fn shutdown_clean(&mut self) {
        if self.shutting_down { return; }
        self.shutting_down = true;
        self.cancel_hibernate_timer();
        self.stop_window_geom();
        self.alive.store(false, Ordering::Release);
        self.hotkeys.unregister_all();
        unsafe { crate::linux::tomoyo_linux_super_panel_close(); }
        self.plugin_host.shutdown_all();
        self.preview_loader.cancel_all();
        if self.wl_clip_active {
            unsafe { tomoyo_wl_clip_stop(); }
            self.wl_clip_active = false;
        }
        if !self.ui.is_null() {
            unsafe { tomoyo_linux_ui_destroy(self.ui); }
            self.ui = std::ptr::null_mut();
        }
    }
}
