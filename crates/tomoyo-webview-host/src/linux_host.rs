//! Native Linux GTK3 + WebKitGTK 4.1 container window, single-instance Unix socket,
//! custom image URI schemes, hotkey capture & async worker relay.

use crate::bridge::handle_host_message;
use crate::clip_view::{assemble_clip_manager_html, is_clip_manager_entry, load_clip_manager_bootstrap};
use crate::db::PluginDb;
use crate::linux_hotkeys::{format_gdk_hotkey, set_global_shortcuts_blocked};
use crate::linux_workers::{
    dispatch_async_workers, eval_dispatch_json, spawn_hotkey_health_worker, try_acquire_instance,
    HostChannel,
};
use crate::ocr_view::{assemble_ocr_viewer_html, is_ocr_viewer_entry, load_ocr_viewer_bootstrap};
use crate::shim_js::build_init_script;
use crate::tr_view::{
    assemble_translator_html, is_translator_entry, load_translator_bootstrap,
};
use crate::{assemble_settings_html, build_settings_init_script_for, LaunchArgs};
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const EMBEDDED_ICON_PNG: &[u8] = include_bytes!("../../../frontend/src/assets/tomoyo-logo.png");

type WvMsgCb = unsafe extern "C" fn(*mut c_void, *const c_char);
type WvKeyCb = unsafe extern "C" fn(*mut c_void, u32, u32) -> c_int;
type WvUriCb = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut *mut u8, *mut usize) -> c_int;
type WvVoidCb = unsafe extern "C" fn(*mut c_void);

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn tomoyo_wv_apply_theme(h: *mut c_void, mode: c_int);
    fn tomoyo_wv_create(
        title: *const c_char,
        width: c_int,
        height: c_int,
        mode: c_int,
        icon_rgba: *const u8,
        icon_w: c_int,
        icon_h: c_int,
        extra_init_js: *const c_char,
        on_msg: WvMsgCb,
        on_key: WvKeyCb,
        on_uri: WvUriCb,
        on_focus_in: WvVoidCb,
        on_close: WvVoidCb,
        ctx: *mut c_void,
    ) -> *mut c_void;
    fn tomoyo_wv_load_html(h: *mut c_void, html: *const c_char);
    fn tomoyo_wv_load_uri(h: *mut c_void, uri: *const c_char);
    fn tomoyo_wv_show_and_run(h: *mut c_void);
    fn tomoyo_wv_close(h: *mut c_void);
    fn tomoyo_wv_toggle_topmost(h: *mut c_void) -> c_int;
    fn tomoyo_wv_destroy(h: *mut c_void);
}

struct LinuxHostCtx {
    wv: *mut c_void,
    channel: Arc<HostChannel>,
    db: RefCell<PluginDb>,
    notify_hwnd: Option<isize>,
    settings_mode: bool,
    capturing_hotkey: AtomicBool,
    last_activate_probe: RefCell<Instant>,
    ocr_dir: Option<PathBuf>,
    clip_dir: Option<PathBuf>,
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((h << 4) | l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn theme_mode(name: &str) -> c_int {
    if name.eq_ignore_ascii_case("light") {
        0
    } else if name.eq_ignore_ascii_case("midnight") {
        2
    } else {
        1
    }
}

unsafe extern "C" fn on_msg_cb(ctx: *mut c_void, json_ptr: *const c_char) {
    if ctx.is_null() || json_ptr.is_null() {
        return;
    }
    let st = unsafe { &*(ctx as *const LinuxHostCtx) };
    let msg = unsafe { CStr::from_ptr(json_ptr) }.to_string_lossy();
    let act = handle_host_message(&msg, &mut st.db.borrow_mut(), st.notify_hwnd);
    if let Some(theme) = &act.apply_theme {
        unsafe { tomoyo_wv_apply_theme(st.wv, theme_mode(theme)); }
    }
    if act.toggle_topmost {
        unsafe { tomoyo_wv_toggle_topmost(st.wv); }
    }
    if act.pick_folder_mode != 0 {
        st.channel.schedule_folder_picker(act.pick_folder_mode);
    }
    if let Some(reply) = &act.reply_json {
        eval_dispatch_json(st.wv, reply);
    }
    if act.hotkey_capture_cmd > 0 {
        st.capturing_hotkey.store(true, Ordering::SeqCst);
        set_global_shortcuts_blocked(true);
        if let Some(p) = st.notify_hwnd {
            crate::bridge_os::notify_parent_mode(p, 3);
        }
    } else if act.hotkey_capture_cmd < 0 {
        st.capturing_hotkey.store(false, Ordering::SeqCst);
        set_global_shortcuts_blocked(false);
        if let Some(p) = st.notify_hwnd {
            crate::bridge_os::notify_parent_mode(p, 4);
        }
    }
    let should_close = act.should_close;
    dispatch_async_workers(act, &st.channel);
    if should_close {
        unsafe { tomoyo_wv_close(st.wv); }
    }
}

fn is_modifier_gdk_keyval(kv: u32) -> bool {
    matches!(
        kv,
        0xFFE1 | 0xFFE2 | 0xFFE3 | 0xFFE4 | 0xFFE7 | 0xFFE8 | 0xFFE9 | 0xFFEA | 0xFFEB | 0xFFEC | 0xFE03
    )
}

unsafe extern "C" fn on_key_cb(ctx: *mut c_void, keyval: u32, state: u32) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    let st = unsafe { &*(ctx as *const LinuxHostCtx) };
    if !st.capturing_hotkey.load(Ordering::SeqCst) {
        return 0;
    }
    if is_modifier_gdk_keyval(keyval) {
        return 1;
    }
    if keyval == 0xFF1B {
        st.capturing_hotkey.store(false, Ordering::SeqCst);
        set_global_shortcuts_blocked(false);
        if let Some(p) = st.notify_hwnd {
            crate::bridge_os::notify_parent_mode(p, 4);
        }
        eval_dispatch_json(st.wv, &serde_json::json!({ "event": "hotkey_captured", "cancelled": true }).to_string());
        return 1;
    }
    if let Some(combo) = format_gdk_hotkey(keyval, state) {
        st.capturing_hotkey.store(false, Ordering::SeqCst);
        set_global_shortcuts_blocked(false);
        if let Some(p) = st.notify_hwnd {
            crate::bridge_os::notify_parent_mode(p, 4);
        }
        eval_dispatch_json(st.wv, &serde_json::json!({ "event": "hotkey_captured", "combo": combo }).to_string());
        return 1;
    }
    eval_dispatch_json(st.wv, &serde_json::json!({ "event": "hotkey_captured", "need_mod": true }).to_string());
    1
}

unsafe extern "C" fn on_uri_cb(
    ctx: *mut c_void,
    scheme_ptr: *const c_char,
    path_ptr: *const c_char,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> c_int {
    if ctx.is_null() || scheme_ptr.is_null() || path_ptr.is_null() {
        return 0;
    }
    let st = unsafe { &*(ctx as *const LinuxHostCtx) };
    let scheme = unsafe { CStr::from_ptr(scheme_ptr) }.to_string_lossy();
    let raw_path = unsafe { CStr::from_ptr(path_ptr) }.to_string_lossy();
    let base_dir = match scheme.as_ref() {
        "tomoyo-clip-img" => st.clip_dir.as_ref(),
        "tomoyo-ocr-img" => st.ocr_dir.as_ref(),
        _ => None,
    };
    let Some(dir) = base_dir else { return 0; };
    let decoded = percent_decode(raw_path.trim_start_matches('/'));
    let Some(fname) = std::path::Path::new(&decoded).file_name() else { return 0; };
    let full = dir.join(fname);
    let Ok(bytes) = std::fs::read(&full) else { return 0; };
    if bytes.is_empty() {
        return 0;
    }
    let buf = unsafe { malloc(bytes.len()) }.cast::<u8>();
    if buf.is_null() {
        return 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        *out_data = buf;
        *out_len = bytes.len();
    }
    1
}

unsafe extern "C" fn on_focus_in_cb(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let st = unsafe { &*(ctx as *const LinuxHostCtx) };
    if !st.settings_mode || st.capturing_hotkey.load(Ordering::SeqCst) {
        return;
    }
    let mut last = st.last_activate_probe.borrow_mut();
    if last.elapsed() < Duration::from_millis(1200) {
        return;
    }
    *last = Instant::now();
    if tomoyo_core::load_hotkey_health().has_unresolved_conflicts()
        && let Some(p) = st.notify_hwnd
    {
        crate::bridge_os::notify_parent_mode(p, 4);
    }
    spawn_hotkey_health_worker(Arc::clone(&st.channel));
}

unsafe extern "C" fn on_close_cb(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let st = unsafe { &*(ctx as *const LinuxHostCtx) };
    if st.capturing_hotkey.swap(false, Ordering::SeqCst) {
        set_global_shortcuts_blocked(false);
        if let Some(p) = st.notify_hwnd {
            crate::bridge_os::notify_parent_mode(p, 4);
        }
    }
    st.channel.alive.store(false, Ordering::SeqCst);
    st.channel.wv_ptr.store(0, Ordering::SeqCst);
}

pub fn run_linux_host(opts: LaunchArgs) {
    let clip_mode = is_clip_manager_entry(&opts.entry);
    let tr_mode = is_translator_entry(&opts.entry);
    let ocr_mode = is_ocr_viewer_entry(&opts.entry);
    let channel = HostChannel::new(opts.notify_hwnd);

    let plugin_tag;
    let mode_tag = if opts.settings_mode {
        Some("settings")
    } else if clip_mode {
        Some("clipboard")
    } else if tr_mode {
        Some("translate")
    } else if !opts.self_test && !ocr_mode && !opts.plugin_id.is_empty() {
        plugin_tag = format!("plugin-{}", tomoyo_core::linux_launch::sanitize_instance_tag(&opts.plugin_id));
        Some(plugin_tag.as_str())
    } else {
        None
    };
    let plugin_query = (!opts.settings_mode && !clip_mode && !tr_mode && !ocr_mode)
        .then_some(opts.query.as_str());
    let _sock_guard = match mode_tag {
        Some(tag) => match try_acquire_instance(tag, tr_mode, clip_mode, plugin_query, &channel) {
            Ok(g) => Some(g),
            Err(()) => return,
        },
        None => None,
    };

    let db = PluginDb::open(&opts.plugin_id);
    let (init_script, ocr_dir, clip_dir) = if opts.settings_mode {
        (build_settings_init_script_for(&opts.query), None, None)
    } else if ocr_mode {
        let b = load_ocr_viewer_bootstrap(&opts.theme, &opts.query);
        (b.init_script, b.image_dir, None)
    } else if clip_mode {
        let b = load_clip_manager_bootstrap(&opts.theme, &opts.query);
        (b.init_script, None, Some(b.images_dir))
    } else if tr_mode {
        (load_translator_bootstrap(&opts.theme), None, None)
    } else {
        (build_init_script(&opts.plugin_id, &opts.query, opts.theme != "light", opts.utools_compat, &db.snapshot_json()), None, None)
    };

    let mut ctx = Box::new(LinuxHostCtx {
        wv: std::ptr::null_mut(),
        channel: Arc::clone(&channel),
        db: RefCell::new(db),
        notify_hwnd: opts.notify_hwnd,
        settings_mode: opts.settings_mode,
        capturing_hotkey: AtomicBool::new(false),
        last_activate_probe: RefCell::new(Instant::now()),
        ocr_dir,
        clip_dir,
    });
    let ctx_ptr = &mut *ctx as *mut LinuxHostCtx;
    let c_title = CString::new(opts.title.as_str()).unwrap_or_else(|_| CString::new("Tomoyo").unwrap());
    let c_init = CString::new(init_script).unwrap_or_default();
    let mode = theme_mode(&opts.theme);

    let icon_decoded = tomoyo_core::wic_thumb::decode_png_rgba_bytes(EMBEDDED_ICON_PNG);
    let (icon_ptr, icon_w, icon_h) = match &icon_decoded {
        Some((w, h, rgba)) => (rgba.as_ptr(), *w as c_int, *h as c_int),
        None => (std::ptr::null(), 0, 0),
    };
    let wv = unsafe {
        tomoyo_wv_create(
            c_title.as_ptr(), opts.width, opts.height, mode,
            icon_ptr, icon_w, icon_h,
            c_init.as_ptr(), on_msg_cb, on_key_cb, on_uri_cb, on_focus_in_cb, on_close_cb,
            ctx_ptr as *mut c_void,
        )
    };
    if wv.is_null() {
        tracing::error!("Failed to initialize GTK3/WebKitGTK WebView host window");
        return;
    }
    ctx.wv = wv;
    channel.wv_ptr.store(wv as usize, Ordering::SeqCst);

    if opts.settings_mode || ocr_mode || clip_mode || tr_mode {
        let html = if opts.settings_mode {
            assemble_settings_html()
        } else if ocr_mode {
            assemble_ocr_viewer_html()
        } else if clip_mode {
            assemble_clip_manager_html()
        } else {
            assemble_translator_html()
        };
        if let Ok(ch) = CString::new(html) {
            unsafe { tomoyo_wv_load_html(wv, ch.as_ptr()); }
        }
    } else {
        let uri = if opts.entry.starts_with("http://") || opts.entry.starts_with("https://") {
            opts.entry.clone()
        } else if let Ok(abs) = std::fs::canonicalize(&opts.entry) {
            format!("file://{}", abs.to_string_lossy())
        } else {
            format!("file://{}", opts.entry)
        };
        if let Ok(cu) = CString::new(uri) {
            unsafe { tomoyo_wv_load_uri(wv, cu.as_ptr()); }
        }
    }

    unsafe {
        tomoyo_wv_show_and_run(wv);
        channel.alive.store(false, Ordering::SeqCst);
        channel.wv_ptr.store(0, Ordering::SeqCst);
        tomoyo_wv_destroy(wv);
    }
    drop(ctx);
}
