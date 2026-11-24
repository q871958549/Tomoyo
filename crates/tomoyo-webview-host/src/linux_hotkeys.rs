//! Linux Global Hotkey Governance & GDK key capture ("Hotkey Doctor" via KGlobalAccel D-Bus).

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_int, CStr};
use tomoyo_core::{parse_hotkey, TomoyoConfig, PLUGIN_CAPTURE, PLUGIN_CLIPBOARD, PLUGIN_TRANSLATE};

unsafe extern "C" {
    fn tomoyo_kga_query_owner(
        qt_key: c_int,
        comp_out: *mut c_char,
        comp_len: usize,
        friendly_out: *mut c_char,
        fr_len: usize,
    ) -> c_int;
    fn tomoyo_kga_block_global(block: c_int);
}

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

pub fn set_global_shortcuts_blocked(block: bool) {
    unsafe { tomoyo_kga_block_global(c_int::from(block)); }
}

pub fn spec_to_qt_key(spec: &str) -> Option<i32> {
    let p = parse_hotkey(spec)?;
    let mut qt: i32 = 0;
    if (p.modifiers & 0x0004) != 0 { qt |= 0x0200_0000; } // Qt::SHIFT
    if (p.modifiers & 0x0002) != 0 { qt |= 0x0400_0000; } // Qt::CTRL
    if (p.modifiers & 0x0001) != 0 { qt |= 0x0800_0000; } // Qt::ALT
    if (p.modifiers & 0x0008) != 0 { qt |= 0x1000_0000; } // Qt::META
    let key_code: i32 = match p.vk {
        0x20 => 0x20,
        0x30..=0x39 | 0x41..=0x5A => p.vk as i32,
        0x09 => 0x0100_0001,
        0x0D => 0x0100_0004,
        0x2D => 0x0100_0006,
        0x2E => 0x0100_0007,
        0x2C => 0x0100_0009,
        0x24 => 0x0100_0010,
        0x23 => 0x0100_0011,
        0x25 => 0x0100_0012,
        0x26 => 0x0100_0013,
        0x27 => 0x0100_0014,
        0x28 => 0x0100_0015,
        0x21 => 0x0100_0016,
        0x22 => 0x0100_0017,
        0x70..=0x7B => 0x0100_0030 + (p.vk as i32 - 0x70),
        0xC0 => 0x60,
        0xBD => 0x2D,
        0xBB => 0x3D,
        0xDB => 0x5B,
        0xDD => 0x5D,
        0xDC => 0x5C,
        0xBA => 0x3B,
        0xDE => 0x27,
        0xBC => 0x2C,
        0xBE => 0x2E,
        0xBF => 0x2F,
        _ => return None,
    };
    Some(qt | key_code)
}

fn query_kga_owner(spec: &str) -> Option<(String, String)> {
    let qt = spec_to_qt_key(spec)?;
    let mut comp = [0 as c_char; 160];
    let mut fr = [0 as c_char; 160];
    let rc = unsafe { tomoyo_kga_query_owner(qt, comp.as_mut_ptr(), comp.len(), fr.as_mut_ptr(), fr.len()) };
    if rc == 1 {
        let c = unsafe { CStr::from_ptr(comp.as_ptr()) }.to_string_lossy().into_owned();
        let f = unsafe { CStr::from_ptr(fr.as_ptr()) }.to_string_lossy().into_owned();
        Some((c, f))
    } else {
        None
    }
}

pub fn audit_system_hotkeys() -> HotkeyAuditReport {
    let cfg = TomoyoConfig::load_or_create();
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

    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    for (_, s) in &tomoyo_slots {
        if !s.is_empty() && seen.insert(s.to_ascii_lowercase()) { ordered.push(s.clone()); }
    }
    for &c in AUDIT_CANDIDATES {
        if seen.insert(c.to_ascii_lowercase()) { ordered.push(c.to_string()); }
    }

    let mut items = Vec::with_capacity(ordered.len());
    let (mut external_occupied, mut conflicts, overridden) = (0usize, 0usize, 0usize);

    for spec in ordered {
        let slot = tomoyo_slots
            .iter()
            .find(|(_, s)| !s.is_empty() && s.eq_ignore_ascii_case(&spec))
            .map(|(k, _)| (*k).to_string());
        let owner = query_kga_owner(&spec);
        let owned_by_tomoyo = owner.as_ref().is_some_and(|(c, _)| c == "tomoyo");
        let (status, suspects) = match owner {
            None => (if slot.is_some() { "tomoyo" } else { "free" }, Vec::new()),
            Some(_) if owned_by_tomoyo => ("tomoyo", Vec::new()),
            Some((comp, friendly)) => {
                external_occupied += 1;
                let s = vec![SuspectProcess {
                    process_name: friendly.clone(),
                    pid: 0,
                    exe_path: String::new(),
                    reason: format!("KDE KGlobalAccel 已绑定至组件 `{comp}` ({friendly})"),
                    confidence: "high",
                    remediation: "可在「系统设置 -> 快捷键」解绑或为 Tomoyo 换用空闲热键".into(),
                    can_terminate: false,
                }];
                if slot.is_some() {
                    conflicts += 1;
                    ("conflict", s)
                } else {
                    ("occupied_external", s)
                }
            }
        };
        items.push(HotkeyAuditItem { spec, status, tomoyo_slot: slot, suspects });
    }

    let is_avail = |k: &str| items.iter().any(|it| it.spec.eq_ignore_ascii_case(k) && it.status == "free");
    let pick_free = |pool: &[&str]| -> Vec<String> {
        pool.iter().copied().filter(|k| is_avail(k)).take(4).map(ToString::to_string).collect()
    };
    let mut recommended_free = HashMap::new();
    recommended_free.insert("main".into(), pick_free(&["Ctrl+Space", "Super+Space", "Ctrl+Shift+Space", "Alt+Q", "F2"]));
    recommended_free.insert("clipboard".into(), pick_free(&["Alt+V", "Ctrl+Shift+V", "Ctrl+Alt+V", "Alt+2", "F3"]));
    recommended_free.insert("translate".into(), pick_free(&["Ctrl+Shift+T", "Alt+T", "Alt+Q", "F4", "Alt+2"]));
    recommended_free.insert("capture".into(), pick_free(&["F1", "PrintScreen", "Alt+1", "Alt+S", "Ctrl+Shift+S", "Ctrl+Shift+A"]));
    recommended_free.insert("pin".into(), pick_free(&["F3", "F4", "Alt+3", "Alt+P", "Ctrl+Shift+P"]));
    recommended_free.insert("ocr".into(), pick_free(&["Alt+Shift+C", "Ctrl+Shift+O", "Alt+O", "F4", "Alt+2"]));
    recommended_free.insert("record".into(), pick_free(&["Alt+Shift+R", "Ctrl+Shift+R", "Alt+R", "F9", "F10"]));

    HotkeyAuditReport { total_probed: items.len(), external_occupied, conflicts, overridden, items, recommended_free }
}

pub fn probe_hotkey_owner_live(spec: &str) -> LiveProbeResult {
    match query_kga_owner(spec) {
        None => LiveProbeResult {
            spec: spec.into(),
            found: false,
            is_free: true,
            confidence: "high",
            pid: 0,
            process_name: String::new(),
            window_title: String::new(),
            exe_path: String::new(),
            detail: format!("`{spec}` 当前未被 KDE KGlobalAccel 占用，可直接绑定。"),
        },
        Some((comp, friendly)) => LiveProbeResult {
            spec: spec.into(),
            found: true,
            is_free: false,
            confidence: "high",
            pid: 0,
            process_name: friendly.clone(),
            window_title: comp.clone(),
            exe_path: String::new(),
            detail: format!("`{spec}` 当前由 KDE 组件 `{comp}` ({friendly}) 注册。"),
        },
    }
}

pub fn terminate_conflict_process(pid: u32, process_name: &str) -> Result<String, String> {
    if pid <= 1 || pid == std::process::id() {
        return Err("拒绝结束关键系统进程或自身进程".into());
    }
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    const SIGTERM: i32 = 15;
    if unsafe { kill(pid as i32, SIGTERM) } == 0 {
        Ok(format!("已发送 SIGTERM 结束进程 {process_name} (PID {pid})"))
    } else {
        Err(format!("无法结束进程 {process_name} (PID {pid})"))
    }
}

/// Converts a GDK `keyval` + modifier mask into Tomoyo's canonical hotkey string.
pub fn format_gdk_hotkey(keyval: u32, gdk_state: u32) -> Option<String> {
    let shift = (gdk_state & (1 << 0)) != 0;
    let ctrl = (gdk_state & (1 << 2)) != 0;
    let alt = (gdk_state & (1 << 3)) != 0;
    let super_k = (gdk_state & (1 << 26)) != 0 || (gdk_state & (1 << 6)) != 0;
    let key = match keyval {
        0x0020 => "Space".to_string(),
        0xFF09 | 0xFE20 => "Tab".to_string(),
        0xFF0D => "Enter".to_string(),
        0xFF63 => "Insert".to_string(),
        0xFFFF => "Delete".to_string(),
        0xFF50 => "Home".to_string(),
        0xFF57 => "End".to_string(),
        0xFF55 => "PageUp".to_string(),
        0xFF56 => "PageDown".to_string(),
        0xFF61 | 0xFD1D => "PrintScreen".to_string(),
        0xFF51 => "Left".to_string(),
        0xFF52 => "Up".to_string(),
        0xFF53 => "Right".to_string(),
        0xFF54 => "Down".to_string(),
        0xFFBE..=0xFFC9 => format!("F{}", keyval - 0xFFBD),
        0x0030..=0x0039 => ((keyval as u8) as char).to_string(),
        0x0041..=0x005A => ((keyval as u8) as char).to_string(),
        0x0061..=0x007A => (((keyval - 32) as u8) as char).to_string(),
        0x0060 | 0x007E => "`".to_string(),
        0x002D | 0x005F => "-".to_string(),
        0x003D | 0x002B => "=".to_string(),
        0x005B | 0x007B => "[".to_string(),
        0x005D | 0x007D => "]".to_string(),
        0x005C | 0x007C => "\\".to_string(),
        0x003B | 0x003A => ";".to_string(),
        0x0027 | 0x0022 => "'".to_string(),
        0x002C | 0x003C => ",".to_string(),
        0x002E | 0x003E => ".".to_string(),
        0x002F | 0x003F => "/".to_string(),
        _ => return None,
    };
    let has_mod = ctrl || alt || shift || super_k;
    let is_fn = key.starts_with('F') || key == "PrintScreen";
    if !has_mod && !is_fn {
        return None;
    }
    let mut parts = Vec::new();
    if ctrl { parts.push("Ctrl".into()); }
    if alt { parts.push("Alt".into()); }
    if shift { parts.push("Shift".into()); }
    if super_k { parts.push("Win".into()); }
    parts.push(key);
    Some(parts.join("+"))
}
