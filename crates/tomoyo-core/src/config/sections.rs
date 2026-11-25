//! Sub-section configuration structs (`Apps`, `Update`, `Window`, `Everything`, `Clipboard`, `WebSearch`).

use super::paths::default_excluded_apps;
use serde::{Deserialize, Serialize};

pub(super) fn default_true() -> bool { true }
pub(super) fn default_plugin_registries() -> Vec<String> {
    vec![
        "registry/index.json".into(),
        "https://raw.githubusercontent.com/q871958549/Tomoyo/main/registry/index.json".into(),
    ]
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

fn default_extra_scan_depth() -> usize { 2 }

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

fn default_update_interval() -> u64 { 24 }
fn default_update_repo() -> String { "q871958549/Tomoyo".into() }

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
    #[serde(default = "default_win_width")]
    pub width_dip: i32,
    #[serde(default = "default_max_rows")]
    pub max_visible_rows: usize,
    #[serde(default = "default_preview_width")]
    pub preview_width_dip: i32,
    #[serde(default)]
    pub custom_x: Option<i32>,
    #[serde(default)]
    pub custom_y: Option<i32>,
}

fn default_win_width() -> i32 { 720 }
fn default_max_rows() -> usize { 8 }
fn default_preview_width() -> i32 { 310 }

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
    #[serde(default)]
    pub extra_filter: String,
    #[serde(default = "default_ev_favorites")]
    pub favorites: Vec<EverythingFavorite>,
}

fn default_ev_global() -> usize { 6 }
fn default_ev_dedicated() -> usize { 30 }

pub fn default_ev_favorites() -> Vec<EverythingFavorite> {
    vec![
        EverythingFavorite { tag: "@today".into(), label: "今日修改文件".into(), expr: "dm:today".into() },
        EverythingFavorite { tag: "@large".into(), label: "大文件 (>50MB)".into(), expr: "size:>50mb".into() },
        EverythingFavorite { tag: "@code".into(), label: "源码工程文件".into(), expr: "ext:rs;ts;tsx;py;go;c;cpp;h;toml".into() },
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
    #[serde(default = "default_clip_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_true")]
    pub capture_enabled: bool,
    #[serde(default = "default_true")]
    pub capture_images: bool,
    #[serde(default = "default_excluded_apps")]
    pub excluded_apps: Vec<String>,
}

fn default_clip_max() -> usize { 500 }
fn default_clip_hotkey() -> String { "Alt+V".into() }

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            max_persisted_items: default_clip_max(),
            persist_sqlite: true,
            hotkey: default_clip_hotkey(),
            capture_enabled: true,
            capture_images: true,
            excluded_apps: default_excluded_apps(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchEngine {
    pub keyword: String,
    pub name: String,
    pub url: String,
}

/// Native Super Panel (uTools-style cursor popup) trigger & inline-translation settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperPanelConfig {
    /// Master switch for the zero-polling mouse Super Panel hook.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Summon Super Panel when holding right mouse button stationary for `hold_ms`.
    #[serde(default = "default_true")]
    pub trigger_right_hold: bool,
    /// Middle mouse button trigger mode: `"hold"` (default, keeps browser tab click intact),
    /// `"click"` (single stationary middle-click), or `"off"`.
    #[serde(default = "default_middle_trigger")]
    pub middle_trigger: String,
    /// Stationary press duration in milliseconds before triggering (`180..=900`, default `320`).
    #[serde(default = "default_super_hold_ms")]
    pub hold_ms: u32,
    /// Automatically run parallel translation inside the Super Panel when text is selected.
    #[serde(default = "default_true")]
    pub auto_translate_selection: bool,
    /// Extra foreground executables (in addition to `hotkey_excluded_apps`) where Super Panel yields.
    #[serde(default)]
    pub excluded_apps: Vec<String>,
}

fn default_middle_trigger() -> String { "hold".into() }
fn default_super_hold_ms() -> u32 { 320 }

impl Default for SuperPanelConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger_right_hold: true,
            middle_trigger: default_middle_trigger(),
            hold_ms: default_super_hold_ms(),
            auto_translate_selection: true,
            excluded_apps: Vec::new(),
        }
    }
}
