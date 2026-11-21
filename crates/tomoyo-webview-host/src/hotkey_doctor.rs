//! Pure-user-mode Windows Global Hotkey Governance Engine ("Hotkey Doctor").
//!
//! 1. Audits Tomoyo's 4 configured hotkeys + 22 high-frequency shortcuts via side-effect-free
//!    Win32 `RegisterHotKey` + immediate `UnregisterHotKey` probing.
//! 2. Static Attribution: cross-references running processes, local config files (high confidence),
//!    and curated default-hotkey references (medium confidence).
//! 3. Live "Catch-in-the-Act" Probe: monitors foreground HWND, newly shown visible windows, AND
//!    per-process CPU wake deltas (`GetProcessTimes`) to expose even silent background occupiers.

#![cfg(windows)]

use crate::bridge_os::notify_parent_mode;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use tomoyo_core::{parse_hotkey, TomoyoConfig};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, SendInput, UnregisterHotKey, HOT_KEY_MODIFIERS, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
};

#[derive(Debug, Clone, Serialize)]
pub struct SuspectProcess {
    pub process_name: String,
    pub pid: u32,
    pub exe_path: String,
    pub reason: String,
    pub confidence: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct HotkeyAuditItem {
    pub spec: String,
    pub status: &'static str,
    pub tomoyo_slot: Option<String>,
    pub suspects: Vec<SuspectProcess>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HotkeyAuditReport {
    pub items: Vec<HotkeyAuditItem>,
    pub recommended_free: HashMap<String, Vec<String>>,
    pub total_probed: usize,
    pub external_occupied: usize,
    pub conflicts: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveProbeResult {
    pub spec: String,
    pub found: bool,
    pub pid: u32,
    pub process_name: String,
    pub window_title: String,
    pub exe_path: String,
    pub detail: String,
}

#[derive(Clone)]
struct ProcInfo {
    pid: u32,
    name: String,
    name_lower: String,
    exe_path: String,
}

const AUDIT_CANDIDATES: &[&str] = &[
    "F1", "F2", "F3", "F4", "PrintScreen",
    "Alt+Space", "Alt+A", "Alt+C", "Alt+Q", "Alt+S", "Alt+T", "Alt+V", "Alt+1", "Alt+2", "Alt+Z",
    "Ctrl+Alt+A", "Ctrl+Alt+V", "Ctrl+Shift+A", "Ctrl+Shift+S", "Ctrl+Shift+T", "Ctrl+Shift+V",
    "Ctrl+Shift+Space",
];

/// Curated reference table `(exe_substring_lower, display_label, default_hotkeys)`.
/// Only verified factory defaults are listed to prevent false attribution.
const KNOWN_SIGNATURES: &[(&str, &str, &[&str])] = &[
    ("wechat", "微信 (WeChat)", &["Alt+A", "Ctrl+Alt+W"]),
    ("weixin", "微信 (WeChat)", &["Alt+A", "Ctrl+Alt+W"]),
    ("qq.exe", "QQ", &["Ctrl+Alt+A", "Ctrl+Alt+Z"]),
    ("pixpin", "PixPin 截图", &["Ctrl+1", "F1", "Ctrl+2"]),
    ("snipaste", "Snipaste", &["F1", "F3", "Shift+F1"]),
    ("pot.exe", "Pot 翻译", &["Alt+T", "Alt+Q", "Alt+D"]),
    ("copyq", "CopyQ 剪贴板", &["Alt+V", "Ctrl+Shift+V"]),
    ("ditto", "Ditto 剪贴板", &["Ctrl+`", "Alt+V"]),
    ("utools", "uTools", &["Alt+Space"]),
    ("powertoys", "Microsoft PowerToys", &["Alt+Space", "Win+Shift+T", "Win+Shift+C"]),
    ("sharex", "ShareX", &["PrintScreen", "Ctrl+PrintScreen", "Alt+PrintScreen"]),
    ("flameshot", "Flameshot", &["PrintScreen"]),
    ("nvdisplay", "NVIDIA Overlay", &["Alt+Z", "Alt+R", "Alt+F1", "Alt+F9"]),
    ("nvidia share", "NVIDIA Overlay", &["Alt+Z", "Alt+R", "Alt+F1"]),
    ("igcc", "Intel 显卡控制中心", &["Ctrl+Alt+F12", "Ctrl+Alt+F1"]),
    ("radeonsoftware", "AMD Adrenalin", &["Alt+R", "Alt+Z"]),
    ("dingtalk", "钉钉", &["Ctrl+Shift+A", "Alt+A"]),
    ("feishu", "飞书", &["Ctrl+Shift+A", "Alt+A"]),
    ("lark", "Lark / 飞书", &["Ctrl+Shift+A"]),
    ("quicker", "Quicker", &["Alt+Q", "Ctrl+Shift+V"]),
    ("listary", "Listary", &["Alt+Space", "Ctrl+Space"]),
    ("youdao", "有道词典", &["Ctrl+Alt+D", "Alt+T"]),
    ("saladict", "沙拉查词", &["Alt+T", "Alt+C"]),
    ("screenclippinghost", "Windows 截图工具", &["PrintScreen"]),
    ("snippingtool", "Windows 截图工具", &["PrintScreen"]),
];

pub fn audit_system_hotkeys(parent_hwnd: Option<isize>) -> HotkeyAuditReport {
    let cfg = TomoyoConfig::load_or_create();
    let procs = snapshot_processes();
    let cfg_hits = scan_known_local_configs(&procs);

    let tomoyo_slots: Vec<(&str, String)> = vec![
        ("main", cfg.hotkey.trim().to_string()),
        ("clipboard", cfg.clipboard.hotkey.trim().to_string()),
        ("translate", cfg.translate.hotkey.trim().to_string()),
        ("capture", cfg.capture.hotkey.trim().to_string()),
    ];

    if let Some(p) = parent_hwnd {
        notify_parent_mode(p, 3);
        std::thread::sleep(std::time::Duration::from_millis(90));
    }

    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    for (_, s) in &tomoyo_slots {
        if !s.is_empty() && seen.insert(s.to_ascii_lowercase()) { ordered.push(s.clone()); }
    }
    for &c in AUDIT_CANDIDATES {
        if seen.insert(c.to_ascii_lowercase()) { ordered.push(c.to_string()); }
    }

    let mut items = Vec::with_capacity(ordered.len());
    let (mut external_occupied, mut conflicts) = (0usize, 0usize);

    for (idx, spec) in ordered.into_iter().enumerate() {
        let slot = tomoyo_slots
            .iter()
            .find(|(_, s)| !s.is_empty() && s.eq_ignore_ascii_case(&spec))
            .map(|(k, _)| (*k).to_string());
        let is_free = probe_register_free(&spec, 0x5400 + idx as i32);
        let (status, suspects) = if is_free {
            if slot.is_some() { ("tomoyo", Vec::new()) } else { ("free", Vec::new()) }
        } else {
            external_occupied += 1;
            let s = guess_suspects(&spec, &procs, &cfg_hits);
            if slot.is_some() {
                conflicts += 1;
                ("conflict", s)
            } else {
                ("occupied_external", s)
            }
        };
        items.push(HotkeyAuditItem { spec, status, tomoyo_slot: slot, suspects });
    }

    if let Some(p) = parent_hwnd {
        notify_parent_mode(p, 4);
    }

    let is_avail = |k: &str| items.iter().any(|it| it.spec.eq_ignore_ascii_case(k) && it.status == "free");
    let pick_free = |pool: &[&str]| -> Vec<String> {
        pool.iter().copied().filter(|k| is_avail(k)).take(4).map(ToString::to_string).collect()
    };
    let mut recommended_free = HashMap::new();
    recommended_free.insert("main".into(), pick_free(&["Alt+Space", "Ctrl+Shift+Space", "Alt+Q", "F2"]));
    recommended_free.insert("clipboard".into(), pick_free(&["Alt+V", "Ctrl+Shift+V", "Ctrl+Alt+V", "Alt+2", "F3"]));
    recommended_free.insert("translate".into(), pick_free(&["Ctrl+Shift+T", "Alt+T", "Alt+Q", "F4", "Alt+2"]));
    recommended_free.insert("capture".into(), pick_free(&["F1", "PrintScreen", "Alt+1", "Alt+S", "Ctrl+Shift+S", "Ctrl+Shift+A"]));

    HotkeyAuditReport { total_probed: items.len(), external_occupied, conflicts, items, recommended_free }
}

fn probe_register_free(spec: &str, id: i32) -> bool {
    let Some(parsed) = parse_hotkey(spec) else { return false; };
    unsafe {
        let ok = RegisterHotKey(None, id, HOT_KEY_MODIFIERS(parsed.modifiers), parsed.vk).is_ok();
        if ok { let _ = UnregisterHotKey(None, id); }
        ok
    }
}

fn snapshot_processes() -> Vec<ProcInfo> {
    let mut out = Vec::new();
    let self_pid = std::process::id();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out; };
        let mut pe = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let pid = pe.th32ProcessID;
                let end = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                let name = String::from_utf16_lossy(&pe.szExeFile[..end]);
                let lower = name.to_ascii_lowercase();
                if pid != self_pid && lower != "tomoyo.exe" && lower != "tomoyo-webview-host.exe" {
                    let exe_path = query_exe_path(pid).unwrap_or_default();
                    out.push(ProcInfo { pid, name, name_lower: lower, exe_path });
                }
                if Process32NextW(snap, &mut pe).is_err() { break; }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn query_exe_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if ok && len > 0 { Some(String::from_utf16_lossy(&buf[..len as usize])) } else { None }
    }
}

fn query_proc_cpu_100ns(pid: u32) -> Option<u64> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let ok = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok();
        let _ = CloseHandle(h);
        if !ok { return None; }
        let kt = ((k.dwHighDateTime as u64) << 32) | (k.dwLowDateTime as u64);
        let ut = ((u.dwHighDateTime as u64) << 32) | (u.dwLowDateTime as u64);
        Some(kt.saturating_add(ut))
    }
}

fn scan_known_local_configs(procs: &[ProcInfo]) -> Vec<(String, String, String)> {
    let mut hits = Vec::new();
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let candidates: &[(&str, &str, String)] = &[
        ("pot.exe", "Pot 本地配置", format!("{appdata}\\com.pot-app.desktop\\config.json")),
        ("copyq", "CopyQ 本地配置", format!("{appdata}\\copyq\\copyq-commands.ini")),
        ("pixpin", "PixPin 本地配置", format!("{appdata}\\PixPin\\Config\\config.ini")),
        ("snipaste", "Snipaste 本地配置", format!("{home}\\.snipaste\\config.ini")),
    ];
    for (proc_key, label, path) in candidates {
        if !procs.iter().any(|p| p.name_lower.contains(proc_key)) { continue; }
        if std::fs::metadata(path).is_ok_and(|m| m.len() <= 262_144) {
            if let Ok(raw) = std::fs::read_to_string(path) {
                hits.push(((*proc_key).to_string(), (*label).to_string(), raw.to_ascii_lowercase()));
            }
        }
    }
    hits
}

fn guess_suspects(spec: &str, procs: &[ProcInfo], cfg_hits: &[(String, String, String)]) -> Vec<SuspectProcess> {
    let mut out = Vec::new();
    let mut seen_pids = HashSet::new();
    let spec_lower = spec.to_ascii_lowercase();

    for (proc_key, label, content) in cfg_hits {
        if content.contains(&spec_lower) {
            if let Some(p) = procs.iter().find(|pr| pr.name_lower.contains(proc_key)) {
                if seen_pids.insert(p.pid) {
                    out.push(SuspectProcess {
                        process_name: p.name.clone(), pid: p.pid, exe_path: p.exe_path.clone(),
                        reason: format!("{label}命中 {spec}（配置文件实锤）"), confidence: "high",
                    });
                }
            }
        }
    }
    for &(key, label, defaults) in KNOWN_SIGNATURES {
        if !defaults.iter().any(|d| d.eq_ignore_ascii_case(spec)) { continue; }
        if let Some(p) = procs.iter().find(|pr| pr.name_lower.contains(key)) {
            if seen_pids.insert(p.pid) {
                out.push(SuspectProcess {
                    process_name: p.name.clone(), pid: p.pid, exe_path: p.exe_path.clone(),
                    reason: format!("{label} 常见默认键参考"), confidence: "medium",
                });
            }
        }
    }
    out
}

#[derive(Clone)]
struct WinSnap { pid: u32, title: String }

fn snapshot_visible_windows() -> HashMap<isize, WinSnap> {
    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let map = unsafe { &mut *(lparam.0 as *mut HashMap<isize, WinSnap>) };
        if unsafe { IsWindowVisible(hwnd).as_bool() } {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)); }
            if pid != 0 {
                let len = unsafe { GetWindowTextLengthW(hwnd) };
                let mut buf = vec![0u16; (len.max(0) as usize) + 1];
                let n = if len > 0 { (unsafe { GetWindowTextW(hwnd, &mut buf) }) as usize } else { 0 };
                map.insert(hwnd.0 as isize, WinSnap { pid, title: String::from_utf16_lossy(&buf[..n]) });
            }
        }
        windows::core::BOOL(1)
    }
    let mut map = HashMap::new();
    unsafe { let _ = EnumWindows(Some(enum_cb), LPARAM(&mut map as *mut _ as isize)); }
    map
}

pub fn probe_hotkey_owner_live(spec: &str, host_hwnd: usize, parent_hwnd: Option<isize>) -> LiveProbeResult {
    let Some(parsed) = parse_hotkey(spec) else {
        return LiveProbeResult {
            spec: spec.into(), found: false, pid: 0, process_name: String::new(),
            window_title: String::new(), exe_path: String::new(), detail: format!("无法解析快捷键 {spec}"),
        };
    };
    if let Some(p) = parent_hwnd {
        notify_parent_mode(p, 3);
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    let self_pid = std::process::id();
    let is_ignored = |pid: u32| -> bool {
        if pid == 0 || pid == self_pid { return true; }
        query_exe_path(pid).is_some_and(|p| {
            let l = p.to_ascii_lowercase();
            l.ends_with("\\tomoyo.exe") || l.ends_with("\\tomoyo-webview-host.exe") || l.ends_with("\\msedgewebview2.exe")
        })
    };
    let procs_before = snapshot_processes();
    let cpu_before: HashMap<u32, u64> = procs_before
        .iter()
        .filter(|p| !is_ignored(p.pid))
        .filter_map(|p| query_proc_cpu_100ns(p.pid).map(|t| (p.pid, t)))
        .collect();
    let before_fg = unsafe { GetForegroundWindow().0 as isize };
    let before_wins = snapshot_visible_windows();

    send_hotkey_chord(parsed.modifiers, parsed.vk);

    let mut caught: Option<(u32, String)> = None;
    for _ in 0..9 {
        std::thread::sleep(std::time::Duration::from_millis(55));
        let fg = unsafe { GetForegroundWindow() };
        if fg.0 as isize != 0 && fg.0 as isize != before_fg && fg.0 as isize != host_hwnd as isize {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)); }
            if !is_ignored(pid) {
                let len = unsafe { GetWindowTextLengthW(fg) };
                let mut buf = vec![0u16; (len.max(0) as usize) + 1];
                let n = if len > 0 { (unsafe { GetWindowTextW(fg, &mut buf) }) as usize } else { 0 };
                caught = Some((pid, String::from_utf16_lossy(&buf[..n])));
                break;
            }
        }
        for (h, snap) in snapshot_visible_windows() {
            if !before_wins.contains_key(&h) && h != host_hwnd as isize && !is_ignored(snap.pid) {
                caught = Some((snap.pid, snap.title));
                break;
            }
        }
        if caught.is_some() { break; }
    }

    if let Some(p) = parent_hwnd { notify_parent_mode(p, 4); }
    unsafe { let _ = SetForegroundWindow(HWND(host_hwnd as *mut c_void)); }

    if let Some((pid, title)) = caught {
        let exe_path = query_exe_path(pid).unwrap_or_default();
        let name = std::path::Path::new(&exe_path).file_name().and_then(|s| s.to_str()).map(ToString::to_string).unwrap_or_else(|| format!("PID {pid}"));
        return LiveProbeResult {
            spec: spec.into(), found: true, pid, process_name: name.clone(), window_title: title, exe_path,
            detail: format!("已捕获前台响应进程：{name} (PID {pid})"),
        };
    }

    // Fallback: detect silent background process CPU wakeup during the 495ms probe window
    let mut cpu_spikes: Vec<(&ProcInfo, u64)> = procs_before
        .iter()
        .filter(|p| !p.name_lower.contains("msedgewebview2") && !p.name_lower.contains("dwm.exe") && !p.name_lower.contains("explorer.exe"))
        .filter_map(|p| {
            let prev = *cpu_before.get(&p.pid)?;
            let now = query_proc_cpu_100ns(p.pid)?;
            let delta = now.saturating_sub(prev);
            (delta >= 120_000).then_some((p, delta)) // >= 12 ms CPU wakeup
        })
        .collect();
    cpu_spikes.sort_by(|a, b| b.1.cmp(&a.1));
    if let Some((top, delta)) = cpu_spikes.first() {
        return LiveProbeResult {
            spec: spec.into(), found: true, pid: top.pid, process_name: top.name.clone(),
            window_title: String::new(), exe_path: top.exe_path.clone(),
            detail: format!("未见弹窗，但触发瞬间后台进程 {} (PID {}) CPU 唤醒 +{}ms（静默占用嫌疑；也可直接开启上方「高优先级强行接管」）", top.name, top.pid, delta / 10_000),
        };
    }

    LiveProbeResult {
        spec: spec.into(), found: false, pid: 0, process_name: String::new(),
        window_title: String::new(), exe_path: String::new(),
        detail: "未检测到弹窗或明显后台唤醒（通常为无窗口系统服务/驱动或管理员权限进程静默占用；可直接开启上方「高优先级强行接管」立即恢复该键）".into(),
    }
}

fn send_hotkey_chord(mods: u32, vk: u32) {
    let mk = |code: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(code),
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                ..Default::default()
            },
        },
    };
    let mut mks = Vec::new();
    if (mods & 0x0002) != 0 { mks.push(VK_CONTROL.0); }
    if (mods & 0x0001) != 0 { mks.push(VK_MENU.0); }
    if (mods & 0x0004) != 0 { mks.push(VK_SHIFT.0); }
    if (mods & 0x0008) != 0 { mks.push(VK_LWIN.0); }

    let mut downs: Vec<INPUT> = mks.iter().map(|&m| mk(m, false)).collect();
    downs.push(mk(vk as u16, false));
    unsafe { SendInput(&downs, std::mem::size_of::<INPUT>() as i32); }
    std::thread::sleep(std::time::Duration::from_millis(40));

    let mut ups: Vec<INPUT> = vec![mk(vk as u16, true)];
    for &m in mks.iter().rev() { ups.push(mk(m, true)); }
    unsafe { SendInput(&ups, std::mem::size_of::<INPUT>() as i32); }
}

pub fn reveal_file_in_explorer(path: &str) {
    if !path.is_empty() && std::path::Path::new(path).exists() {
        let _ = std::process::Command::new("explorer.exe").arg(format!("/select,\"{path}\"")).spawn();
    }
}
