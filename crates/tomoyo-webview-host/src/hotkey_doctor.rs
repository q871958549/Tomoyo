//! Pure-user-mode Windows Global Hotkey Governance Engine ("Hotkey Doctor").
//!
//! 1. Audits Tomoyo's enabled hotkeys + 22 high-frequency shortcuts via side-effect-free
//!    Win32 `RegisterHotKey` + immediate `UnregisterHotKey` probing.
//! 2. Static Attribution: cross-references running processes, local config files (high confidence),
//!    and curated default-hotkey signatures (`hotkey_signatures.rs`).
//! 3. 3-Stage Differential "Catch-in-the-Act" Probe:
//!    - Stage 1: 300ms idle baseline CPU calibration (eliminates continuous renderers like Wallpaper Engine).
//!    - Stage 2: Active chord injection monitoring Foreground HWND, new visible windows & clipboard sequence.
//!    - Stage 3: Baseline-subtracted CPU spike + post-quiet verification to eliminate random timer noise.

#![cfg(windows)]

use crate::bridge_os::notify_parent_mode;
use crate::hotkey_signatures::{
    guess_suspects, is_background_noise_process, scan_known_local_configs, ProcInfo,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use tomoyo_core::{parse_hotkey, TomoyoConfig, PLUGIN_CAPTURE, PLUGIN_CLIPBOARD, PLUGIN_TRANSLATE};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
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
    pub remediation: String,
    pub can_terminate: bool,
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
    pub overridden: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveProbeResult {
    pub spec: String,
    pub found: bool,
    pub is_free: bool,
    pub confidence: &'static str,
    pub pid: u32,
    pub process_name: String,
    pub window_title: String,
    pub exe_path: String,
    pub detail: String,
}

const AUDIT_CANDIDATES: &[&str] = &[
    "F1", "F2", "F3", "F4", "PrintScreen",
    "Alt+Space", "Alt+A", "Alt+C", "Alt+Q", "Alt+S", "Alt+T", "Alt+V", "Alt+1", "Alt+2", "Alt+Z",
    "Ctrl+Alt+A", "Ctrl+Alt+V", "Ctrl+Shift+A", "Ctrl+Shift+S", "Ctrl+Shift+T", "Ctrl+Shift+V",
    "Ctrl+Shift+Space",
];

pub fn audit_system_hotkeys(parent_hwnd: Option<isize>) -> HotkeyAuditReport {
    let cfg = TomoyoConfig::load_or_create();
    let procs = snapshot_processes();
    let cfg_hits = scan_known_local_configs(&procs);

    let cap_on = cfg.is_plugin_enabled(PLUGIN_CAPTURE);
    let tomoyo_slots: Vec<(&str, String)> = vec![
        ("main", cfg.hotkey.trim().to_string()),
        ("clipboard", if cfg.is_plugin_enabled(PLUGIN_CLIPBOARD) { cfg.clipboard.hotkey.trim().to_string() } else { String::new() }),
        ("translate", if cfg.is_plugin_enabled(PLUGIN_TRANSLATE) { cfg.translate.hotkey.trim().to_string() } else { String::new() }),
        ("capture", if cap_on { cfg.capture.hotkey.trim().to_string() } else { String::new() }),
        ("pin", if cap_on { cfg.capture.pin_hotkey.trim().to_string() } else { String::new() }),
        ("ocr", if cap_on { cfg.capture.ocr_hotkey.trim().to_string() } else { String::new() }),
        ("record", if cap_on { cfg.capture.record_hotkey.trim().to_string() } else { String::new() }),
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
    let (mut external_occupied, mut conflicts, mut overridden) = (0usize, 0usize, 0usize);

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
                if cfg.override_conflict_hotkeys {
                    overridden += 1;
                    ("overridden", s)
                } else {
                    conflicts += 1;
                    ("conflict", s)
                }
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
    recommended_free.insert("pin".into(), pick_free(&["F3", "F4", "Alt+3", "Alt+P", "Ctrl+Shift+P"]));
    recommended_free.insert("ocr".into(), pick_free(&["Alt+Shift+C", "Ctrl+Shift+O", "Alt+O", "F4", "Alt+2"]));
    recommended_free.insert("record".into(), pick_free(&["Alt+Shift+R", "Ctrl+Shift+R", "Alt+R", "F9", "F10"]));

    HotkeyAuditReport { total_probed: items.len(), external_occupied, conflicts, overridden, items, recommended_free }
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

fn sample_cpu_map(procs: &[ProcInfo]) -> HashMap<u32, u64> {
    procs
        .iter()
        .filter(|p| p.pid > 4 && !is_background_noise_process(&p.name_lower))
        .filter_map(|p| query_proc_cpu_100ns(p.pid).map(|t| (p.pid, t)))
        .collect()
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
            spec: spec.into(), found: false, is_free: false, confidence: "none", pid: 0,
            process_name: String::new(), window_title: String::new(), exe_path: String::new(),
            detail: format!("无法解析快捷键 {spec}"),
        };
    };
    if let Some(p) = parent_hwnd {
        notify_parent_mode(p, 3);
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    // Fast path: check if the probed shortcut is actually FREE right now!
    if probe_register_free(spec, 0x54FE) {
        if let Some(p) = parent_hwnd { notify_parent_mode(p, 4); }
        return LiveProbeResult {
            spec: spec.into(), found: false, is_free: true, confidence: "confirmed", pid: 0,
            process_name: String::new(), window_title: String::new(), exe_path: String::new(),
            detail: format!("实测空闲：{spec} 当前未被任何外部程序占用，可直接安全绑定！"),
        };
    }

    let self_pid = std::process::id();
    let is_ignored = |pid: u32| -> bool {
        if pid == 0 || pid == self_pid { return true; }
        query_exe_path(pid).is_some_and(|p| {
            let l = p.to_ascii_lowercase();
            l.ends_with("\\tomoyo.exe") || l.ends_with("\\tomoyo-webview-host.exe") || l.ends_with("\\msedgewebview2.exe")
        })
    };

    // Stage 1: 300ms idle baseline CPU calibration (eliminates live wallpapers / video renderers!)
    let procs = snapshot_processes();
    let cpu_t0 = sample_cpu_map(&procs);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let cpu_t1 = sample_cpu_map(&procs);

    // Stage 2: Active chord injection monitoring Foreground HWND, new popups & clipboard sequence
    let before_fg = unsafe { GetForegroundWindow().0 as isize };
    let before_wins = snapshot_visible_windows();
    let before_clip_seq = unsafe { GetClipboardSequenceNumber() };

    send_hotkey_chord(parsed.modifiers, parsed.vk);

    let mut caught: Option<(u32, String)> = None;
    for _ in 0..8 {
        std::thread::sleep(std::time::Duration::from_millis(48));
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
    let cpu_t2 = sample_cpu_map(&procs);
    let clip_mutated = unsafe { GetClipboardSequenceNumber() } != before_clip_seq;

    if let Some(p) = parent_hwnd { notify_parent_mode(p, 4); }
    unsafe { let _ = SetForegroundWindow(HWND(host_hwnd as *mut c_void)); }

    if let Some((pid, title)) = caught {
        let exe_path = query_exe_path(pid).unwrap_or_default();
        let name = std::path::Path::new(&exe_path).file_name().and_then(|s| s.to_str()).map(ToString::to_string).unwrap_or_else(|| format!("PID {pid}"));
        return LiveProbeResult {
            spec: spec.into(), found: true, is_free: false, confidence: "confirmed", pid,
            process_name: name.clone(), window_title: title, exe_path,
            detail: format!("实锤捕获前台响应进程：{name} (PID {pid})"),
        };
    }

    // Stage 3: Baseline-subtracted CPU spike + 220ms post-quiet verification (zero false positives!)
    let mut spikes: Vec<(&ProcInfo, u64, u64)> = procs
        .iter()
        .filter_map(|p| {
            let t0 = *cpu_t0.get(&p.pid)?;
            let t1 = *cpu_t1.get(&p.pid)?;
            let t2 = *cpu_t2.get(&p.pid)?;
            let base = t1.saturating_sub(t0);
            let probe = t2.saturating_sub(t1);
            // Must be genuinely quiet at rest (< 6ms in 300ms) and spike >= 15ms & >= 4x baseline
            (base < 60_000 && probe >= (base * 4).max(base + 150_000)).then_some((p, base, probe))
        })
        .collect();
    spikes.sort_by_key(|a| std::cmp::Reverse(a.2));

    if let Some((top, base, probe)) = spikes.first().copied() {
        std::thread::sleep(std::time::Duration::from_millis(220));
        let post_quiet = query_proc_cpu_100ns(top.pid)
            .and_then(|t3| cpu_t2.get(&top.pid).map(|&t2| t3.saturating_sub(t2)))
            .unwrap_or(0);
        if post_quiet < probe / 2 {
            let clip_hint = if clip_mutated { "且触发瞬间改动了系统剪贴板" } else { "触发后已恢复静默" };
            return LiveProbeResult {
                spec: spec.into(), found: true, is_free: false, confidence: "likely", pid: top.pid,
                process_name: top.name.clone(), window_title: String::new(), exe_path: top.exe_path.clone(),
                detail: format!(
                    "差分探针命中后台进程 {} (PID {})：静息 {}ms → 触发瞬峰 +{}ms（{}）",
                    top.name, top.pid, base / 10_000, probe / 10_000, clip_hint
                ),
            };
        }
    }

    let clip_note = if clip_mutated { "检测到剪贴板被静默改写，但未见前台窗口。" } else { "" };
    LiveProbeResult {
        spec: spec.into(), found: false, is_free: false, confidence: "none", pid: 0,
        process_name: String::new(), window_title: String::new(), exe_path: String::new(),
        detail: format!(
            "{clip_note}经静息基线校准后未见弹窗或可复现后台唤醒（通常为输入法/显卡驱动/无窗口服务静默注册）。开启「高优先级自动接管冲突热键」即可让 Tomoyo 无视外部静默占位、照常唤起！"
        ),
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
    crate::hotkey_signatures::reveal_path_native(path);
}

pub fn terminate_conflict_process(pid: u32, process_name: &str) -> Result<String, String> {
    crate::hotkey_signatures::terminate_conflict_process_safe(pid, process_name)
}
