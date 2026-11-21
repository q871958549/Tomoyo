//! Core engine for Tomoyo: configuration, native WinHTTP auto-updater, i18n,
//! private Alibaba PuHuiTi fonts, query routing, bounded file/image preview,
//! Pinyin + fuzzy matching, frecency ranking, intent detection, and memory telemetry.

pub mod config;
pub mod diagnostics;
pub mod engine;
mod engine_rank;
pub mod fonts;
pub mod frecency;
pub mod http;
pub mod i18n;
pub mod intent;
pub mod matcher;
pub mod matcher_highlight;
pub mod permissions;
pub mod pkg_zip;
pub mod preview;
mod preview_docs;
pub mod sync;
pub mod telemetry;
pub mod translate;
pub mod updater;
pub mod wic_thumb;

pub use config::*;
pub use diagnostics::*;
pub use engine::*;
pub use fonts::*;
pub use frecency::*;
pub use http::*;
pub use i18n::*;
pub use intent::*;
pub use matcher::*;
pub use matcher_highlight::*;
pub use permissions::*;
pub use pkg_zip::*;
pub use preview::*;
pub use sync::*;
pub use telemetry::*;
pub use updater::*;

use std::path::PathBuf;

/// Returns Tomoyo's local data directory (`%LOCALAPPDATA%\Tomoyo` on Windows).
pub fn data_dir() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Tomoyo");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".tomoyo");
    }
    PathBuf::from(".tomoyo")
}
