//! GTK main-loop idle bridge & background worker dispatch for the Linux WebView host.

use crate::bridge::HostAction;
use crate::linux_hotkeys::{audit_system_hotkeys, probe_hotkey_owner_live};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tomoyo_core::{
    download_and_verify_release_asset_with_progress, fetch_latest_github_release,
    launch_verified_update_installer, set_sync_dir, validate_scan_dir, ReleaseInfo, TomoyoConfig,
    UpdateStatus, CURRENT_VERSION,
};

unsafe extern "C" {
    pub fn tomoyo_wv_eval_js(h: *mut c_void, js: *const c_char);
    pub fn tomoyo_wv_apply_theme(h: *mut c_void, mode: c_int);
    pub fn tomoyo_wv_present(h: *mut c_void);
    pub fn tomoyo_wv_pick_folder(h: *mut c_void, title: *const c_char) -> *mut c_char;
    pub fn tomoyo_wv_pick_file(h: *mut c_void, title: *const c_char) -> *mut c_char;
    pub fn tomoyo_wv_free_str(s: *mut c_char);
    pub fn tomoyo_wv_schedule_idle(cb: unsafe extern "C" fn(*mut c_void), data: *mut c_void);
}

/// Thread-safe relay from worker threads to WebKitGTK's main thread.
pub struct HostChannel {
    pub wv_ptr: AtomicUsize,
    pub alive: AtomicBool,
    pub notify_hwnd: Option<isize>,
    pub plugin_id: String,
    pub desired_theme: AtomicI32,
    tx: mpsc::Sender<String>,
    rx: Mutex<mpsc::Receiver<String>>,
}

impl HostChannel {
    pub fn new(notify_hwnd: Option<isize>, plugin_id: String) -> Arc<Self> {
        let (tx, rx) = mpsc::channel();
        Arc::new(Self {
            wv_ptr: AtomicUsize::new(0),
            alive: AtomicBool::new(true),
            notify_hwnd,
            plugin_id,
            desired_theme: AtomicI32::new(-1),
            tx,
            rx: Mutex::new(rx),
        })
    }

    pub fn push(self: &Arc<Self>, event_json: String) {
        if !self.alive.load(Ordering::SeqCst) {
            return;
        }
        if self.tx.send(event_json).is_ok() {
            let ptr = Arc::into_raw(Arc::clone(self)) as *mut c_void;
            unsafe { tomoyo_wv_schedule_idle(drain_idle_cb, ptr); }
        }
    }

    pub fn request_present(self: &Arc<Self>) {
        if !self.alive.load(Ordering::SeqCst) {
            return;
        }
        let ptr = Arc::into_raw(Arc::clone(self)) as *mut c_void;
        unsafe { tomoyo_wv_schedule_idle(present_idle_cb, ptr); }
    }

    pub fn schedule_folder_picker(self: &Arc<Self>, mode: u8) {
        if !self.alive.load(Ordering::SeqCst) {
            return;
        }
        let job = Box::new((Arc::clone(self), mode));
        unsafe { tomoyo_wv_schedule_idle(pick_folder_idle_cb, Box::into_raw(job) as *mut c_void); }
    }

    pub fn schedule_plugin_path_picker(self: &Arc<Self>, plugin_id: String, key: String, is_file: bool) {
        if !self.alive.load(Ordering::SeqCst) {
            return;
        }
        let job = Box::new((Arc::clone(self), plugin_id, key, is_file));
        unsafe { tomoyo_wv_schedule_idle(pick_plugin_path_idle_cb, Box::into_raw(job) as *mut c_void); }
    }
}

pub fn eval_dispatch_json(wv: *mut c_void, json: &str) {
    if wv.is_null() {
        return;
    }
    if let Some(q_json) = json.strip_prefix("__TOMOYO_REACTIVATE__:") {
        let script = format!("window.tomoyo && window.tomoyo.__reactivate && window.tomoyo.__reactivate({q_json});");
        if let Ok(cs) = CString::new(script) {
            unsafe { tomoyo_wv_eval_js(wv, cs.as_ptr()); }
        }
        return;
    }
    if let Some(r_json) = json.strip_prefix("__TOMOYO_NAVIGATE_ROUTE__:") {
        let script = format!("window.__TOMOYO_NAVIGATE_ROUTE__ && window.__TOMOYO_NAVIGATE_ROUTE__({r_json});");
        if let Ok(cs) = CString::new(script) {
            unsafe { tomoyo_wv_eval_js(wv, cs.as_ptr()); }
        }
        return;
    }
    let quoted = serde_json::to_string(json).unwrap_or_else(|_| "\"{}\"".into());
    let script = format!("window.__tomoyoDispatchHostEvent && window.__tomoyoDispatchHostEvent({quoted});");
    if let Ok(cs) = CString::new(script) {
        unsafe { tomoyo_wv_eval_js(wv, cs.as_ptr()); }
    }
}

unsafe extern "C" fn drain_idle_cb(ud: *mut c_void) {
    let ch = unsafe { Arc::from_raw(ud as *const HostChannel) };
    if !ch.alive.load(Ordering::SeqCst) {
        return;
    }
    let wv = ch.wv_ptr.load(Ordering::SeqCst) as *mut c_void;
    if let Ok(rx) = ch.rx.lock() {
        while let Ok(evt) = rx.try_recv() {
            eval_dispatch_json(wv, &evt);
        }
    }
}

unsafe extern "C" fn present_idle_cb(ud: *mut c_void) {
    let ch = unsafe { Arc::from_raw(ud as *const HostChannel) };
    if !ch.alive.load(Ordering::SeqCst) {
        return;
    }
    let wv = ch.wv_ptr.load(Ordering::SeqCst) as *mut c_void;
    if let Ok(rx) = ch.rx.lock() {
        while let Ok(evt) = rx.try_recv() {
            eval_dispatch_json(wv, &evt);
        }
    }
    let theme = ch.desired_theme.swap(-1, Ordering::SeqCst);
    if !wv.is_null() {
        if theme >= 0 {
            unsafe { tomoyo_wv_apply_theme(wv, theme); }
        }
        unsafe { tomoyo_wv_present(wv); }
    }
}

unsafe extern "C" fn pick_folder_idle_cb(ud: *mut c_void) {
    let boxed = unsafe { Box::from_raw(ud as *mut (Arc<HostChannel>, u8)) };
    let (ch, mode) = *boxed;
    if !ch.alive.load(Ordering::SeqCst) {
        return;
    }
    let wv = ch.wv_ptr.load(Ordering::SeqCst) as *mut c_void;
    if wv.is_null() {
        return;
    }
    let title = match mode {
        3 => "选择本地同步目录（Syncthing / Nextcloud / Git 等）",
        2 => "选择截图默认保存目录",
        _ => "选择自定义应用或便携软件目录",
    };
    let c_title = CString::new(title).unwrap();
    let raw = unsafe { tomoyo_wv_pick_folder(wv, c_title.as_ptr()) };
    let chosen = if raw.is_null() {
        None
    } else {
        let s = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
        unsafe { tomoyo_wv_free_str(raw); }
        Some(s)
    };
    let reply = match mode {
        3 => match chosen {
            Some(p) => match set_sync_dir(Some(&p)) {
                Ok((dir, message)) => {
                    let cfg = TomoyoConfig::load_or_create();
                    let sync_str = dir.map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Some(t) = ch.notify_hwnd {
                        crate::bridge_os::notify_parent_mode(t, 1);
                    }
                    serde_json::json!({
                        "event": "sync_dir_updated", "ok": true, "cancelled": false,
                        "sync_dir": sync_str, "config": cfg, "message": message,
                        "config_path": TomoyoConfig::config_path().to_string_lossy(),
                    }).to_string()
                }
                Err(err) => serde_json::json!({
                    "event": "sync_dir_updated", "ok": false, "cancelled": false, "message": err
                }).to_string(),
            },
            None => serde_json::json!({ "event": "sync_dir_updated", "ok": false, "cancelled": true }).to_string(),
        },
        _ => match chosen {
            Some(p) => match validate_scan_dir(&p) {
                Ok(clean) => serde_json::json!({
                    "event": "folder_picked", "ok": true, "cancelled": false, "path": clean
                }).to_string(),
                Err(err) => serde_json::json!({
                    "event": "folder_picked", "ok": false, "cancelled": false, "path": p, "error": err
                }).to_string(),
            },
            None => serde_json::json!({ "event": "folder_picked", "ok": false, "cancelled": true }).to_string(),
        },
    };
    eval_dispatch_json(wv, &reply);
}

unsafe extern "C" fn pick_plugin_path_idle_cb(ud: *mut c_void) {
    let boxed = unsafe { Box::from_raw(ud as *mut (Arc<HostChannel>, String, String, bool)) };
    let (ch, plugin_id, key, is_file) = *boxed;
    if !ch.alive.load(Ordering::SeqCst) {
        return;
    }
    let wv = ch.wv_ptr.load(Ordering::SeqCst) as *mut c_void;
    if wv.is_null() {
        return;
    }
    let title = if is_file { "选择插件所需文件" } else { "选择插件所需目录" };
    let c_title = CString::new(title).unwrap();
    let raw = unsafe {
        if is_file {
            tomoyo_wv_pick_file(wv, c_title.as_ptr())
        } else {
            tomoyo_wv_pick_folder(wv, c_title.as_ptr())
        }
    };
    let reply = if raw.is_null() {
        serde_json::json!({
            "event": "plugin_path_picked", "plugin_id": plugin_id, "key": key,
            "ok": false, "cancelled": true
        }).to_string()
    } else {
        let p = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
        unsafe { tomoyo_wv_free_str(raw); }
        serde_json::json!({
            "event": "plugin_path_picked", "plugin_id": plugin_id, "key": key,
            "ok": true, "cancelled": false, "path": p
        }).to_string()
    };
    eval_dispatch_json(wv, &reply);
}

pub fn spawn_hotkey_health_worker(ch: Arc<HostChannel>) {
    let _ = std::thread::Builder::new().name("tomoyo-hk-health".into()).spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(180));
        ch.push(serde_json::json!({
            "event": "hotkey_health",
            "health": tomoyo_core::load_hotkey_health(),
        }).to_string());
    });
}

fn spawn_update_worker(repo: String, ch: Arc<HostChannel>) {
    let _ = std::thread::Builder::new().name("tomoyo-wv-updater".into()).spawn(move || {
        let st = fetch_latest_github_release(&repo, CURRENT_VERSION);
        if matches!(st, UpdateStatus::UpToDate { .. } | UpdateStatus::Available { .. }) {
            let mut cfg = TomoyoConfig::load_or_create();
            cfg.update.last_checked_unix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let _ = cfg.save();
        }
        ch.push(serde_json::json!({ "event": "update_status", "status": st }).to_string());
    });
}

fn spawn_install_update_worker(rel: ReleaseInfo, ch: Arc<HostChannel>) {
    let _ = std::thread::Builder::new().name("tomoyo-wv-installer".into()).spawn(move || {
        let mut last_pct: i16 = -1;
        let mut last_stage = String::new();
        let res = download_and_verify_release_asset_with_progress(&rel, |prog| {
            if prog.percent as i16 == last_pct && prog.stage == last_stage {
                return;
            }
            last_pct = prog.percent as i16;
            last_stage = prog.stage.clone();
            ch.push(serde_json::json!({ "event": "update_install_progress", "progress": prog }).to_string());
        }).and_then(|p| launch_verified_update_installer(&p));
        let launched = matches!(res, Ok(true));
        let evt = match res {
            Ok(started) => serde_json::json!({
                "event": "update_install_result", "ok": true, "launched_installer": started,
                "message": if started { "SHA-256 校验通过，正在后台安装新版本并自动重启 Tomoyo..." }
                           else { "SHA-256 校验通过，已在文件管理器中定位安装包" }
            }),
            Err(e) => serde_json::json!({
                "event": "update_install_result", "ok": false, "launched_installer": false, "message": e
            }),
        }.to_string();
        ch.push(evt);
        if launched && let Some(h) = ch.notify_hwnd {
            std::thread::sleep(std::time::Duration::from_millis(250));
            crate::bridge_os::notify_parent_mode(h, 5);
        }
    });
}

pub fn dispatch_async_workers(act: HostAction, ch: &Arc<HostChannel>) {
    if let Some(repo) = act.async_check_updates {
        spawn_update_worker(repo, Arc::clone(ch));
    }
    if let Some(rel) = act.async_install_update {
        spawn_install_update_worker(rel, Arc::clone(ch));
    }
    if let Some(req) = act.async_fetch_store {
        let c = Arc::clone(ch);
        let _ = std::thread::Builder::new().name("tomoyo-wv-store".into()).spawn(move || {
            c.push(crate::store_index::query_store_page_json(&req));
        });
    }
    if let Some((plugin_id, sources)) = act.async_fetch_plugin_detail {
        let c = Arc::clone(ch);
        let _ = std::thread::Builder::new().name("tomoyo-store-detail".into()).spawn(move || {
            c.push(crate::store_index::query_plugin_detail_json(&plugin_id, &sources));
        });
    }
    if let Some(sources) = act.async_check_plugin_health {
        let c = Arc::clone(ch);
        let _ = std::thread::Builder::new().name("tomoyo-plugin-health".into()).spawn(move || {
            c.push(crate::store_health::run_plugin_health_check_json(&sources));
        });
    }
    if act.async_refresh_hotkey_health {
        spawn_hotkey_health_worker(Arc::clone(ch));
    }
    if act.async_audit_hotkeys {
        let c = Arc::clone(ch);
        let _ = std::thread::Builder::new().name("tomoyo-hk-audit".into()).spawn(move || {
            let report = audit_system_hotkeys();
            c.push(serde_json::json!({ "event": "hotkey_audit", "report": report }).to_string());
            std::thread::sleep(std::time::Duration::from_millis(160));
            c.push(serde_json::json!({ "event": "hotkey_health", "health": tomoyo_core::load_hotkey_health() }).to_string());
        });
    }
    if let Some(spec) = act.async_probe_hotkey {
        let c = Arc::clone(ch);
        let _ = std::thread::Builder::new().name("tomoyo-hk-probe".into()).spawn(move || {
            let result = probe_hotkey_owner_live(&spec);
            c.push(serde_json::json!({ "event": "hotkey_probe_result", "result": result }).to_string());
            std::thread::sleep(std::time::Duration::from_millis(160));
            c.push(serde_json::json!({ "event": "hotkey_health", "health": tomoyo_core::load_hotkey_health() }).to_string());
        });
    }
    if let Some((req_id, method, params)) = act.async_plugin_rpc {
        let c = Arc::clone(ch);
        let pid = ch.plugin_id.clone();
        let _ = std::thread::Builder::new().name("tomoyo-plugin-rpc".into()).spawn(move || {
            c.push(crate::plugin_rpc::execute_plugin_rpc(&pid, &req_id, &method, params));
        });
    }
}

pub struct SocketGuard(pub Option<PathBuf>);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn try_acquire_instance(
    mode_tag: &str,
    plugin_query: Option<&str>,
    settings_route: Option<&str>,
    ch: &Arc<HostChannel>,
) -> Result<SocketGuard, ()> {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("tomoyo-wv-{}-{mode_tag}.sock", unsafe { getuid() }));
    if let Ok(mut stream) = UnixStream::connect(&path) {
        if let Some(q) = plugin_query {
            let jq = serde_json::to_string(q).unwrap_or_else(|_| "\"\"".into());
            let _ = stream.write_all(format!("activate_plugin:{jq}\n").as_bytes());
        } else if let Some(r) = settings_route.map(str::trim).filter(|s| !s.is_empty()) {
            let jr = serde_json::to_string(r).unwrap_or_else(|_| "\"\"".into());
            let _ = stream.write_all(format!("navigate_route:{jr}\n").as_bytes());
        } else {
            let _ = stream.write_all(b"activate\n");
        }
        return Err(());
    }
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else {
        return Ok(SocketGuard(None));
    };
    let c = Arc::clone(ch);
    let _ = std::thread::Builder::new().name("tomoyo-wv-ipc".into()).spawn(move || {
        for conn in listener.incoming() {
            if !c.alive.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(stream) = conn {
                let mut cmd = String::new();
                let _ = BufReader::new(stream).read_line(&mut cmd);
                let t = TomoyoConfig::load_or_create().theme;
                let mode = if t.eq_ignore_ascii_case("light") { 0 } else if t.eq_ignore_ascii_case("midnight") { 2 } else { 1 };
                c.desired_theme.store(mode, Ordering::SeqCst);
                let trimmed = cmd.trim();
                if let Some(q_json) = trimmed.strip_prefix("activate_plugin:") {
                    c.push(format!("__TOMOYO_REACTIVATE__:{q_json}"));
                } else if let Some(r_json) = trimmed.strip_prefix("navigate_route:") {
                    c.push(format!("__TOMOYO_NAVIGATE_ROUTE__:{r_json}"));
                }
                c.request_present();
            }
        }
    });
    Ok(SocketGuard(Some(path)))
}
