//! Background threads for Settings Center updater checks, installer execution & Store loading.

use crate::store::fetch_store_catalog_json;
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
        });
}

pub fn spawn_store_worker(
    sources: Vec<String>,
    tx: mpsc::Sender<String>,
    hwnd_raw: usize,
    notify_msg: u32,
) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-settings-store".into())
        .spawn(move || {
            let evt = fetch_store_catalog_json(&sources);
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

pub fn spawn_translate_worker(
    text: String,
    source_lang: String,
    target_lang: String,
    tx: mpsc::Sender<String>,
    hwnd_raw: usize,
    notify_msg: u32,
) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-translator-worker".into())
        .spawn(move || {
            let cfg = TomoyoConfig::load_or_create();
            let batch = tomoyo_core::translate::translate_parallel(
                &cfg.translate,
                &text,
                &source_lang,
                &target_lang,
            );
            let store = tomoyo_core::translate::TranslateStore::load_default();
            let evt = serde_json::json!({
                "event": "translate_result",
                "data": batch,
                "store": {
                    "history": store.history,
                    "wordbook": store.wordbook,
                }
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
        });
}

/// Waits briefly for `tomoyo.exe` to re-register Win32 global hotkeys after `SaveSettings`,
/// then pushes the authoritative `hotkey_status.json` health report back to Settings Center.
pub fn spawn_hotkey_health_worker(tx: mpsc::Sender<String>, hwnd_raw: usize, notify_msg: u32) {
    let _ = std::thread::Builder::new()
        .name("tomoyo-hotkey-health".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(180));
            let evt = serde_json::json!({
                "event": "hotkey_health",
                "health": tomoyo_core::load_hotkey_health(),
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
        });
}
