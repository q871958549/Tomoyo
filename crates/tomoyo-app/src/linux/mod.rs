//! Native Linux GTK3 + Cairo + Pango + KGlobalAccel shell backend for `tomoyo-app`.

mod hotkeys;
mod ipc;
mod render_pango;
mod shell_commands;
mod shell_events;
mod shell_menus;
mod shell_query;
mod shell_state;

pub use ipc::{wake_existing_instance, SingletonGuard};

use crate::brand::generate_icon_bgra;
use ipc::schedule_on_ui;
use shell_state::{with_current_core, AppCore};
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tomoyo_core::{QueryRouter, TomoyoConfig, UpdateManager};
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::AttachedContext;

#[repr(C)]
pub struct LinuxTrayMenuItem {
    pub label: *const c_char,
    pub cmd: u32,
    pub parent_idx: i32,
    pub checked: i32,
    pub disabled: i32,
}

unsafe extern "C" {
    pub fn mi_collect(force: bool);
    pub fn tomoyo_linux_ui_create(
        ctx: *mut c_void,
        on_draw: unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, c_int),
        on_key: unsafe extern "C" fn(*mut c_void, u32, u32) -> c_int,
        on_mouse: unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int),
        on_text_changed: unsafe extern "C" fn(*mut c_void),
        on_paste: unsafe extern "C" fn(*mut c_void),
        on_focus_out: unsafe extern "C" fn(*mut c_void),
        on_clip_change: unsafe extern "C" fn(*mut c_void),
        on_tray_cmd: unsafe extern "C" fn(*mut c_void, u32),
    ) -> *mut c_void;
    pub fn tomoyo_linux_ui_set_theme(a: *mut c_void, text_rgb: u32, bg_rgb: u32, font_px: c_int);
    pub fn tomoyo_linux_ui_begin_drag(a: *mut c_void);
    pub fn tomoyo_wl_clip_start(on_change: unsafe extern "C" fn());
    pub fn tomoyo_wl_clip_stop();
    pub fn tomoyo_linux_ui_set_entry_bounds(a: *mut c_void, x: c_int, y: c_int, w: c_int, h: c_int);
    pub fn tomoyo_linux_ui_set_entry_text(a: *mut c_void, txt: *const c_char);
    pub fn tomoyo_linux_ui_get_entry_text(a: *mut c_void) -> *const c_char;
    pub fn tomoyo_linux_ui_set_placeholder(a: *mut c_void, txt: *const c_char);
    pub fn tomoyo_linux_ui_show(a: *mut c_void, x: c_int, y: c_int, w: c_int, h: c_int);
    pub fn tomoyo_linux_ui_resize(a: *mut c_void, w: c_int, h: c_int);
    pub fn tomoyo_linux_ui_hide(a: *mut c_void);
    pub fn tomoyo_linux_ui_is_visible(a: *mut c_void) -> c_int;
    pub fn tomoyo_linux_ui_queue_draw(a: *mut c_void);
    pub fn tomoyo_linux_ui_screen_info(a: *mut c_void, ox: *mut c_int, oy: *mut c_int, ow: *mut c_int, oh: *mut c_int, oscale: *mut c_int);
    pub fn tomoyo_linux_tray_setup(a: *mut c_void, icon_path: *const c_char, hotkey_lbl: *const c_char);
    pub fn tomoyo_linux_tray_set_title(a: *mut c_void, title: *const c_char);
    pub fn tomoyo_linux_tray_set_menu(a: *mut c_void, items: *const LinuxTrayMenuItem, n: usize);
    pub fn tomoyo_linux_ui_focus_entry(a: *mut c_void);
    pub fn tomoyo_linux_ui_is_active(a: *mut c_void) -> c_int;
    pub fn tomoyo_linux_entry_has_selection(a: *mut c_void) -> c_int;
    pub fn tomoyo_linux_entry_edit_cmd(a: *mut c_void, op: c_int);
    pub fn tomoyo_linux_schedule_idle(fn_cb: unsafe extern "C" fn(*mut c_void), data: *mut c_void);
    pub fn tomoyo_linux_add_timeout(ms: u32, cb: unsafe extern "C" fn(*mut c_void) -> c_int, data: *mut c_void) -> u32;
    pub fn tomoyo_linux_remove_source(id: u32);
    pub fn tomoyo_linux_ui_run();
    pub fn tomoyo_linux_ui_quit();
    pub fn tomoyo_linux_ui_destroy(a: *mut c_void);
}

pub struct AppShell {
    core: Rc<RefCell<AppCore>>,
}

impl AppShell {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        config: Arc<RwLock<TomoyoConfig>>,
        router: Arc<QueryRouter>,
        plugin_host: Arc<PluginHost>,
        updater: Arc<UpdateManager>,
        font_regular: &'static str,
        _font_medium: &'static str,
        install_tray: bool,
    ) -> Result<Box<Self>, String> {
        let core = AppCore::create_rc(
            config,
            router,
            plugin_host,
            updater,
            font_regular,
            install_tray,
        )?;
        Ok(Box::new(Self { core }))
    }

    pub fn schedule_startup_update_check(&self) {
        let (updater, config) = {
            let c = self.core.borrow();
            (Arc::clone(&c.updater), Arc::clone(&c.config))
        };
        updater.check_async_if_due_with_notify(&config, || {
            schedule_on_ui(|| with_current_core(|st| st.on_update_ready()));
        });
    }

    pub fn hotkey_label(&self) -> String {
        self.core.borrow().hotkeys.active_label.clone()
    }

    pub fn show_launcher(&mut self) {
        self.core.borrow_mut().show_launcher_with_query(None);
    }

    pub fn show_toast(&mut self, msg: &str) {
        self.core.borrow_mut().show_toast(msg);
    }

    pub fn apply_language(&mut self, code: &str, persist: bool) {
        self.core.borrow_mut().apply_language(code, persist);
    }

    pub fn apply_theme(&mut self, name: &str, persist: bool) {
        self.core.borrow_mut().apply_theme(name, persist);
    }

    pub fn run_hibernate(&mut self) -> u64 {
        self.core.borrow_mut().run_hibernate()
    }

    pub fn is_renderer_hibernated(&self) -> bool {
        self.core.borrow().renderer.is_hibernated()
    }

    pub fn wait_background_idle_for_test(&mut self) {
        let mut spins = 0u32;
        while self.core.borrow().query_in_flight.load(Ordering::Acquire) > 0 && spins < 200 {
            std::thread::sleep(Duration::from_millis(2));
            spins += 1;
        }
        self.core.borrow().preview_loader.wait_idle_and_drain();
    }

    pub fn self_test_paint_and_hibernate(&mut self, query: &str) -> usize {
        self.core.borrow_mut().self_test_paint(query)
    }

    pub fn self_test_attached_paint(&mut self, ctx: AttachedContext) -> usize {
        let mut c = self.core.borrow_mut();
        c.attached = Some(ctx);
        let hits = c.self_test_paint("");
        c.attached = None;
        hits
    }

    pub fn run_message_loop(&mut self) {
        unsafe { tomoyo_linux_ui_run(); }
        self.core.borrow_mut().shutdown_clean();
    }
}

pub(super) fn ensure_tray_icon_png() -> Option<PathBuf> {
    unsafe extern "C" { fn getuid() -> u32; }
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("tomoyo-tray-{}.png", unsafe { getuid() }));
    if path.is_file() {
        return Some(path);
    }
    let bgra = generate_icon_bgra(64);
    let mut rgba = vec![0u8; bgra.len()];
    for (s, d) in bgra.as_chunks::<4>().0.iter().zip(rgba.as_chunks_mut::<4>().0.iter_mut()) {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
    let file = std::fs::File::create(&path).ok()?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 64, 64);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().ok()?.write_image_data(&rgba).ok()?;
    Some(path)
}

/// Disables Linux Transparent Huge Pages so mimalloc purges 4 KiB pages promptly when idle.
pub(crate) fn disable_transparent_hugepages() {
    const PR_SET_THP_DISABLE: std::ffi::c_int = 41;
    unsafe extern "C" {
        fn prctl(
            option: std::ffi::c_int,
            arg2: std::ffi::c_ulong,
            arg3: std::ffi::c_ulong,
            arg4: std::ffi::c_ulong,
            arg5: std::ffi::c_ulong,
        ) -> std::ffi::c_int;
    }
    unsafe {
        let _ = prctl(PR_SET_THP_DISABLE, 1, 0, 0, 0);
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{c_char, c_int, c_void, CString};

    unsafe extern "C" {
        fn tomoyo_linux_load_icon_bgra(
            icon_name_or_path: *const c_char,
            sz: c_int,
            out: *mut u8,
            out_len: usize,
        ) -> c_int;
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
    }

    #[test]
    fn renders_svg_icon_via_librsvg_without_spawning_glycin_threads() {
        let svg = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/tomoyo-icon.svg");
        assert!(svg.is_file(), "missing test SVG: {}", svg.display());
        let cs = CString::new(svg.to_string_lossy().as_bytes()).unwrap();
        let mut buf = vec![0u8; 32 * 32 * 4];
        let ok = unsafe { tomoyo_linux_load_icon_bgra(cs.as_ptr(), 32, buf.as_mut_ptr(), buf.len()) };
        const RTLD_LAZY: c_int = 1;
        const RTLD_NOLOAD: c_int = 4;
        let rsvg_handle = unsafe { dlopen(c"librsvg-2.so.2".as_ptr(), RTLD_LAZY | RTLD_NOLOAD) };
        let has_rsvg = !rsvg_handle.is_null();
        if has_rsvg {
            unsafe { let _ = dlclose(rsvg_handle); }
            assert_eq!(ok, 1, "librsvg loaded but SVG icon render failed");
            assert!(buf.as_chunks::<4>().0.iter().any(|p| p[3] > 0), "rendered SVG buffer is empty");
            if let Ok(tasks) = std::fs::read_dir("/proc/self/task") {
                for ent in tasks.flatten() {
                    let comm = std::fs::read_to_string(ent.path().join("comm")).unwrap_or_default();
                    let name = comm.trim();
                    assert!(
                        name != "gly-global-exec" && name != "async-io",
                        "unexpected glycin thread `{name}` spawned during SVG render"
                    );
                }
            }
        }
    }
}
