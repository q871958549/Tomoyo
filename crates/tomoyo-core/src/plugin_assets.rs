//! Bundled plugin icon discovery, inline SVG/PNG data-URL encoding, and Quiet.md companion discovery.

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

/// Maps official module IDs (`dev.tomoyo.*` / `builtin.*`) to their dedicated 64x64 logo filename.
pub fn official_icon_filename(plugin_id: &str) -> Option<&'static str> {
    match plugin_id.trim().to_ascii_lowercase().as_str() {
        "dev.tomoyo.clipboard" | "builtin.clipboard" => Some("clipboard.png"),
        "dev.tomoyo.translate" | "builtin.translate" => Some("translate.png"),
        "dev.tomoyo.capture" | "builtin.capture" => Some("capture.png"),
        "dev.tomoyo.ocr" => Some("ocr.png"),
        "dev.tomoyo.notes" => Some("notes.png"),
        "dev.tomoyo.markdown-editor" => Some("markdown-editor.png"),
        "dev.tomoyo.example-hello" => Some("hello.png"),
        "dev.tomoyo.example-python" => Some("python.png"),
        "dev.tomoyo.example-wasm" => Some("wasm.png"),
        "dev.tomoyo.example-webview-json" => Some("webview-json.png"),
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

/// Managed companion directory `%LOCALAPPDATA%\Tomoyo\tools\quiet-md` for one-click verified Quiet.md install.
pub fn quiet_md_tools_dir() -> PathBuf {
    crate::data_dir().join("tools").join("quiet-md")
}

/// Detects an installed `Quiet.md` binary across managed tools dir, user installs, Program Files, and `PATH`.
pub fn detect_quiet_md_binary(custom_path: Option<&str>) -> Option<PathBuf> {
    if let Some(raw) = custom_path.map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(raw);
        if p.is_file() {
            return Some(p);
        }
    }

    let mut candidates = Vec::new();
    let tools = quiet_md_tools_dir();
    #[cfg(windows)]
    {
        for exe_name in ["quiet-md.exe", "Quiet.md.exe", "quiet_md.exe"] {
            candidates.push(tools.join(exe_name));
            if let Ok(local) = std::env::var("LOCALAPPDATA") {
                let lb = PathBuf::from(&local);
                candidates.push(lb.join("Programs").join("Quiet.md").join(exe_name));
                candidates.push(lb.join("Programs").join("quiet-md").join(exe_name));
                candidates.push(lb.join("Quiet.md").join(exe_name));
            }
            for env_key in ["ProgramFiles", "ProgramFiles(x86)"] {
                if let Ok(pf) = std::env::var(env_key) {
                    candidates.push(PathBuf::from(&pf).join("Quiet.md").join(exe_name));
                    candidates.push(PathBuf::from(&pf).join("quiet-md").join(exe_name));
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        candidates.push(tools.join("quiet-md.AppImage"));
        candidates.push(tools.join("quiet-md"));
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(PathBuf::from(&home).join(".local/bin/quiet-md"));
        }
        candidates.push(PathBuf::from("/usr/bin/quiet-md"));
        candidates.push(PathBuf::from("/usr/local/bin/quiet-md"));
    }

    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }

    let path_var = std::env::var_os("PATH")?;
    let names: &[&str] = if cfg!(windows) {
        &["quiet-md.exe", "quiet_md.exe"]
    } else {
        &["quiet-md", "quiet_md"]
    };
    for dir in std::env::split_paths(&path_var) {
        for n in names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_official_plugin_logos_without_tomoyo_fallback() {
        assert_eq!(official_icon_filename("dev.tomoyo.notes"), Some("notes.png"));
        assert_eq!(
            official_icon_filename("dev.tomoyo.markdown-editor"),
            Some("markdown-editor.png")
        );
        assert_eq!(official_icon_filename("builtin.translate"), Some("translate.png"));
        assert_eq!(official_icon_filename("dev.tomoyo.unknown"), None);
    }
}
