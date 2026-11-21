//! Win32 Shell execution, window focus, SendInput auto-paste, process termination & ephemeral WebView spawning.

use std::process::Command;
use tomoyo_plugin_host::JobGuard;
use tomoyo_protocol::SystemCommandKind;

pub fn spawn_settings_window(parent_hwnd: isize, theme_name: &str) -> Result<(), String> {
    if try_focus_existing_settings_window() {
        return Ok(());
    }
    let host_exe = locate_webview_host()?;
    let mut cmd = Command::new(&host_exe);
    cmd.args([
        "--settings",
        "--notify-hwnd",
        &parent_hwnd.to_string(),
        "--theme",
        theme_name,
    ]);
    if theme_name.eq_ignore_ascii_case("light") {
        cmd.arg("--light");
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch settings host {}: {e}", host_exe.display()))?;
    let job = JobGuard::assign_child(&child, 512).ok();
    let _ = std::thread::Builder::new()
        .name("tomoyo-settings-reaper".into())
        .spawn(move || {
            let _keep_job = job;
            let _ = child.wait();
        });
    Ok(())
}

pub fn spawn_clipboard_manager_window(
    parent_hwnd: isize,
    theme_name: &str,
    focus_key: Option<&str>,
) -> Result<(), String> {
    if focus_key.is_none() && try_focus_existing_clipboard_window() {
        return Ok(());
    }
    let host_exe = locate_webview_host()?;
    let mut cmd = Command::new(&host_exe);
    cmd.args([
        "--plugin-id",
        "builtin.clipboard",
        "--title",
        "Tomoyo 剪贴板工作台",
        "--entry",
        "__tomoyo_clipboard_manager__",
        "--notify-hwnd",
        &parent_hwnd.to_string(),
        "--theme",
        theme_name,
        "--width",
        "1060",
        "--height",
        "680",
        "--query",
        focus_key.unwrap_or(""),
    ]);
    if theme_name.eq_ignore_ascii_case("light") {
        cmd.arg("--light");
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch clipboard workbench {}: {e}", host_exe.display()))?;
    let job = JobGuard::assign_child(&child, 512).ok();
    let _ = std::thread::Builder::new()
        .name("tomoyo-clipmgr-reaper".into())
        .spawn(move || {
            let _keep_job = job;
            let _ = child.wait();
        });
    Ok(())
}

pub fn spawn_translator_window(
    parent_hwnd: isize,
    theme_name: &str,
    text: &str,
    target_lang: Option<&str>,
) -> Result<(), String> {
    let req_dir = tomoyo_core::logs_dir();
    let req_json = serde_json::json!({
        "text": text,
        "target_lang": target_lang.unwrap_or(""),
    });
    let _ = std::fs::write(req_dir.join("last_translate_req.json"), req_json.to_string());

    if try_focus_existing_translator_window() {
        return Ok(());
    }
    let host_exe = locate_webview_host()?;
    let mut cmd = Command::new(&host_exe);
    cmd.args([
        "--plugin-id",
        "builtin.translate",
        "--title",
        "Tomoyo 多源翻译",
        "--entry",
        "__tomoyo_translator__",
        "--notify-hwnd",
        &parent_hwnd.to_string(),
        "--theme",
        theme_name,
        "--width",
        "780",
        "--height",
        "620",
    ]);
    if theme_name.eq_ignore_ascii_case("light") {
        cmd.arg("--light");
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch translator {}: {e}", host_exe.display()))?;
    let job = JobGuard::assign_child(&child, 512).ok();
    let _ = std::thread::Builder::new()
        .name("tomoyo-tr-reaper".into())
        .spawn(move || {
            let _keep_job = job;
            let _ = child.wait();
        });
    Ok(())
}

#[cfg(windows)]
fn try_focus_existing_translator_window() -> bool {
    use windows::core::w;
    try_focus_existing_window(w!("TomoyoTranslatorWindowClass"), 0x8000 + 35)
}

#[cfg(windows)]
fn try_focus_existing_settings_window() -> bool {
    use windows::core::w;
    try_focus_existing_window(w!("TomoyoSettingsCenterWindowClass"), 0x8000 + 32)
}

#[cfg(windows)]
fn try_focus_existing_clipboard_window() -> bool {
    use windows::core::w;
    try_focus_existing_window(w!("TomoyoClipboardManagerWindowClass"), 0x8000 + 34)
}

#[cfg(windows)]
fn try_focus_existing_window(class_name: windows::core::PCWSTR, activate_msg: u32) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, IsIconic, PostMessageW,
        SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };

    unsafe {
        let Ok(hwnd) = FindWindowW(class_name, PCWSTR::null()) else {
            return false;
        };
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            let _ = AllowSetForegroundWindow(pid);
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = PostMessageW(Some(hwnd), activate_msg, Default::default(), Default::default());
        true
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_ephemeral_webview(
    plugin_id: &str,
    title: &str,
    entry_path: &str,
    utools_compat: bool,
    initial_query: &str,
    width: u32,
    height: u32,
    max_memory_mb: u32,
    is_dark: bool,
) -> Result<(), String> {
    let host_exe = locate_webview_host()?;
    let mut cmd = Command::new(&host_exe);
    cmd.args([
        "--plugin-id",
        plugin_id,
        "--title",
        title,
        "--entry",
        entry_path,
        "--query",
        initial_query,
        "--width",
        &width.to_string(),
        "--height",
        &height.to_string(),
    ]);
    if utools_compat {
        cmd.arg("--utools-compat");
    }
    if !is_dark {
        cmd.arg("--light");
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch {}: {e}", host_exe.display()))?;
    let job = JobGuard::assign_child(&child, max_memory_mb.max(512)).ok();
    let _ = std::thread::Builder::new()
        .name("tomoyo-wv-reaper".into())
        .spawn(move || {
            let _keep_job = job;
            let _ = child.wait();
        });
    Ok(())
}

fn locate_sibling_exe(name: &str) -> Result<std::path::PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sib = dir.join(name);
            if sib.is_file() {
                return Ok(sib);
            }
            if let Some(parent) = dir.parent() {
                let p = parent.join(name);
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }
    Err(format!("{name} not found next to tomoyo.exe"))
}

fn locate_webview_host() -> Result<std::path::PathBuf, String> {
    locate_sibling_exe("tomoyo-webview-host.exe")
}

pub fn spawn_capture_tool(mode: &str) -> Result<(), String> {
    let cap_exe = locate_sibling_exe("tomoyo-capture.exe")?;
    let flag = match mode {
        "ocr" => "--ocr",
        "translate" => "--translate",
        "pin_clipboard" | "pin" => "--pin-clipboard",
        _ => "--capture",
    };
    let mut child = Command::new(&cap_exe)
        .arg(flag)
        .spawn()
        .map_err(|e| format!("failed to launch {}: {e}", cap_exe.display()))?;
    let _ = std::thread::Builder::new()
        .name("tomoyo-cap-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

/// Restores the previously focused window and simulates `Ctrl+V` via native `SendInput`.
#[cfg(windows)]
pub fn paste_into_previous_window_async(prev_hwnd: isize) {
    if prev_hwnd == 0 {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("tomoyo-autopaste".into())
        .spawn(move || {
            use std::ffi::c_void;
            use std::time::Duration;
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
                VIRTUAL_KEY, VK_CONTROL,
            };
            use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

            std::thread::sleep(Duration::from_millis(55));
            unsafe {
                let _ = SetForegroundWindow(HWND(prev_hwnd as *mut c_void));
            }
            std::thread::sleep(Duration::from_millis(35));

            let make_key = |vk: VIRTUAL_KEY, up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                        ..Default::default()
                    },
                },
            };
            let inputs = [
                make_key(VK_CONTROL, false),
                make_key(VIRTUAL_KEY(b'V' as u16), false),
                make_key(VIRTUAL_KEY(b'V' as u16), true),
                make_key(VK_CONTROL, true),
            ];
            unsafe {
                let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        });
}

#[cfg(windows)]
pub fn shell_open(target: &str, args: Option<&str>, workdir: Option<&str>) -> bool {
    shell_open_with_verb(windows::core::w!("open"), target, args, workdir)
}

#[cfg(windows)]
pub fn shell_open_as_admin(target: &str, args: Option<&str>, workdir: Option<&str>) -> bool {
    shell_open_with_verb(windows::core::w!("runas"), target, args, workdir)
}

#[cfg(windows)]
fn shell_open_with_verb(
    verb: windows::core::PCWSTR,
    target: &str,
    args: Option<&str>,
    workdir: Option<&str>,
) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let target_w: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let args_w: Option<Vec<u16>> = args.map(|a| a.encode_utf16().chain(std::iter::once(0)).collect());
    let dir_w: Option<Vec<u16>> = workdir.map(|d| d.encode_utf16().chain(std::iter::once(0)).collect());

    unsafe {
        let hinst = ShellExecuteW(
            None,
            verb,
            PCWSTR(target_w.as_ptr()),
            args_w.as_ref().map(|v| PCWSTR(v.as_ptr())).unwrap_or(PCWSTR::null()),
            dir_w.as_ref().map(|v| PCWSTR(v.as_ptr())).unwrap_or(PCWSTR::null()),
            SW_SHOWNORMAL,
        );
        (hinst.0 as isize) > 32
    }
}

#[cfg(windows)]
pub fn focus_window(raw_hwnd: isize) {
    use std::ffi::c_void;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    unsafe {
        let hwnd = HWND(raw_hwnd as *mut c_void);
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(windows)]
pub fn run_system_command(cmd: SystemCommandKind) {
    use windows::Win32::System::Shutdown::LockWorkStation;
    use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOSOUND};

    match cmd {
        SystemCommandKind::LockWorkstation => unsafe {
            let _ = LockWorkStation();
        },
        SystemCommandKind::Sleep => {
            let _ = shell_open("rundll32.exe", Some("powrprof.dll,SetSuspendState 0,1,0"), None);
        }
        SystemCommandKind::EmptyRecycleBin => unsafe {
            let _ = SHEmptyRecycleBinW(None, None, SHERB_NOCONFIRMATION | SHERB_NOSOUND);
        },
        SystemCommandKind::OpenSettings => {
            let _ = shell_open("ms-settings:", None, None);
        }
        SystemCommandKind::OpenTaskManager => {
            let _ = shell_open("taskmgr.exe", None, None);
        }
        SystemCommandKind::OpenTerminal => {
            if !shell_open("wt.exe", None, None) {
                let _ = shell_open("powershell.exe", None, None);
            }
        }
    }
}

#[cfg(windows)]
pub fn kill_process_by_pid(pid: u32) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    if pid == 0 || pid == 4 || pid == std::process::id() {
        return Err("拒绝结束受保护系统或自身进程".into());
    }
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| format!("无法打开进程 PID {pid}（可能需要管理员权限）: {e}"))?;
        let res = TerminateProcess(handle, 1).map_err(|e| format!("结束进程失败: {e}"));
        let _ = CloseHandle(handle);
        res
    }
}
