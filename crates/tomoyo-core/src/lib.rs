//! Core engine for Tomoyo: configuration, native WinHTTP auto-updater, i18n,
//! native OS UI font engine, query routing, bounded file/image preview,
//! Pinyin + fuzzy matching, frecency ranking, intent detection, and memory telemetry.

pub mod config;
pub mod diagnostics;
pub mod engine;
mod engine_rank;
mod engine_slow;
pub mod file_manager;
pub mod flows;
pub mod flows_llm;
pub mod fonts;
pub mod frecency;
pub mod http;
pub mod i18n;
pub mod intent;
#[cfg(target_os = "linux")]
pub mod linux_launch;
#[cfg(target_os = "linux")]
pub mod linux_sys;
pub mod matcher;
pub mod matcher_highlight;
pub mod permissions;
pub mod pkg_zip;
pub mod plugin_assets;
pub mod preview;
mod preview_docs;
mod preview_xlsx;
pub mod sync;
pub mod telemetry;
pub mod translate;
pub mod updater;
pub mod wic_thumb;

pub use config::*;
pub use diagnostics::*;
pub use engine::*;
pub use file_manager::{
    resolve_open_folder_command, resolve_reveal_file_command, reveal_file_in_file_manager,
    reveal_in_custom_file_manager,
};
pub use flows::*;
pub use flows_llm::*;
pub use fonts::*;
pub use frecency::*;
pub use http::*;
pub use i18n::*;
pub use intent::*;
pub use matcher::*;
pub use matcher_highlight::*;
pub use permissions::*;
pub use pkg_zip::*;
pub use plugin_assets::*;
pub use preview::*;
pub use sync::*;
pub use telemetry::*;
pub use translate::{
    delete_secret, get_secret, has_secret, is_valid_secret_key, list_stored_secret_keys,
    load_plugin_secrets, plugin_secret_key, set_secret,
};
pub use updater::*;

use std::path::PathBuf;

/// Returns Tomoyo's local data directory (`%LOCALAPPDATA%\Tomoyo` on Windows,
/// `$XDG_DATA_HOME/tomoyo` or `~/.local/share/tomoyo` on Linux/Unix), honoring
/// `TOMOYO_DATA_DIR` when explicitly overridden (e.g., isolated `--self-test` runs).
pub fn data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("TOMOYO_DATA_DIR")
        && !custom.trim().is_empty()
    {
        return PathBuf::from(custom.trim());
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Tomoyo");
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg.trim()).join("tomoyo");
    }
    if let Ok(home) = std::env::var("HOME") {
        let legacy = PathBuf::from(&home).join(".tomoyo");
        let xdg_default = PathBuf::from(&home).join(".local").join("share").join("tomoyo");
        if legacy.exists() && !xdg_default.exists() {
            return legacy;
        }
        return xdg_default;
    }
    PathBuf::from(".tomoyo")
}

/// Normalizes a plugin ID into a safe, bounded ASCII identifier shared by Win32 window
/// classes, kernel single-instance mutexes, and pending-query handoff files.
pub fn sanitize_plugin_instance_tag(raw: &str) -> String {
    let tag: String = raw
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(48)
        .collect();
    if tag.is_empty() {
        "default".into()
    } else {
        tag
    }
}

/// Returns the deterministic Win32 window class name for a standalone plugin WebView window.
pub fn plugin_webview_window_class(plugin_id: &str) -> String {
    format!("TomoyoPluginHost_{}", sanitize_plugin_instance_tag(plugin_id))
}

/// Returns the pending query handoff file path used when reactivating an existing plugin window.
pub fn pending_plugin_query_path(plugin_id: &str) -> PathBuf {
    logs_dir().join(format!(
        "pending_plugin_query_{}.txt",
        sanitize_plugin_instance_tag(plugin_id)
    ))
}
