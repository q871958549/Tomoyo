//! Win32 global hotkey parser and Windows login autostart registration.

/// Parsed Win32 hotkey descriptor `(mod_flags, vk_code)`.
/// Bit flags match Win32: ALT=0x0001, CONTROL=0x0002, SHIFT=0x0004, WIN=0x0008, NOREPEAT=0x4000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedHotkey {
    pub modifiers: u32,
    pub vk: u32,
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
            s if s.len() == 1 => {
                let b = s.as_bytes()[0].to_ascii_uppercase();
                if b.is_ascii_alphanumeric() {
                    vk = Some(b as u32);
                }
            }
            s if s.starts_with('f') && s.len() <= 3 => {
                if let Ok(n) = s[1..].parse::<u32>() {
                    if (1..=12).contains(&n) {
                        vk = Some(0x6F + n); // VK_F1 = 0x70
                    }
                }
            }
            _ => return None,
        }
    }

    let vk = vk?;
    if (mods & 0x000F) == 0 {
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

        assert!(parse_hotkey("Space").is_none());
    }
}
