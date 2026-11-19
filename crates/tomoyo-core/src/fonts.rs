//! Process-private Alibaba PuHuiTi 3.0 TrueType font loader (`AddFontResourceExW(FR_PRIVATE)`).
//!
//! Zero system pollution and zero heap copy: Windows memory-maps the `.ttf` files read-only
//! for this process only and cleanly unloads them on drop.

use std::path::PathBuf;

/// Exact Windows TrueType family names verified from Alibaba PuHuiTi 3.0 TTF headers.
pub const FONT_ALIBABA_REGULAR: &str = "阿里巴巴普惠体 3.0 55 Regular";
pub const FONT_ALIBABA_MEDIUM: &str = "阿里巴巴普惠体 3.0 65 Medium";
pub const FONT_FALLBACK_UI: &str = "Microsoft YaHei UI";

/// RAII guard that registers bundled Alibaba PuHuiTi TTF files privately for the process.
pub struct PrivateFonts {
    loaded_paths: Vec<PathBuf>,
}

impl PrivateFonts {
    /// Discovers and loads `AlibabaPuHuiTi-3-55-Regular.ttf` and `AlibabaPuHuiTi-3-65-Medium.ttf`.
    pub fn load_default() -> Self {
        let mut loaded_paths = Vec::new();
        if let Some(dir) = locate_fonts_dir() {
            for fname in [
                "AlibabaPuHuiTi-3-55-Regular.ttf",
                "AlibabaPuHuiTi-3-65-Medium.ttf",
            ] {
                let path = dir.join(fname);
                if path.is_file() && register_private_font(&path) {
                    tracing::info!("Loaded private font: {}", path.display());
                    loaded_paths.push(path);
                }
            }
        }
        Self { loaded_paths }
    }

    pub fn has_alibaba_regular(&self) -> bool {
        self.loaded_paths
            .iter()
            .any(|p| p.to_string_lossy().contains("55-Regular"))
    }

    pub fn has_alibaba_medium(&self) -> bool {
        self.loaded_paths
            .iter()
            .any(|p| p.to_string_lossy().contains("65-Medium"))
    }

    pub fn regular_family(&self) -> &'static str {
        if self.has_alibaba_regular() {
            FONT_ALIBABA_REGULAR
        } else {
            FONT_FALLBACK_UI
        }
    }

    pub fn medium_family(&self) -> &'static str {
        if self.has_alibaba_medium() {
            FONT_ALIBABA_MEDIUM
        } else if self.has_alibaba_regular() {
            FONT_ALIBABA_REGULAR
        } else {
            FONT_FALLBACK_UI
        }
    }
}

impl Drop for PrivateFonts {
    fn drop(&mut self) {
        for path in &self.loaded_paths {
            unregister_private_font(path);
        }
    }
}

/// Searches standard relative & user locations for `assets/fonts`.
pub fn locate_fonts_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("assets").join("fonts"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("assets").join("fonts"));
            candidates.push(dir.join("fonts"));
            // Cargo workspace `target/release` or `target/debug` -> `../../assets/fonts`
            if let Some(p1) = dir.parent() {
                if let Some(p2) = p1.parent() {
                    candidates.push(p2.join("assets").join("fonts"));
                }
            }
        }
    }
    candidates.push(crate::data_dir().join("fonts"));

    candidates
        .into_iter()
        .find(|d| d.join("AlibabaPuHuiTi-3-55-Regular.ttf").is_file())
}

#[cfg(windows)]
fn register_private_font(path: &std::path::Path) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{AddFontResourceExW, FR_PRIVATE};

    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe { AddFontResourceExW(PCWSTR(wide.as_ptr()), FR_PRIVATE, None) > 0 }
}

#[cfg(windows)]
fn unregister_private_font(path: &std::path::Path) {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{RemoveFontResourceExW, FR_PRIVATE};

    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = RemoveFontResourceExW(PCWSTR(wide.as_ptr()), FR_PRIVATE.0, None);
    }
}

#[cfg(not(windows))]
fn register_private_font(_path: &std::path::Path) -> bool {
    false
}
#[cfg(not(windows))]
fn unregister_private_font(_path: &std::path::Path) {}
