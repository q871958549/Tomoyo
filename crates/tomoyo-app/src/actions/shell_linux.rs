//! Native Linux desktop shell launching, XDG file reveal, terminal dispatch,
//! KWin window activation, XTest Ctrl+V autopaste, and systemd/KDE commands.

use std::ffi::{c_char, c_int, CString};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use tomoyo_core::linux_launch::{command_in_path, sanitize_instance_tag, spawn_isolated};
use tomoyo_protocol::SystemCommandKind;

unsafe extern "C" {
    fn getuid() -> u32;
    fn tomoyo_linux_launch_desktop(path: *const c_char) -> c_int;
    fn tomoyo_linux_send_ctrl_chord(key: c_char);
    fn kill(pid: i32, sig: i32) -> i32;
}

pub fn spawn_settings_window_at(
    parent_pid: isize,
    theme_name: &str,
    route: Option<&str>,
) -> Result<bool, String> {
    let mut args = vec!["--settings", "--title", "Tomoyo 设置中心"];
    let clean = route.map(str::trim).filter(|s| !s.is_empty());
    if let Some(r) = clean {
        args.push("--query");
        args.push(r);
    }
    spawn_ephemeral_singleton(
        "settings",
        parent_pid,
        theme_name,
        &args,
        960,
        670,
    )
    .map(|()| true)
}

fn try_wake_singleton_socket(mode_tag: &str, payload: &str) -> bool {
    if mode_tag.is_empty() {
        return false;
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let sock = dir.join(format!("tomoyo-wv-{}-{mode_tag}.sock", unsafe { getuid() }));
    let Ok(mut stream) = UnixStream::connect(&sock) else {
        return false;
    };
    stream.write_all(payload.as_bytes()).is_ok()
}

fn spawn_ephemeral_singleton(
    mode_tag: &str,
    parent_pid: isize,
    theme_name: &str,
    extra: &[&str],
    width: i32,
    height: i32,
) -> Result<(), String> {
    if try_wake_singleton_socket(mode_tag, "activate\n") {
        return Ok(());
    }
    let host_exe = locate_sibling_exe("tomoyo-webview-host")?;
    let mut cmd = Command::new(&host_exe);
    cmd.args(extra)
        .args(["--theme", theme_name, "--width", &width.to_string(), "--height", &height.to_string()]);
    if theme_name.eq_ignore_ascii_case("light") {
        cmd.arg("--light");
    }
    if parent_pid > 0 {
        cmd.args(["--notify-hwnd", &parent_pid.to_string()]);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch {}: {e}", host_exe.display()))?;
    let _ = std::thread::Builder::new().name("tomoyo-wv-reaper".into()).spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_ephemeral_webview_ex(
    plugin_id: &str,
    title: &str,
    entry_path: &str,
    utools_compat: bool,
    initial_query: &str,
    width: u32,
    height: u32,
    _max_memory_mb: u32,
    is_dark: bool,
    icon_path: Option<&str>,
    _inline_hwnd: Option<isize>,
    _dock_rect: Option<(i32, i32, i32, i32)>,
) -> Result<u32, String> {
    if !plugin_id.is_empty() {
        let tag = sanitize_instance_tag(plugin_id);
        let jq = serde_json::to_string(initial_query).unwrap_or_else(|_| "\"\"".into());
        if try_wake_singleton_socket(&format!("plugin-{tag}"), &format!("activate_plugin:{jq}\n")) {
            return Ok(0);
        }
    }
    let host_exe = locate_sibling_exe("tomoyo-webview-host")?;
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
    if let Some(ip) = icon_path.filter(|s| !s.is_empty()) {
        cmd.args(["--icon", ip]);
    }
    if utools_compat {
        cmd.arg("--utools-compat");
    }
    if !is_dark {
        cmd.arg("--light");
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch {}: {e}", host_exe.display()))?;
    let pid = child.id();
    let _ = std::thread::Builder::new().name("tomoyo-wv-reaper".into()).spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

fn locate_sibling_exe(name: &str) -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
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
    Err(format!("{name} not found next to tomoyo"))
}

pub fn paste_into_previous_window_async(_prev_hwnd: isize) {
    let _ = std::thread::Builder::new().name("tomoyo-autopaste".into()).spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(85));
        if command_in_path("wtype") {
            let _ = Command::new("wtype").args(["-M", "ctrl", "v", "-m", "ctrl"]).status();
        } else {
            unsafe { tomoyo_linux_send_ctrl_chord(b'v' as c_char); }
        }
    });
}

pub fn send_ctrl_c_cleanly() {
    if command_in_path("wtype") {
        let _ = Command::new("wtype").args(["-M", "ctrl", "c", "-m", "ctrl"]).status();
    } else {
        unsafe { tomoyo_linux_send_ctrl_chord(b'c' as c_char); }
    }
}

pub fn shell_open(target: &str, args: Option<&str>, workdir: Option<&str>) -> bool {
    if target.ends_with(".desktop") && Path::new(target).is_file() {
        if command_in_path("gio")
            && spawn_isolated("gio", &["launch".into(), target.into()], workdir, Some(target))
        {
            return true;
        }
        if command_in_path("kioclient")
            && spawn_isolated("kioclient", &["exec".into(), target.into()], workdir, Some(target))
        {
            return true;
        }
        return CString::new(target)
            .is_ok_and(|cs| unsafe { tomoyo_linux_launch_desktop(cs.as_ptr()) != 0 });
    }

    let is_url = target.contains("://") || target.starts_with("mailto:");
    let use_xdg = is_url || (args.is_none() && !command_in_path(target));
    if use_xdg {
        spawn_isolated("xdg-open", &[target.to_string()], workdir, Some("xdg-open"))
    } else {
        let argv: Vec<String> = args
            .map(|a| a.split_whitespace().map(ToString::to_string).collect())
            .unwrap_or_default();
        spawn_isolated(target, &argv, workdir, Some(target))
    }
}

pub fn shell_open_as_admin(target: &str, args: Option<&str>, workdir: Option<&str>) -> bool {
    let mut argv = vec![target.to_string()];
    if let Some(a) = args {
        argv.extend(a.split_whitespace().map(ToString::to_string));
    }
    spawn_isolated("pkexec", &argv, workdir, Some(target))
}

pub fn reveal_in_file_manager(path: &str) -> bool {
    if tomoyo_core::linux_sys::dbus_reveal_file(path) {
        return true;
    }
    let parent = Path::new(path).parent().and_then(|p| p.to_str()).unwrap_or(path);
    shell_open(parent, None, None)
}

pub fn open_terminal_at(dir: &str) -> bool {
    let target_dir = if Path::new(dir).is_dir() { dir } else { "." };
    if let Ok(term) = std::env::var("TERMINAL") {
        let parts: Vec<String> = term.split_whitespace().map(ToString::to_string).collect();
        if let Some(bin) = parts.first()
            && spawn_isolated(bin, &parts[1..], Some(target_dir), Some(bin))
        {
            return true;
        }
    }
    for (bin, flag) in [
        ("konsole", Some("--workdir")),
        ("alacritty", Some("--working-directory")),
        ("kitty", Some("--directory")),
        ("wezterm", Some("start --cwd")),
        ("gnome-terminal", Some("--working-directory")),
        ("xfce4-terminal", Some("--working-directory")),
        ("xterm", None),
    ] {
        if !command_in_path(bin) {
            continue;
        }
        let mut argv: Vec<String> = flag
            .map(|f| f.split_whitespace().map(ToString::to_string).collect())
            .unwrap_or_default();
        if flag.is_some() {
            argv.push(target_dir.to_string());
        }
        if spawn_isolated(bin, &argv, Some(target_dir), Some(bin)) {
            return true;
        }
    }
    false
}

pub fn focus_window(token: isize) {
    if let Some(id) = tomoyo_providers::windows_switcher::resolve_linux_window_id(token) {
        let _ = tomoyo_core::linux_sys::dbus_krunner_run(&id);
    }
}

pub fn run_system_command(cmd: SystemCommandKind) {
    match cmd {
        SystemCommandKind::LockWorkstation => {
            if Command::new("loginctl").arg("lock-session").spawn().is_err() {
                let _ = Command::new("qdbus6")
                    .args(["org.freedesktop.ScreenSaver", "/ScreenSaver", "Lock"])
                    .spawn();
            }
        }
        SystemCommandKind::Sleep => {
            let _ = Command::new("systemctl").arg("suspend").spawn();
        }
        SystemCommandKind::EmptyRecycleBin => {
            let data = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
            if let Some(base) = data {
                for sub in ["Trash/files", "Trash/info"] {
                    let dir = base.join(sub);
                    if let Ok(rd) = std::fs::read_dir(&dir) {
                        for ent in rd.flatten() {
                            let p = ent.path();
                            if p.is_dir() {
                                let _ = std::fs::remove_dir_all(&p);
                            } else {
                                let _ = std::fs::remove_file(&p);
                            }
                        }
                    }
                }
            }
        }
        SystemCommandKind::OpenSettings => {
            if !shell_open("systemsettings", None, None) {
                let _ = shell_open("gnome-control-center", None, None);
            }
        }
        SystemCommandKind::OpenTaskManager => {
            let _ = ["plasma-systemmonitor", "ksysguard", "gnome-system-monitor"]
                .iter()
                .any(|b| spawn_isolated(b, &[], None, Some(b)));
        }
        SystemCommandKind::OpenTerminal => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            let _ = open_terminal_at(&home);
        }
    }
}

pub fn kill_process_by_pid(pid: u32) -> Result<(), String> {
    if pid <= 1 || pid == std::process::id() {
        return Err("拒绝结束受保护系统或自身进程".into());
    }
    if unsafe { kill(pid as i32, 15) } == 0 {
        Ok(())
    } else {
        Err(format!("无法结束进程 PID {pid}"))
    }
}
