//! Native Win32 bridge helpers (parent reload notification, ShellExecuteW, Unicode clipboard write).

#[cfg(windows)]
pub const WM_APP_CONFIG_SAVED: u32 = 0x8000 + 20;

#[cfg(windows)]
pub fn notify_parent_mode(raw_hwnd: isize, mode: usize) {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    if raw_hwnd != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw_hwnd as *mut c_void)),
                WM_APP_CONFIG_SAVED,
                WPARAM(mode),
                LPARAM(0),
            );
        }
    }
}

#[cfg(target_os = "linux")]
pub fn notify_parent_mode(raw_hwnd: isize, mode: usize) {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let sock = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
        .join("tomoyo-ipc.sock");
    if let Ok(mut s) = UnixStream::connect(&sock)
        && s.write_all(format!("reload:{mode}\n").as_bytes()).is_ok()
    {
        return;
    }
    if raw_hwnd > 0 && matches!(mode, 0 | 1) {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        const SIGUSR1: i32 = 10;
        unsafe { let _ = kill(raw_hwnd as i32, SIGUSR1); }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn notify_parent_mode(_raw_hwnd: isize, _mode: usize) {}

#[cfg(windows)]
pub fn shell_open(target: &str) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(target_os = "linux")]
pub fn shell_open(target: &str) {
    let _ = tomoyo_core::linux_launch::spawn_isolated(
        "xdg-open",
        &[target.to_string()],
        None,
        Some("xdg-open"),
    );
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn shell_open(_target: &str) {}

#[cfg(windows)]
pub fn set_clipboard_text(text: &str) {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * 2;
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, byte_len) {
            let ptr = GlobalLock(hmem) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(hmem);
                let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

#[cfg(target_os = "linux")]
pub fn set_clipboard_text(text: &str) {
    unsafe extern "C" {
        fn tomoyo_wv_set_clipboard(text: *const std::ffi::c_char);
    }
    if let Ok(cs) = std::ffi::CString::new(text) {
        unsafe { tomoyo_wv_set_clipboard(cs.as_ptr()); }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn set_clipboard_text(_text: &str) {}

pub fn spawn_settings_center(target: &str, notify_hwnd: Option<isize>) {
    let Ok(exe) = std::env::current_exe() else { return };
    let cfg = tomoyo_core::TomoyoConfig::load_or_create();
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["--settings", "--theme", &cfg.theme, "--query", target]);
    if cfg.theme.eq_ignore_ascii_case("light") {
        cmd.arg("--light");
    }
    if let Some(hwnd) = notify_hwnd {
        cmd.args(["--notify-hwnd", &hwnd.to_string()]);
    }
    let _ = cmd.spawn();
}

/// Enumerates curated privacy/remote-desktop apps + currently running desktop processes
/// for Settings Center searchable comboboxes.
pub fn enumerate_running_app_exes() -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for preset in [
        "keepass.exe",
        "keepassxc.exe",
        "bitwarden.exe",
        "1password.exe",
        "enpass.exe",
        "lastpass.exe",
        "mstsc.exe",
        "todesk.exe",
        "anydesk.exe",
        "wechat.exe",
        "qq.exe",
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "code.exe",
    ] {
        set.insert(preset.to_string());
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        unsafe {
            if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut pe = PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                if Process32FirstW(snap, &mut pe).is_ok() {
                    loop {
                        let len = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                        let name = String::from_utf16_lossy(&pe.szExeFile[..len]).to_ascii_lowercase();
                        if name.ends_with(".exe")
                            && !matches!(name.as_str(), "svchost.exe" | "conhost.exe" | "csrss.exe" | "smss.exe" | "services.exe" | "lsass.exe" | "dwm.exe" | "tomoyo.exe" | "tomoyo-webview-host.exe")
                        {
                            set.insert(name);
                        }
                        if Process32NextW(snap, &mut pe).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snap);
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for preset in [
            "keepassxc", "bitwarden", "1password", "enpass", "rustdesk", "remmina",
            "firefox", "chromium", "google-chrome", "code", "wechat", "qq",
        ] {
            set.insert(preset.to_string());
        }
        if let Ok(rd) = std::fs::read_dir("/proc") {
            for ent in rd.flatten() {
                let name = ent.file_name();
                if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
                    continue;
                }
                if let Ok(comm) = std::fs::read_to_string(ent.path().join("comm")) {
                    let c = comm.trim().to_ascii_lowercase();
                    if !c.is_empty() && !matches!(c.as_str(), "tomoyo" | "tomoyo-webview-host" | "systemd" | "kworker" | "dbus-daemon") {
                        set.insert(c);
                    }
                }
            }
        }
    }
    set.into_iter().take(120).collect()
}
