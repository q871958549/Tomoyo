//! Hot-reloadable `config.toml`, hotkey parser, and Windows login autostart management.

mod capture;
mod hotkey;
mod modules;
mod paths;
mod sanitize;
mod sections;
mod typography;

pub use capture::{sanitize_capture_config, CaptureConfig, CaptureToolHotkeys};
pub use sections::{
    default_ev_favorites, AppsConfig, ClipboardConfig, EverythingConfig, EverythingFavorite,
    UpdateConfig, WebSearchEngine, WindowConfig,
};
use sections::{default_plugin_registries, default_true};
pub use typography::{
    quantize_font_weight, sanitize_typography, TypographyConfig,
};
pub use crate::translate::TranslateConfig;
pub use hotkey::{
    format_hotkey, is_autostart_enabled, is_standalone_allowed_vk, load_hotkey_health, parse_hotkey,
    save_hotkey_health, set_autostart_enabled, vk_to_key_name, HotkeyHealthReport, HotkeySlotStatus,
    ParsedHotkey,
};
pub use modules::{
    migrate_legacy_plugin_config, sanitize_filters_and_plugins, PLUGIN_CAPTURE, PLUGIN_CLIPBOARD,
    PLUGIN_MD_EDITOR, PLUGIN_NOTES, PLUGIN_OCR, PLUGIN_TRANSLATE,
};
pub use paths::{
    default_excluded_apps, default_hotkey_excluded_apps, default_web_searches, normalize_exe_name,
    validate_scan_dir,
};
pub use sanitize::{
    is_safe_http_url, is_valid_github_repo, sanitize_config, validate_web_search, MAX_EXTRA_DIRS,
    MAX_SCAN_DEPTH,
};
pub use tomoyo_protocol::CustomFilterTab;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomoyoConfig {
    /// UI language: `"zh"` (default, Simplified Chinese) or `"en"` (English).
    #[serde(default = "default_language")]
    pub language: String,
    /// Global summon shortcut, e.g. `"Alt+Space"`, `"Ctrl+Space"`, `"Ctrl+Shift+Space"`.
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// Automatically rescue conflicted Tomoyo shortcuts via a zero-thread low-level hook ONLY when
    /// Win32 `RegisterHotKey` fails (zero hooks mounted when no shortcut conflicts exist).
    #[serde(default = "default_true")]
    pub override_conflict_hotkeys: bool,
    /// Automatically suspend Tomoyo hotkeys when a fullscreen game/video is active.
    #[serde(default = "default_true")]
    pub suspend_hotkeys_in_fullscreen: bool,
    /// Foreground process executables (e.g. `raylink.exe`, `mstsc.exe`) where Tomoyo yields hotkeys.
    #[serde(default = "default_hotkey_excluded_apps")]
    pub hotkey_excluded_apps: Vec<String>,
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
    /// Launcher filter bar layout mode: `"wrap"` (multi-row + overflow) or `"single"` (1-row + overflow).
    #[serde(default = "typography::default_filter_layout")]
    pub filter_bar_layout: String,
    /// Built-in filter tab IDs hidden by user (e.g. `["ppt", "word"]`).
    #[serde(default)]
    pub hidden_builtin_filters: Vec<String>,
    /// User-extensible custom filter tabs shown in the search filter bar.
    #[serde(default)]
    pub custom_filters: Vec<CustomFilterTab>,
    /// Result & input box typography preferences (font family, weights, size).
    #[serde(default)]
    pub typography: TypographyConfig,
    /// Disabled official or community plugin IDs (e.g. `["dev.tomoyo.translate"]`).
    #[serde(default)]
    pub disabled_plugins: Vec<String>,
    /// Protocol v2: per-plugin global hotkey overrides keyed by `"<plugin_id>:<hotkey_id>"`.
    #[serde(default)]
    pub plugin_hotkeys: BTreeMap<String, String>,
    /// Protocol v2: declarative per-plugin settings values keyed by `plugin_id -> setting_key`.
    #[serde(default)]
    pub plugin_settings: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    /// Per-plugin presentation override (`"inline"` inside launcher preview vs `"window"` detached).
    #[serde(default)]
    pub plugin_display_modes: BTreeMap<String, String>,
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
    /// Multi-engine translation settings (Pot replacement).
    #[serde(default)]
    pub translate: TranslateConfig,
    /// On-demand native screenshot & Pin-to-Desktop settings (PixPin replacement).
    #[serde(default)]
    pub capture: CaptureConfig,
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

fn default_language() -> String { "zh".into() }
fn default_hotkey() -> String { "Alt+Space".into() }
fn default_theme() -> String { "dark".into() }
fn default_hibernate_secs() -> u32 { 15 }

impl Default for TomoyoConfig {
    fn default() -> Self {
        Self {
            language: default_language(),
            hotkey: default_hotkey(),
            override_conflict_hotkeys: true,
            suspend_hotkeys_in_fullscreen: true,
            hotkey_excluded_apps: default_hotkey_excluded_apps(),
            theme: default_theme(),
            hibernate_delay_secs: default_hibernate_secs(),
            auto_preview: true,
            show_filter_bar: true,
            filter_bar_layout: typography::default_filter_layout(),
            hidden_builtin_filters: Vec::new(),
            custom_filters: Vec::new(),
            typography: TypographyConfig::default(),
            disabled_plugins: Vec::new(),
            plugin_hotkeys: BTreeMap::new(),
            plugin_settings: BTreeMap::new(),
            plugin_display_modes: BTreeMap::new(),
            window: WindowConfig::default(),
            apps: AppsConfig::default(),
            everything: EverythingConfig::default(),
            clipboard: ClipboardConfig::default(),
            translate: TranslateConfig::default(),
            capture: CaptureConfig::default(),
            update: UpdateConfig::default(),
            plugin_registries: default_plugin_registries(),
            web_searches: default_web_searches(),
        }
    }
}

impl TomoyoConfig {
    pub fn is_plugin_enabled(&self, plugin_id: &str) -> bool {
        !self.disabled_plugins.iter().any(|id| id.eq_ignore_ascii_case(plugin_id))
    }

    pub fn sanitize(&mut self) -> Vec<String> { sanitize_config(self) }
    pub fn lang(&self) -> crate::Lang { crate::Lang::from_code(&self.language) }
    pub fn config_path() -> PathBuf { crate::sync_root_dir().join("config.toml") }
    pub fn load_or_create() -> Self { Self::load_from_path(&Self::config_path()) }

    pub fn load_from_path(path: &Path) -> Self {
        if let Ok(raw) = std::fs::read_to_string(path) {
            match toml::from_str::<Self>(&raw) {
                Ok(mut cfg) => {
                    let notes = cfg.sanitize();
                    let migrated = migrate_legacy_plugin_config(&mut cfg);
                    if migrated {
                        tracing::info!("Migrated legacy plugin hotkeys/settings into Protocol v3 config");
                    }
                    if !notes.is_empty() {
                        tracing::warn!("Sanitized config.toml on load: {}", notes.join("; "));
                    }
                    if migrated || !notes.is_empty() {
                        let _ = cfg.save_to_path(path);
                    }
                    return cfg;
                }
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
        let mut clean = self.clone();
        let _ = clean.sanitize();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(&clean).unwrap_or_default();
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
