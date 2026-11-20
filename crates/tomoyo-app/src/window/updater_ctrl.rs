//! Async auto-update check, progress-reporting download, and silent self-upgrade orchestration.

use super::{AppShell, WM_APP_UPDATE_INSTALLED, WM_APP_UPDATE_READY};
use std::ffi::c_void;
use std::sync::Arc;
use tomoyo_core::{
    download_and_verify_release_asset_with_progress, launch_verified_update_installer, Lang,
    UpdateStatus,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, PostMessageW, WM_APP};

pub(super) const WM_APP_UPDATE_PROGRESS: u32 = WM_APP + 23;

#[derive(Default)]
pub(crate) struct UpdateInstallSlot {
    pub(super) progress_toast: Option<String>,
    pub(super) outcome: Option<Result<bool, String>>,
}

impl AppShell {
    pub(super) fn trigger_update_check_async(&mut self) {
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

    pub(super) fn on_update_ready(&mut self) {
        let msg = match self.updater.snapshot() {
            UpdateStatus::Available { release, .. } => match self.lang {
                Lang::Zh => format!("发现新版本 v{}！点击右下角徽标或输入 :update 一键升级", release.version),
                Lang::En => format!("New version v{} available! Click badge or :update to upgrade", release.version),
            },
            UpdateStatus::UpToDate { current, note, .. } => {
                if note.is_empty() {
                    match self.lang {
                        Lang::Zh => format!("当前已是最新版本 v{current}"),
                        Lang::En => format!("Already on latest version v{current}"),
                    }
                } else {
                    format!("v{current} · {note}")
                }
            }
            UpdateStatus::Error { message, .. } => match self.lang {
                Lang::Zh => format!("检查更新失败：{message}"),
                Lang::En => format!("Update check failed: {message}"),
            },
            _ => return,
        };
        if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool() } {
            self.show_toast(&msg);
            if self.current_query.trim().starts_with(':') {
                self.dispatch_query();
            }
        } else {
            self.invalidate();
        }
    }

    pub(super) fn trigger_update_install_async(&mut self) {
        let Some(release) = self.updater.available_release() else {
            self.trigger_update_check_async();
            return;
        };
        self.show_toast(&match self.lang {
            Lang::Zh => format!("准备下载 v{} 安装包并校验 SHA-256...", release.version),
            Lang::En => format!("Preparing v{} download & SHA-256 check...", release.version),
        });
        let slot = Arc::clone(&self.pending_update_install);
        let hwnd_raw = self.hwnd.0 as usize;
        let _ = std::thread::Builder::new()
            .name("tomoyo-self-updater".into())
            .spawn(move || {
                let mut last_pct: i16 = -10;
                let mut last_stage = String::new();
                let res = download_and_verify_release_asset_with_progress(&release, |prog| {
                    let pct = prog.percent as i16;
                    if prog.stage == last_stage && (pct - last_pct).abs() < 8 {
                        return;
                    }
                    last_pct = pct;
                    last_stage = prog.stage.clone();
                    if let Ok(mut g) = slot.lock() {
                        g.progress_toast = Some(prog.message);
                    }
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(hwnd_raw as *mut c_void)),
                            WM_APP_UPDATE_PROGRESS,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                })
                .and_then(|path| launch_verified_update_installer(&path));
                if let Ok(mut g) = slot.lock() {
                    g.outcome = Some(res);
                }
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut c_void)),
                        WM_APP_UPDATE_INSTALLED,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            });
    }

    pub(super) fn on_update_progress(&mut self) {
        let msg = self
            .pending_update_install
            .lock()
            .ok()
            .and_then(|mut g| g.progress_toast.take());
        if let Some(m) = msg {
            self.show_toast(&m);
        }
    }

    pub(super) fn on_update_installed(&mut self) {
        let outcome = self
            .pending_update_install
            .lock()
            .ok()
            .and_then(|mut g| g.outcome.take());
        let Some(res) = outcome else {
            return;
        };
        match res {
            Ok(true) => {
                self.shutdown_clean();
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            Ok(false) => {
                self.show_toast(self.lang.pick(
                    "便携包校验通过并已下载，已在资源管理器中选中",
                    "Verified update ZIP downloaded and highlighted in Explorer",
                ));
            }
            Err(err) => {
                self.show_toast(&format!(
                    "{}: {err}",
                    self.lang.pick("自动更新失败", "Auto-update failed")
                ));
            }
        }
    }
}
