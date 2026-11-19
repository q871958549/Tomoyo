//! Hot-reloadable `config.toml`, hotkey parser, and Windows login autostart management.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomoyoConfig {
    /// UI language: `"zh"` (default, Simplified Chinese) or `"en"` (English).
    #[serde(default = "default_language")]
    pub language: String,
    /// Global summon shortcut, e.g. `"Alt+Space"`, `"Ctrl+Space"`, `"Ctrl+Shift+Space"`.
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// UI theme preset: `"dark"`, `"light"`, or `"midnight"`.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Seconds hidden before dropping DIB surfaces, icons, SQLite cache & trimming Working Set.
    #[serde(default = "default_hibernate_secs")]
    pub hibernate_delay_secs: u32,
    /// Show right-side live preview pane automatically when an item supports preview.
    #[serde(default = "default_true")]
    pub auto_preview: bool,
    /// Persistent window dimensions and optional user-dragged position.
    #[serde(default)]
    pub window: WindowConfig,
    /// Everything search customization.
    #[serde(default)]
    pub everything: EverythingConfig,
    /// Clipboard persistence settings.
    #[serde(default)]
    pub clipboard: ClipboardConfig,
    /// Auto-update check settings (GitHub Releases).
    #[serde(default)]
    pub update: UpdateConfig,
    /// Decentralized plugin registry sources (local path or HTTPS JSON URL).
    #[serde(default = "default_plugin_registries")]
    pub plugin_registries: Vec<String>,
    /// Customizable quick web search engines (`g rust`, `gh tokio`, `crates wasmi`, ...).
    #[serde(default = "default_web_searches")]
    pub web_searches: Vec<WebSearchEngine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    #[serde(default = "default_true")]
    pub auto_check: bool,
    #[serde(default = "default_update_interval")]
    pub check_interval_hours: u64,
    #[serde(default = "default_update_repo")]
    pub repo: String,
    #[serde(default)]
    pub last_checked_unix: u64,
    #[serde(default)]
    pub skipped_version: Option<String>,
}

fn default_update_interval() -> u64 {
    24
}
fn default_update_repo() -> String {
    "q871958549/Tomoyo".into()
}
fn default_plugin_registries() -> Vec<String> {
    vec![
        "registry/index.json".into(),
        "https://raw.githubusercontent.com/q871958549/Tomoyo/main/registry/index.json".into(),
    ]
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            auto_check: true,
            check_interval_hours: default_update_interval(),
            repo: default_update_repo(),
            last_checked_unix: 0,
            skipped_version: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    /// Main list pane width in DIPs (default 720, range 520..=1400).
    #[serde(default = "default_win_width")]
    pub width_dip: i32,
    /// Maximum visible result rows before scrolling (default 8, range 4..=18).
    #[serde(default = "default_max_rows")]
    pub max_visible_rows: usize,
    /// Right-side live preview pane width in DIPs (default 310, range 220..=620).
    #[serde(default = "default_preview_width")]
    pub preview_width_dip: i32,
    /// Persisted screen X coordinate when user drags the launcher (`None` = auto center).
    #[serde(default)]
    pub custom_x: Option<i32>,
    /// Persisted screen Y coordinate when user drags the launcher (`None` = auto upper-third).
    #[serde(default)]
    pub custom_y: Option<i32>,
}

fn default_win_width() -> i32 {
    720
}
fn default_max_rows() -> usize {
    8
}
fn default_preview_width() -> i32 {
    310
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width_dip: default_win_width(),
            max_visible_rows: default_max_rows(),
            preview_width_dip: default_preview_width(),
            custom_x: None,
            custom_y: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EverythingConfig {
    #[serde(default = "default_ev_global")]
    pub max_global_results: usize,
    #[serde(default = "default_ev_dedicated")]
    pub max_dedicated_results: usize,
    /// Extra default filter appended to dedicated queries (e.g. `"!*.tmp"`).
    #[serde(default)]
    pub extra_filter: String,
}

impl Default for EverythingConfig {
    fn default() -> Self {
        Self {
            max_global_results: default_ev_global(),
            max_dedicated_results: default_ev_dedicated(),
            extra_filter: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardConfig {
    #[serde(default = "default_clip_max")]
    pub max_persisted_items: usize,
    #[serde(default = "default_true")]
    pub persist_sqlite: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            max_persisted_items: default_clip_max(),
            persist_sqlite: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchEngine {
    pub keyword: String,
    pub name: String,
    /// URL template with `{q}` placeholder.
    pub url: String,
}

fn default_language() -> String {
    "zh".into()
}
fn default_hotkey() -> String {
    "Alt+Space".into()
}
fn default_theme() -> String {
    "dark".into()
}
fn default_hibernate_secs() -> u32 {
    15
}
fn default_true() -> bool {
    true
}
fn default_ev_global() -> usize {
    6
}
fn default_ev_dedicated() -> usize {
    30
}
fn default_clip_max() -> usize {
    500
}

pub fn default_web_searches() -> Vec<WebSearchEngine> {
    vec![
        WebSearchEngine {
            keyword: "g".into(),
            name: "Google Search".into(),
            url: "https://www.google.com/search?q={q}".into(),
        },
        WebSearchEngine {
            keyword: "gh".into(),
            name: "GitHub Search".into(),
            url: "https://github.com/search?q={q}".into(),
        },
        WebSearchEngine {
            keyword: "crates".into(),
            name: "crates.io (Rust)".into(),
            url: "https://crates.io/search?q={q}".into(),
        },
        WebSearchEngine {
            keyword: "docs".into(),
            name: "docs.rs (Rust Docs)".into(),
            url: "https://docs.rs/releases/search?query={q}".into(),
        },
        WebSearchEngine {
            keyword: "bd".into(),
            name: "Baidu 百度搜索".into(),
            url: "https://www.baidu.com/s?wd={q}".into(),
        },
        WebSearchEngine {
            keyword: "zh".into(),
            name: "Zhihu 知乎搜索".into(),
            url: "https://www.zhihu.com/search?type=content&q={q}".into(),
        },
        WebSearchEngine {
            keyword: "mdn".into(),
            name: "MDN Web Docs".into(),
            url: "https://developer.mozilla.org/en-US/search?q={q}".into(),
        },
    ]
}

impl Default for TomoyoConfig {
    fn default() -> Self {
        Self {
            language: default_language(),
            hotkey: default_hotkey(),
            theme: default_theme(),
            hibernate_delay_secs: default_hibernate_secs(),
            auto_preview: true,
            window: WindowConfig::default(),
            everything: EverythingConfig::default(),
            clipboard: ClipboardConfig::default(),
            update: UpdateConfig::default(),
            plugin_registries: default_plugin_registries(),
            web_searches: default_web_searches(),
        }
    }
}

impl TomoyoConfig {
    pub fn lang(&self) -> crate::Lang {
        crate::Lang::from_code(&self.language)
    }
    pub fn config_path() -> PathBuf {
        crate::data_dir().join("config.toml")
    }

    pub fn load_or_create() -> Self {
        let path = Self::config_path();
        Self::load_from_path(&path)
    }

    pub fn load_from_path(path: &Path) -> Self {
        if let Ok(raw) = std::fs::read_to_string(path) {
            match toml::from_str::<Self>(&raw) {
                Ok(cfg) => return cfg,
                Err(e) => {
                    tracing::warn!("Invalid config.toml at {}: {e}; using defaults", path.display());
                }
            }
        }
        let cfg = Self::default();
        let _ = cfg.save_to_path(path);
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to_path(&Self::config_path())
    }

    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self).unwrap_or_default();
        let header = "# Tomoyo Configuration (hot-reloaded via `:reload`)\n\
                      # Themes: \"dark\" | \"light\" | \"midnight\"\n\
                      # Hotkey examples: \"Alt+Space\", \"Ctrl+Space\", \"Ctrl+Shift+Space\"\n\n";
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, format!("{header}{body}"))?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

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
