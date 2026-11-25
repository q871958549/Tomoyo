//! Win32 COM-STA shell launcher and OS window-readiness detector for `launch_ctrl`.

use std::collections::HashSet;
use std::time::{Duration, Instant};
use tomoyo_core::Lang;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND,
    ERROR_NO_ASSOCIATION, ERROR_PATH_NOT_FOUND, HANDLE, HWND, LPARAM, RECT, WAIT_OBJECT_0,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{GetProcessId, WaitForInputIdle, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
    IsWindowVisible, GWL_EXSTYLE, SW_SHOWNORMAL, WS_EX_TOOLWINDOW,
};

pub(super) struct ComStaGuard(bool);
impl ComStaGuard {
    pub(super) fn init() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() })
    }
}
impl Drop for ComStaGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize(); }
        }
    }
}

pub(super) struct ProcessHandleGuard(HANDLE);
impl ProcessHandleGuard {
    fn pid(&self) -> u32 {
        unsafe { GetProcessId(self.0) }
    }
}
impl Drop for ProcessHandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe { let _ = CloseHandle(self.0); }
        }
    }
}

pub(super) fn shell_execute_ex(
    as_admin: bool,
    target: &str,
    args: Option<&str>,
    workdir: Option<&str>,
    lang: Lang,
) -> Result<Option<ProcessHandleGuard>, String> {
    let target_w: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let args_w: Option<Vec<u16>> = args.map(|a| a.encode_utf16().chain(std::iter::once(0)).collect());
    let dir_w: Option<Vec<u16>> = workdir.map(|d| d.encode_utf16().chain(std::iter::once(0)).collect());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | if as_admin { 0 } else { SEE_MASK_FLAG_NO_UI },
        lpVerb: if as_admin { w!("runas") } else { w!("open") },
        lpFile: PCWSTR(target_w.as_ptr()),
        lpParameters: args_w.as_ref().map(|v| PCWSTR(v.as_ptr())).unwrap_or(PCWSTR::null()),
        lpDirectory: dir_w.as_ref().map(|v| PCWSTR(v.as_ptr())).unwrap_or(PCWSTR::null()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut info) }.is_ok() {
        Ok((!info.hProcess.is_invalid()).then_some(ProcessHandleGuard(info.hProcess)))
    } else {
        Err(format_launch_error(unsafe { GetLastError() }.0, as_admin, lang))
    }
}

/// Waits until the launched application actually presents a visible top-level window (or gains
/// foreground focus), handling stub launchers (Electron/JetBrains/WeChat/Steam) that spawn a
/// child process and exit early, bounded by safe timeouts (25s with process handle, 8s shell-only).
pub(super) fn wait_until_app_ready(
    proc_guard: Option<ProcessHandleGuard>,
    fg_before: isize,
    tomoyo_hwnd: isize,
    wins_before: &HashSet<isize>,
) {
    let deadline = Instant::now() + Duration::from_secs(if proc_guard.is_some() { 25 } else { 8 });
    let mut exited_at: Option<Instant> = None;
    let pid = proc_guard.as_ref().map(|g| g.pid()).unwrap_or(0);

    while Instant::now() < deadline {
        if pid != 0 && has_visible_window_for_pid(pid) {
            return;
        }
        let fg_now = unsafe { GetForegroundWindow().0 as isize };
        if fg_now != 0 && fg_now != fg_before && fg_now != tomoyo_hwnd {
            return;
        }
        if has_new_visible_window(wins_before, tomoyo_hwnd) {
            return;
        }
        if let Some(ref g) = proc_guard {
            if unsafe { WaitForSingleObject(g.0, 0) } == WAIT_OBJECT_0 {
                let first_exit = *exited_at.get_or_insert_with(Instant::now);
                if first_exit.elapsed() >= Duration::from_millis(1400) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            } else {
                let _ = unsafe { WaitForInputIdle(g.0, 250) };
                std::thread::sleep(Duration::from_millis(80));
            }
        } else {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

pub(super) fn snapshot_visible_top_windows() -> HashSet<isize> {
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if is_meaningful_visible_window(hwnd) {
            let set = unsafe { &mut *(lparam.0 as *mut HashSet<isize>) };
            set.insert(hwnd.0 as isize);
        }
        BOOL(1)
    }
    let mut set = HashSet::new();
    unsafe { let _ = EnumWindows(Some(cb), LPARAM(&mut set as *mut _ as isize)); }
    set
}

fn is_meaningful_visible_window(hwnd: HWND) -> bool {
    if !unsafe { IsWindowVisible(hwnd).as_bool() } {
        return false;
    }
    let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    if (ex & WS_EX_TOOLWINDOW.0) != 0 {
        return false;
    }
    let mut rc = RECT::default();
    let ok = unsafe { GetWindowRect(hwnd, &mut rc).is_ok() };
    ok && (rc.right - rc.left) >= 48 && (rc.bottom - rc.top) >= 48
}

fn has_visible_window_for_pid(target_pid: u32) -> bool {
    struct Ctx { target_pid: u32, found: bool }
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)); }
        if pid == ctx.target_pid && is_meaningful_visible_window(hwnd) {
            ctx.found = true;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut ctx = Ctx { target_pid, found: false };
    unsafe { let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut _ as isize)); }
    ctx.found
}

fn has_new_visible_window(before: &HashSet<isize>, ignore_hwnd: isize) -> bool {
    struct Ctx<'a> { before: &'a HashSet<isize>, ignore_hwnd: isize, found: bool }
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx<'_>) };
        let raw = hwnd.0 as isize;
        if raw != ctx.ignore_hwnd && !ctx.before.contains(&raw) && is_meaningful_visible_window(hwnd) {
            ctx.found = true;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut ctx = Ctx { before, ignore_hwnd, found: false };
    unsafe { let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut _ as isize)); }
    ctx.found
}

fn format_launch_error(code: u32, as_admin: bool, lang: Lang) -> String {
    match code {
        c if c == ERROR_FILE_NOT_FOUND.0 => lang.pick("目标文件或快捷方式不存在", "Target file or shortcut not found").into(),
        c if c == ERROR_PATH_NOT_FOUND.0 => lang.pick("目标路径不存在或磁盘未连接", "Path not found or drive disconnected").into(),
        c if c == ERROR_CANCELLED.0 => if as_admin { lang.pick("已取消管理员授权", "Admin UAC prompt cancelled") } else { lang.pick("启动已被系统取消", "Launch cancelled by system") }.into(),
        c if c == ERROR_ACCESS_DENIED.0 => lang.pick("权限不足，可尝试 Ctrl+Shift+Enter 管理员运行", "Access denied (try Ctrl+Shift+Enter)").into(),
        c if c == ERROR_NO_ASSOCIATION.0 => lang.pick("该文件类型暂无默认打开程序", "No default app associated with this file").into(),
        other => match lang { Lang::Zh => format!("系统返回错误 #{other}"), Lang::En => format!("Windows error #{other}") },
    }
}
