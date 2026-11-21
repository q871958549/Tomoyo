//! Win32 global hotkey parser, live registration health persistence, and Windows login autostart.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Parsed Win32 hotkey descriptor `(mod_flags, vk_code)`.
/// Bit flags match Win32: ALT=0x0001, CONTROL=0x0002, SHIFT=0x0004, WIN=0x0008, NOREPEAT=0x4000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedHotkey {
    pub modifiers: u32,
    pub vk: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HotkeySlotStatus {
    pub spec: String,
    pub active: String,
    pub ok: bool,
    pub disabled: bool,
    #[serde(default)]
    pub overridden: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HotkeyHealthReport {
    pub main: HotkeySlotStatus,
    pub clipboard: HotkeySlotStatus,
    pub translate: HotkeySlotStatus,
    pub capture: HotkeySlotStatus,
    #[serde(default)]
    pub pin: HotkeySlotStatus,
    pub updated_at_unix: u64,
}

pub fn hotkey_health_path() -> PathBuf {
    crate::data_dir().join("hotkey_status.json")
}

pub fn save_hotkey_health(report: &HotkeyHealthReport) {
    let path = hotkey_health_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(report) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub fn load_hotkey_health() -> HotkeyHealthReport {
    std::fs::read_to_string(hotkey_health_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<HotkeyHealthReport>(&raw).ok())
        .unwrap_or_default()
}

/// Keys that Win32 `RegisterHotKey` may safely bind without Ctrl/Alt/Shift/Win modifiers.
pub fn is_standalone_allowed_vk(vk: u32) -> bool {
    (0x70..=0x7B).contains(&vk) || vk == 0x2C
}

/// Converts a Win32 virtual-key code into Tomoyo's canonical hotkey token name.
pub fn vk_to_key_name(vk: u32) -> Option<String> {
    let s = match vk {
        0x20 => "Space",
        0x09 => "Tab",
        0x0D => "Enter",
        0x2D => "Insert",
        0x2E => "Delete",
        0x24 => "Home",
        0x23 => "End",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x2C => "PrintScreen",
        0x26 => "Up",
        0x28 => "Down",
        0x25 => "Left",
        0x27 => "Right",
        0xC0 => "`",
        0xBD => "-",
        0xBB => "=",
        0xDB => "[",
        0xDD => "]",
        0xDC => "\\",
        0xBA => ";",
        0xDE => "'",
        0xBC => ",",
        0xBE => ".",
        0xBF => "/",
        0x30..=0x39 | 0x41..=0x5A => return Some((vk as u8 as char).to_string()),
        0x70..=0x7B => return Some(format!("F{}", vk - 0x6F)),
        _ => return None,
    };
    Some(s.to_string())
}

/// Formats Win32 modifier bits (`ALT=1, CTRL=2, SHIFT=4, WIN=8`) and a VK code into canonical spec.
pub fn format_hotkey(mods: u32, vk: u32) -> Option<String> {
    let key = vk_to_key_name(vk)?;
    let has_mod = (mods & 0x000F) != 0;
    if !has_mod && !is_standalone_allowed_vk(vk) {
        return None;
    }
    let mut parts: Vec<String> = Vec::with_capacity(5);
    if (mods & 0x0002) != 0 { parts.push("Ctrl".into()); }
    if (mods & 0x0001) != 0 { parts.push("Alt".into()); }
    if (mods & 0x0004) != 0 { parts.push("Shift".into()); }
    if (mods & 0x0008) != 0 { parts.push("Win".into()); }
    parts.push(key);
    Some(parts.join("+"))
}

pub fn parse_hotkey(spec: &str) -> Option<ParsedHotkey> {
    let mut mods = 0x4000u32; // MOD_NOREPEAT
    let mut vk: Option<u32> = None;

    for raw_part in spec.split('+') {
        let part = raw_part.trim().to_ascii_lowercase();
        match part.as_str() {
            "alt" => mods |= 0x0001,
            "ctrl" | "control" => mods |= 0x0002,
            "shift" => mods |= 0x0004,
            "win" | "super" | "meta" => mods |= 0x0008,
            "space" => vk = Some(0x20),
            "tab" => vk = Some(0x09),
            "enter" | "return" => vk = Some(0x0D),
            "insert" | "ins" => vk = Some(0x2D),
            "delete" | "del" => vk = Some(0x2E),
            "home" => vk = Some(0x24),
            "end" => vk = Some(0x23),
            "pageup" | "pgup" => vk = Some(0x21),
            "pagedown" | "pgdn" => vk = Some(0x22),
            "printscreen" | "prtsc" => vk = Some(0x2C),
            "up" => vk = Some(0x26),
            "down" => vk = Some(0x28),
            "left" => vk = Some(0x25),
            "right" => vk = Some(0x27),
            "`" | "~" | "backquote" => vk = Some(0xC0),
            "-" | "minus" => vk = Some(0xBD),
            "=" | "equal" => vk = Some(0xBB),
            "[" | "bracketleft" => vk = Some(0xDB),
            "]" | "bracketright" => vk = Some(0xDD),
            "\\" | "backslash" => vk = Some(0xDC),
            ";" | "semicolon" => vk = Some(0xBA),
            "'" | "quote" => vk = Some(0xDE),
            "," | "comma" => vk = Some(0xBC),
            "." | "period" => vk = Some(0xBE),
            "/" | "slash" => vk = Some(0xBF),
            s if s.len() == 1 => {
                let b = s.as_bytes()[0].to_ascii_uppercase();
                if b.is_ascii_alphanumeric() {
                    vk = Some(b as u32);
                } else {
                    return None;
                }
            }
            s if s.starts_with('f') && s.len() <= 3 => {
                if let Ok(n) = s[1..].parse::<u32>() {
                    if (1..=12).contains(&n) {
                        vk = Some(0x6F + n); // VK_F1 = 0x70
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }

    let vk = vk?;
    if (mods & 0x000F) == 0 && !is_standalone_allowed_vk(vk) {
        return None;
    }
    Some(ParsedHotkey { modifiers: mods, vk })
}

/// Checks whether Tomoyo is registered in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
#[cfg(windows)]
pub fn is_autostart_enabled() -> bool {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };

    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return false;
        }
        let exists = RegQueryValueExW(hkey, w!("Tomoyo"), None, None, None, None).is_ok();
        let _ = RegCloseKey(hkey);
        let _ = PCWSTR::null();
        exists
    }
}

#[cfg(not(windows))]
pub fn is_autostart_enabled() -> bool {
    false
}

/// Enables or disables automatic launch at Windows login (`--silent` mode).
#[cfg(windows)]
pub fn set_autostart_enabled(enable: bool) -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };

    unsafe {
        let mut hkey = HKEY::default();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            Some(0),
            KEY_SET_VALUE,
            &mut hkey,
        )
        .ok()
        .map_err(|e| format!("RegOpenKeyExW failed: {e}"))?;

        let res = if enable {
            let cur = std::env::current_exe().map_err(|e| e.to_string())?;
            // Ensure toggling autostart from `tomoyo-webview-host.exe` registers `tomoyo.exe`!
            let sibling_launcher = cur.with_file_name("tomoyo.exe");
            let exe = if sibling_launcher.is_file() {
                sibling_launcher
            } else {
                cur
            }
            .to_string_lossy()
            .into_owned();
            let cmd = format!("\"{exe}\" --silent");
            let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            RegSetValueExW(hkey, w!("Tomoyo"), Some(0), REG_SZ, Some(bytes))
                .ok()
                .map_err(|e| format!("RegSetValueExW failed: {e}"))
        } else {
            let _ = RegDeleteValueW(hkey, w!("Tomoyo"));
            Ok(())
        };

        let _ = RegCloseKey(hkey);
        res
    }
}

#[cfg(not(windows))]
pub fn set_autostart_enabled(_enable: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hotkeys_accurately() {
        let h = parse_hotkey("Alt+Space").unwrap();
        assert_eq!(h.modifiers, 0x4001);
        assert_eq!(h.vk, 0x20);

        let h2 = parse_hotkey("Ctrl+Shift+K").unwrap();
        assert_eq!(h2.modifiers, 0x4006);
        assert_eq!(h2.vk, b'K' as u32);

        let h3 = parse_hotkey("Alt+`").unwrap();
        assert_eq!(h3.vk, 0xC0);
        let h4 = parse_hotkey("Ctrl+Shift+PrintScreen").unwrap();
        assert_eq!(h4.vk, 0x2C);

        let f1 = parse_hotkey("F1").unwrap();
        assert_eq!(f1.modifiers, 0x4000);
        assert_eq!(f1.vk, 0x70);
        let prt = parse_hotkey("PrintScreen").unwrap();
        assert_eq!(prt.modifiers, 0x4000);
        assert_eq!(prt.vk, 0x2C);
        assert_eq!(format_hotkey(0, 0x70).as_deref(), Some("F1"));
        assert_eq!(format_hotkey(0x0003, b'C' as u32).as_deref(), Some("Ctrl+Alt+C"));
        assert!(format_hotkey(0, b'A' as u32).is_none());

        assert!(parse_hotkey("Space").is_none());
        assert!(parse_hotkey("A").is_none());
    }
}
