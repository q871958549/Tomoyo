//! Non-blocking COM-STA `ShellExecuteExW` app/file/URL launcher with `< 2 ms` visual confirmation,
//! foreground-activation handoff (`AllowSetForegroundWindow(ASFW_ANY)`), and actionable error feedback.

use super::{AppShell, TIMER_LAUNCH_HIDE, WM_APP_LAUNCH_DONE};
use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tomoyo_core::Lang;
use tomoyo_protocol::ActionKind;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ACCESS_DENIED, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND,
    ERROR_NO_ASSOCIATION, ERROR_PATH_NOT_FOUND, HWND, LPARAM, WPARAM,
};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, KillTimer, PostMessageW, SetTimer, ASFW_ANY, SW_SHOWNORMAL,
};

pub(crate) struct LaunchingState {
    pub item_id: String,
    pub started_at: Instant,
    pub finished_ok: bool,
}

pub(crate) struct LaunchResultMsg {
    pub item_id: String,
    pub title: String,
    pub is_admin: bool,
    pub result: Result<(), String>,
}

enum LaunchSpec {
    Shell {
        verb_admin: bool,
        target: String,
        args: Option<String>,
        workdir: Option<String>,
    },
    Terminal {
        dir: String,
    },
}

struct ComStaGuard(bool);
impl ComStaGuard {
    fn init() -> Self {
        let ok = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
        Self(ok)
    }
}
impl Drop for ComStaGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize(); }
        }
    }
}

impl AppShell {
    /// Attempts to handle shell-launch actions asynchronously with instant `< 2 ms` UI feedback.
    /// Returns `true` if the action was accepted by the async launcher.
    pub(super) fn try_start_async_launch(
        &mut self,
        kind: &ActionKind,
        item_id: &str,
        item_title: &str,
    ) -> bool {
        let (spec, is_admin) = match kind {
            ActionKind::LaunchPath { path, args, working_dir } => (
                LaunchSpec::Shell {
                    verb_admin: false,
                    target: path.clone(),
                    args: args.clone(),
                    workdir: working_dir.clone(),
                },
                false,
            ),
            ActionKind::LaunchAsAdmin { path, args, working_dir } => (
                LaunchSpec::Shell {
                    verb_admin: true,
                    target: path.clone(),
                    args: args.clone(),
                    workdir: working_dir.clone(),
                },
                true,
            ),
            ActionKind::OpenUrl { url } => (
                LaunchSpec::Shell {
                    verb_admin: false,
                    target: url.clone(),
                    args: None,
                    workdir: None,
                },
                false,
            ),
            ActionKind::RevealInExplorer { path } => (
                LaunchSpec::Shell {
                    verb_admin: false,
                    target: "explorer.exe".into(),
                    args: Some(format!("/select,\"{path}\"")),
                    workdir: None,
                },
                false,
            ),
            ActionKind::OpenTerminalAt { dir } => (LaunchSpec::Terminal { dir: dir.clone() }, false),
            _ => return false,
        };

        // Guard against frantic triple-click / repeated Enter spawning duplicate app instances.
        if let Some(active) = self.inter.launching.as_ref()
            && active.item_id == item_id
            && active.started_at.elapsed().as_millis() < 1600
        {
            return true;
        }

        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_HIDE);
        }
        self.action_menu_open = false;
        self.inter.drag_origin = None;
        if is_admin {
            self.inter.in_modal_overlay = true;
        }
        self.inter.launching = Some(LaunchingState {
            item_id: item_id.to_string(),
            started_at: Instant::now(),
            finished_ok: false,
        });

        let msg = match (self.lang, is_admin) {
            (Lang::Zh, true) => format!("正在请求管理员权限启动「{item_title}」…"),
            (Lang::En, true) => format!("Requesting admin launch for \"{item_title}\"…"),
            (Lang::Zh, false) => format!("正在启动「{item_title}」…"),
            (Lang::En, false) => format!("Launching \"{item_title}\"…"),
        };
        self.show_toast(&msg);
        // Flush the visual confirmation frame synchronously (< 2 ms) before spawning the launch thread!
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            let _ = UpdateWindow(self.hwnd);
        }

        let slot = Arc::clone(&self.pending_launch_result);
        let hwnd_raw = self.hwnd.0 as usize;
        let id_owned = item_id.to_string();
        let title_owned = item_title.to_string();
        let lang = self.lang;
        let _ = std::thread::Builder::new()
            .name("tomoyo-launch".into())
            .spawn(move || {
                let _com = ComStaGuard::init();
                unsafe {
                    let _ = AllowSetForegroundWindow(ASFW_ANY);
                }
                let result = match spec {
                    LaunchSpec::Shell { verb_admin, target, args, workdir } => {
                        shell_execute_ex(verb_admin, &target, args.as_deref(), workdir.as_deref(), lang)
                    }
                    LaunchSpec::Terminal { dir } => {
                        let target_dir = if Path::new(&dir).is_dir() { dir.as_str() } else { "." };
                        let wt_arg = format!("-d \"{target_dir}\"");
                        shell_execute_ex(false, "wt.exe", Some(&wt_arg), Some(target_dir), lang)
                            .or_else(|_| shell_execute_ex(false, "powershell.exe", None, Some(target_dir), lang))
                    }
                };
                if let Ok(mut g) = slot.lock() {
                    *g = Some(LaunchResultMsg {
                        item_id: id_owned,
                        title: title_owned,
                        is_admin,
                        result,
                    });
                }
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut c_void)),
                        WM_APP_LAUNCH_DONE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            });
        true
    }

    pub(super) fn on_async_launch_done(&mut self) {
        let Some(done) = self.pending_launch_result.lock().ok().and_then(|mut g| g.take()) else {
            return;
        };
        if done.is_admin {
            self.inter.in_modal_overlay = false;
        }
        match done.result {
            Ok(()) => {
                let elapsed = self
                    .inter
                    .launching
                    .as_ref()
                    .map(|l| l.started_at.elapsed().as_millis() as u32)
                    .unwrap_or(0);
                if let Some(l) = self.inter.launching.as_mut()
                    && l.item_id == done.item_id
                {
                    l.finished_ok = true;
                }
                let ok_msg = match self.lang {
                    Lang::Zh => format!("✓ 已启动「{}」", done.title),
                    Lang::En => format!("✓ Launched \"{}\"", done.title),
                };
                self.show_toast(&ok_msg);
                unsafe {
                    let _ = InvalidateRect(Some(self.hwnd), None, false);
                    let _ = UpdateWindow(self.hwnd);
                }
                let remain_ms = 240u32.saturating_sub(elapsed).clamp(130, 240);
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_HIDE);
                    let _ = SetTimer(Some(self.hwnd), TIMER_LAUNCH_HIDE, remain_ms, None);
                }
            }
            Err(err) => {
                let tid = tomoyo_core::next_trace_id("ACT");
                tracing::warn!(trace_id = %tid, item_id = %done.item_id, title = %done.title, "Async launch failed: {err}");
                self.inter.launching = None;
                let fail_msg = match self.lang {
                    Lang::Zh => format!("✗ 启动失败「{}」：{err}", done.title),
                    Lang::En => format!("✗ Failed \"{}\": {err}", done.title),
                };
                self.show_toast(&fail_msg);
            }
        }
    }

    pub(super) fn cancel_pending_launch_hide(&mut self) {
        self.inter.launching = None;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_HIDE);
        }
    }
}

fn shell_execute_ex(
    as_admin: bool,
    target: &str,
    args: Option<&str>,
    workdir: Option<&str>,
    lang: Lang,
) -> Result<(), String> {
    let target_w: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let args_w: Option<Vec<u16>> = args.map(|a| a.encode_utf16().chain(std::iter::once(0)).collect());
    let dir_w: Option<Vec<u16>> = workdir.map(|d| d.encode_utf16().chain(std::iter::once(0)).collect());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: if as_admin { 0 } else { SEE_MASK_FLAG_NO_UI },
        lpVerb: if as_admin { w!("runas") } else { w!("open") },
        lpFile: PCWSTR(target_w.as_ptr()),
        lpParameters: args_w.as_ref().map(|v| PCWSTR(v.as_ptr())).unwrap_or(PCWSTR::null()),
        lpDirectory: dir_w.as_ref().map(|v| PCWSTR(v.as_ptr())).unwrap_or(PCWSTR::null()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    let res = unsafe { ShellExecuteExW(&mut info) };
    if res.is_ok() {
        Ok(())
    } else {
        let code = unsafe { GetLastError() };
        Err(format_launch_error(code.0, as_admin, lang))
    }
}

fn format_launch_error(code: u32, as_admin: bool, lang: Lang) -> String {
    match code {
        c if c == ERROR_FILE_NOT_FOUND.0 => {
            lang.pick("目标文件或快捷方式不存在", "Target file or shortcut not found").into()
        }
        c if c == ERROR_PATH_NOT_FOUND.0 => {
            lang.pick("目标路径不存在或磁盘未连接", "Path not found or drive disconnected").into()
        }
        c if c == ERROR_CANCELLED.0 => {
            if as_admin {
                lang.pick("已取消管理员授权", "Admin UAC prompt cancelled").into()
            } else {
                lang.pick("启动已被系统取消", "Launch cancelled by system").into()
            }
        }
        c if c == ERROR_ACCESS_DENIED.0 => {
            lang.pick("权限不足，可尝试 Ctrl+Shift+Enter 管理员运行", "Access denied (try Ctrl+Shift+Enter)").into()
        }
        c if c == ERROR_NO_ASSOCIATION.0 => {
            lang.pick("该文件类型暂无默认打开程序", "No default app associated with this file").into()
        }
        other => match lang {
            Lang::Zh => format!("系统返回错误 #{other}"),
            Lang::En => format!("Windows error #{other}"),
        },
    }
}
