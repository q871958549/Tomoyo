//! Runtime settings resolver, canonical `<tomoyo_data_dir>` locator, and pause-flag helpers.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ClipRuntimeConfig {
    pub capture_enabled: bool,
    pub capture_images: bool,
    pub persist_sqlite: bool,
    pub max_persisted_items: usize,
    pub excluded_apps: Vec<String>,
    pub auto_clear_secrets: bool,
    pub secret_ttl_secs: u64,
}

impl ClipRuntimeConfig {
    pub fn from_settings(settings: &BTreeMap<String, Value>) -> Self {
        let capture_enabled = settings
            .get("capture_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let capture_images = settings
            .get("capture_images")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let persist_sqlite = settings
            .get("persist_sqlite")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let max_persisted_items = settings
            .get("max_persisted_items")
            .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(500)
            .clamp(50, 2000) as usize;
        let excluded_apps = settings
            .get("excluded_apps")
            .and_then(Value::as_str)
            .map(|raw| {
                raw.split([',', ';', '\n'])
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let auto_clear_secrets = settings
            .get("auto_clear_secrets")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let secret_ttl_secs = settings
            .get("secret_ttl_secs")
            .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(60)
            .clamp(15, 600);
        Self {
            capture_enabled,
            capture_images,
            persist_sqlite,
            max_persisted_items,
            excluded_apps,
            auto_clear_secrets,
            secret_ttl_secs,
        }
    }

    pub fn is_effectively_capturing(&self) -> bool {
        self.capture_enabled && !is_runtime_paused()
    }
}

pub fn tomoyo_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("Tomoyo");
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
            && !xdg.trim().is_empty()
        {
            return PathBuf::from(xdg).join("tomoyo");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".local").join("share").join("tomoyo");
        }
    }
    std::env::temp_dir().join("tomoyo")
}

fn pause_flag_path() -> PathBuf {
    tomoyo_data_dir().join("clipboard_paused")
}

pub fn is_runtime_paused() -> bool {
    pause_flag_path().is_file()
}

pub fn toggle_runtime_paused() -> bool {
    let p = pause_flag_path();
    if p.is_file() {
        let _ = std::fs::remove_file(&p);
        false
    } else {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&p, "1");
        true
    }
}
