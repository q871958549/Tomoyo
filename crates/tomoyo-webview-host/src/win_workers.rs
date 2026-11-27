//! Background threads for Settings Center updater checks, installer execution & Store loading.

use crate::store_index::{query_plugin_detail_json, query_store_page_json, StoreQuery};
use std::ffi::c_void;
use std::sync::mpsc;
use tomoyo_core::{
    download_and_verify_release_asset_with_progress, fetch_latest_github_release,
    launch_verified_update_installer, ReleaseInfo, TomoyoConfig, UpdateStatus, CURRENT_VERSION,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub fn spawn_update_worker(
    repo: String,
    tx: mpsc::Sender<String>,
    hwnd_raw: usize,
    notify_msg: u32,
) {
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
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut c_void)),
                        notify_msg,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        });
}

pub fn spawn_install_update_worker(
    rel: ReleaseInfo,
    parent_hwnd: Option<isize>,
    tx: mpsc::Sender<String>,
    hwnd_raw: usize,
    notify_msg: u32,
) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-settings-installer".into())
        .spawn(move || {
            let mut last_pct: i16 = -1;
            let mut last_stage = String::new();
            let res = download_and_verify_release_asset_with_progress(&rel, |prog| {
                if prog.percent as i16 == last_pct && prog.stage == last_stage {
                    return;
                }
                last_pct = prog.percent as i16;
                last_stage = prog.stage.clone();
                let evt = serde_json::json!({
                    "event": "update_install_progress",
                    "progress": prog
                })
                .to_string();
                if tx.send(evt).is_ok() {
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(hwnd_raw as *mut c_void)),
                            notify_msg,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            })
            .and_then(|path| launch_verified_update_installer(&path));
            let launched_installer = matches!(res, Ok(true));
            let evt = match res {
                Ok(started_exe) => serde_json::json!({
                    "event": "update_install_result",
                    "ok": true,
                    "launched_installer": started_exe,
                    "message": if started_exe {
                        "SHA-256 校验通过，正在后台安装新版本并自动重启 Tomoyo..."
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
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut c_void)),
                        notify_msg,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
            if launched_installer && let Some(h) = parent_hwnd {
                // Give WebView a moment to render the final status before tomoyo.exe shuts down cleanly.
                std::thread::sleep(std::time::Duration::from_millis(250));
                crate::bridge_os::notify_parent_mode(h, 5);
            }
        });
}

pub fn spawn_store_worker(
    req: StoreQuery,
    tx: mpsc::Sender<String>,
    hwnd_raw: usize,
    notify_msg: u32,
) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-settings-store".into())
        .spawn(move || {
            let evt = query_store_page_json(&req);
            if tx.send(evt).is_ok() {
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut c_void)),
                        notify_msg,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        });
}

/// Waits briefly for `tomoyo.exe` to re-register Win32 global hotkeys after `SaveSettings`,
/// then pushes the authoritative `hotkey_status.json` health report back to Settings Center.
pub fn spawn_hotkey_health_worker(tx: mpsc::Sender<String>, hwnd_raw: usize, notify_msg: u32) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-hotkey-health".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(180));
            push_evt(
                &tx,
                hwnd_raw,
                notify_msg,
                serde_json::json!({
                    "event": "hotkey_health",
                    "health": tomoyo_core::load_hotkey_health(),
                })
                .to_string(),
            );
        });
}

pub fn spawn_hotkey_audit_worker(
    parent_hwnd: Option<isize>,
    tx: mpsc::Sender<String>,
    hwnd_raw: usize,
    notify_msg: u32,
) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-hotkey-audit".into())
        .spawn(move || {
            let report = crate::hotkey_doctor::audit_system_hotkeys(parent_hwnd);
            push_evt(
                &tx,
                hwnd_raw,
                notify_msg,
                serde_json::json!({ "event": "hotkey_audit", "report": report }).to_string(),
            );
            // Wait briefly for tomoyo.exe to finish restoring hotkeys (mode 4) & push fresh health!
            std::thread::sleep(std::time::Duration::from_millis(160));
            push_evt(
                &tx,
                hwnd_raw,
                notify_msg,
                serde_json::json!({
                    "event": "hotkey_health",
                    "health": tomoyo_core::load_hotkey_health(),
                })
                .to_string(),
            );
        });
}

pub fn spawn_hotkey_probe_worker(
    spec: String,
    parent_hwnd: Option<isize>,
    tx: mpsc::Sender<String>,
    hwnd_raw: usize,
    notify_msg: u32,
) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-hotkey-probe".into())
        .spawn(move || {
            let result = crate::hotkey_doctor::probe_hotkey_owner_live(&spec, hwnd_raw, parent_hwnd);
            push_evt(
                &tx,
                hwnd_raw,
                notify_msg,
                serde_json::json!({ "event": "hotkey_probe_result", "result": result }).to_string(),
            );
            std::thread::sleep(std::time::Duration::from_millis(160));
            push_evt(
                &tx,
                hwnd_raw,
                notify_msg,
                serde_json::json!({
                    "event": "hotkey_health",
                    "health": tomoyo_core::load_hotkey_health(),
                })
                .to_string(),
            );
        });
}

pub fn dispatch_async_workers(
    act: crate::bridge::HostAction,
    plugin_id: &str,
    parent_hwnd: Option<isize>,
    tx: &mpsc::Sender<String>,
    hwnd_raw: usize,
    notify_msg: u32,
) {
    if let Some(repo) = act.async_check_updates {
        spawn_update_worker(repo, tx.clone(), hwnd_raw, notify_msg);
    }
    if let Some(rel) = act.async_install_update {
        spawn_install_update_worker(rel, parent_hwnd, tx.clone(), hwnd_raw, notify_msg);
    }
    if let Some(req) = act.async_fetch_store {
        spawn_store_worker(req, tx.clone(), hwnd_raw, notify_msg);
    }
    if let Some((plugin_id, sources)) = act.async_fetch_plugin_detail {
        let t = tx.clone();
        let _ = std::thread::Builder::new().name("tomoyo-store-detail".into()).spawn(move || {
            push_evt(&t, hwnd_raw, notify_msg, query_plugin_detail_json(&plugin_id, &sources));
        });
    }
    if let Some(sources) = act.async_check_plugin_health {
        let t = tx.clone();
        let _ = std::thread::Builder::new().name("tomoyo-plugin-health".into()).spawn(move || {
            push_evt(&t, hwnd_raw, notify_msg, crate::store_health::run_plugin_health_check_json(&sources));
        });
    }
    if let Some(sources) = act.async_check_plugin_updates {
        let t = tx.clone();
        let _ = std::thread::Builder::new().name("tomoyo-plugin-upd".into()).spawn(move || {
            push_evt(&t, hwnd_raw, notify_msg, crate::store_health::check_plugin_updates_silent_json(&sources));
        });
    }
    if act.async_refresh_hotkey_health {
        spawn_hotkey_health_worker(tx.clone(), hwnd_raw, notify_msg);
    }
    if act.async_audit_hotkeys {
        spawn_hotkey_audit_worker(parent_hwnd, tx.clone(), hwnd_raw, notify_msg);
    }
    if let Some(spec) = act.async_probe_hotkey {
        spawn_hotkey_probe_worker(spec, parent_hwnd, tx.clone(), hwnd_raw, notify_msg);
    }
    if let Some((req_id, method, params)) = act.async_plugin_rpc {
        let pid = plugin_id.to_string();
        let tx_c = tx.clone();
        let _ = std::thread::Builder::new().name("tomoyo-plugin-rpc".into()).spawn(move || {
            let reply = crate::plugin_rpc::execute_plugin_rpc(&pid, &req_id, &method, params);
            push_evt(&tx_c, hwnd_raw, notify_msg, reply);
        });
    }
}

fn push_evt(tx: &mpsc::Sender<String>, hwnd_raw: usize, notify_msg: u32, evt: String) {
    if tx.send(evt).is_ok() {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd_raw as *mut c_void)),
                notify_msg,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}
