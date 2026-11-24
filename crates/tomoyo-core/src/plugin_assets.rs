//! Bundled built-in module icon discovery and inline PNG/ICO data-URL encoding.

use base64::Engine as _;
use std::path::{Path, PathBuf};
use tomoyo_protocol::IconSpec;

/// Searches workspace, install directory, Linux `/usr/share`, and user data dir for `assets/plugin-icons`.
pub fn locate_plugin_icons_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("assets").join("plugin-icons"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("assets").join("plugin-icons"));
        candidates.push(dir.join("plugin-icons"));
        if let Some(p1) = dir.parent() {
            candidates.push(p1.join("share").join("tomoyo").join("plugin-icons"));
            if let Some(p2) = p1.parent() {
                candidates.push(p2.join("assets").join("plugin-icons"));
            }
        }
    }
    candidates.push(crate::data_dir().join("plugin-icons"));
    #[cfg(target_os = "linux")]
    {
        candidates.push(PathBuf::from("/usr/share/tomoyo/plugin-icons"));
        candidates.push(PathBuf::from("/usr/local/share/tomoyo/plugin-icons"));
    }
    candidates.into_iter().find(|d| d.is_dir())
}

/// Maps still-embedded built-in module IDs (`clipboard`/`translate`/`capture`) to their 64x64 logo filename.
/// External plugins resolve icons strictly from their own `icon.png` / `manifest.icon` directory.
pub fn official_icon_filename(plugin_id: &str) -> Option<&'static str> {
    match plugin_id.trim().to_ascii_lowercase().as_str() {
        "dev.tomoyo.clipboard" | "builtin.clipboard" => Some("clipboard.png"),
        "dev.tomoyo.translate" | "builtin.translate" => Some("translate.png"),
        "dev.tomoyo.capture" | "builtin.capture" => Some("capture.png"),
        _ => None,
    }
}

/// Resolves the dedicated `icon.png` path for any official module or bundled plugin.
pub fn resolve_bundled_module_icon(plugin_id: &str) -> Option<PathBuf> {
    let fname = official_icon_filename(plugin_id)?;
    let dir = locate_plugin_icons_dir()?;
    let full = dir.join(fname);
    full.is_file().then_some(full)
}

/// Returns `IconSpec::ImageFile` if an official module logo is present on disk, else `Monogram`.
pub fn official_icon_or_monogram(plugin_id: &str, fallback_monogram: &str) -> IconSpec {
    resolve_bundled_module_icon(plugin_id)
        .map(|p| IconSpec::ImageFile(p.to_string_lossy().into_owned()))
        .unwrap_or_else(|| IconSpec::Monogram(fallback_monogram.to_string()))
}

/// Encodes a local PNG/ICO/SVG plugin icon (`<= 48 KB`) into a data URL for Settings UI cards.
pub fn read_icon_data_url(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() == 0 || meta.len() > 48 * 1024 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return None,
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_official_plugin_logos_without_tomoyo_fallback() {
        assert_eq!(official_icon_filename("dev.tomoyo.clipboard"), Some("clipboard.png"));
        assert_eq!(official_icon_filename("builtin.translate"), Some("translate.png"));
        assert_eq!(official_icon_filename("dev.tomoyo.capture"), Some("capture.png"));
        assert_eq!(official_icon_filename("dev.tomoyo.markdown-editor"), None);
        assert_eq!(official_icon_filename("dev.tomoyo.unknown"), None);
    }
}
