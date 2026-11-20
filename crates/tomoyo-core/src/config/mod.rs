//! Hot-reloadable `config.toml`, hotkey parser, and Windows login autostart management.

mod hotkey;

pub use hotkey::{is_autostart_enabled, parse_hotkey, set_autostart_enabled, ParsedHotkey};

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
    /// Show quick category/file-type filter bar below the search input.
    #[serde(default = "default_true")]
    pub show_filter_bar: bool,
    /// Persistent window dimensions and optional user-dragged position.
    #[serde(default)]
    pub window: WindowConfig,
    /// Application indexing & desktop organizer integration.
    #[serde(default)]
    pub apps: AppsConfig,
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
pub struct AppsConfig {
    /// Extra directories scanned for `.lnk` / `.exe` applications (e.g. `E:\Tools`).
    #[serde(default)]
    pub extra_dirs: Vec<String>,
    /// Auto-detect desktop organizer folders (DeskBox, Tencent DeskGo, etc.).
    #[serde(default = "default_true")]
    pub auto_detect_desktop_organizers: bool,
    /// Maximum recursion depth for `extra_dirs` (`1..=4`, default `2`).
    #[serde(default = "default_extra_scan_depth")]
    pub extra_scan_depth: usize,
}

fn default_extra_scan_depth() -> usize {
    2
}

impl Default for AppsConfig {
    fn default() -> Self {
        Self {
            extra_dirs: Vec::new(),
            auto_detect_desktop_organizers: true,
            extra_scan_depth: default_extra_scan_depth(),
        }
    }
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
pub struct EverythingFavorite {
    pub tag: String,
    pub label: String,
    pub expr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EverythingConfig {
    #[serde(default = "default_ev_global")]
    pub max_global_results: usize,
    #[serde(default = "default_ev_dedicated")]
    pub max_dedicated_results: usize,
    /// Extra default filter appended to dedicated/file queries (e.g. `"!*.tmp"`).
    #[serde(default)]
    pub extra_filter: String,
    /// User saved favorite Everything filter expressions (`@today`, `@large`, `@code`, etc.).
    #[serde(default = "default_ev_favorites")]
    pub favorites: Vec<EverythingFavorite>,
}

pub fn default_ev_favorites() -> Vec<EverythingFavorite> {
    vec![
        EverythingFavorite {
            tag: "@today".into(),
            label: "今日修改文件".into(),
            expr: "dm:today".into(),
        },
        EverythingFavorite {
            tag: "@large".into(),
            label: "大文件 (>50MB)".into(),
            expr: "size:>50mb".into(),
        },
        EverythingFavorite {
            tag: "@code".into(),
            label: "源码工程文件".into(),
            expr: "ext:rs;ts;tsx;py;go;c;cpp;h;toml".into(),
        },
    ]
}

impl Default for EverythingConfig {
    fn default() -> Self {
        Self {
            max_global_results: default_ev_global(),
            max_dedicated_results: default_ev_dedicated(),
            extra_filter: String::new(),
            favorites: default_ev_favorites(),
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
            show_filter_bar: true,
            window: WindowConfig::default(),
            apps: AppsConfig::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_defaults_when_apps_section_missing() {
        let legacy = "language = \"zh\"\nhotkey = \"Alt+Space\"\ntheme = \"light\"\n";
        let cfg: TomoyoConfig = toml::from_str(legacy).unwrap();
        assert!(cfg.show_filter_bar);
        assert!(cfg.apps.auto_detect_desktop_organizers);
        assert_eq!(cfg.apps.extra_scan_depth, 2);
        assert!(cfg.apps.extra_dirs.is_empty());
    }
}
