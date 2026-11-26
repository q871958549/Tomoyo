//! Native OS UI font stack defaults, system Fontconfig/GDI font enumeration, and optional private TTF loader.

use std::path::PathBuf;

/// Legacy bundled TrueType family names (loaded automatically only if user drops them in `assets/fonts/`).
pub const FONT_ALIBABA_REGULAR: &str = "阿里巴巴普惠体 3.0 55 Regular";
pub const FONT_ALIBABA_MEDIUM: &str = "阿里巴巴普惠体 3.0 65 Medium";
#[cfg(windows)]
pub const FONT_FALLBACK_UI: &str = "Microsoft YaHei UI";
#[cfg(target_os = "linux")]
pub const FONT_FALLBACK_UI: &str = "sans-serif";
#[cfg(all(not(windows), not(target_os = "linux")))]
pub const FONT_FALLBACK_UI: &str = ".AppleSystemUIFont";

/// RAII guard for optional process-private TrueType fonts (defaults to native OS UI stack).
pub struct PrivateFonts {
    loaded_paths: Vec<PathBuf>,
}

impl PrivateFonts {
    /// Initializes Tomoyo's default font engine (uses the host OS native UI font stack with zero bundled TTF bloat).
    pub fn load_default() -> Self {
        Self { loaded_paths: Vec::new() }
    }

    /// Optionally registers a custom process-private TrueType font file until this guard is dropped.
    pub fn load_custom_ttf(&mut self, path: &std::path::Path) -> bool {
        if path.is_file() && register_private_font(path) {
            tracing::info!("Loaded private font: {}", path.display());
            self.loaded_paths.push(path.to_path_buf());
            true
        } else {
            false
        }
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
        FONT_FALLBACK_UI
    }

    pub fn medium_family(&self) -> &'static str {
        FONT_FALLBACK_UI
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
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent() {
            candidates.push(dir.join("assets").join("fonts"));
            candidates.push(dir.join("fonts"));
            // Cargo workspace `target/release` or `target/debug` -> `../../assets/fonts`
            if let Some(p1) = dir.parent() {
                candidates.push(p1.join("share").join("tomoyo").join("fonts"));
                if let Some(p2) = p1.parent() {
                    candidates.push(p2.join("assets").join("fonts"));
                }
            }
        }
    candidates.push(crate::data_dir().join("fonts"));
    #[cfg(target_os = "linux")]
    {
        candidates.push(PathBuf::from("/usr/share/tomoyo/fonts"));
        candidates.push(PathBuf::from("/usr/local/share/tomoyo/fonts"));
    }

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

#[cfg(target_os = "linux")]
fn register_private_font(path: &std::path::Path) -> bool {
    crate::linux_sys::fc_add_font(path)
}
#[cfg(target_os = "linux")]
fn unregister_private_font(_path: &std::path::Path) {
    crate::linux_sys::fc_clear_fonts();
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn register_private_font(_path: &std::path::Path) -> bool {
    false
}
#[cfg(all(not(windows), not(target_os = "linux")))]
fn unregister_private_font(_path: &std::path::Path) {}

/// Enumerates clean, non-vertical UI font families installed on the host OS, prioritized for CJK/UI legibility.
pub fn list_system_font_families() -> Vec<String> {
    const CURATED: &[&str] = &[
        "Inter",
        "Inter Variable",
        "Noto Sans CJK SC",
        "Noto Sans SC",
        "Source Han Sans SC",
        "Sarasa UI SC",
        "HarmonyOS Sans SC",
        "MiSans",
        "OPPO Sans",
        "PingFang SC",
        "LXGW WenKai Screen",
        "Segoe UI Variable Text",
        "Segoe UI Variable Display",
        "Segoe UI",
        "Microsoft YaHei UI",
        "JetBrainsMono Nerd Font",
        "JetBrains Mono",
        "Cascadia Mono",
        "Cantarell",
        "Ubuntu",
        "Noto Sans",
        "DejaVu Sans",
        "Consolas",
    ];
    #[cfg(windows)]
    let mut raw: Vec<String> = {
        use std::collections::HashSet;
        use windows::Win32::Foundation::LPARAM;
        use windows::Win32::Graphics::Gdi::{
            EnumFontFamiliesExW, GetDC, ReleaseDC, DEFAULT_CHARSET, LOGFONTW, TEXTMETRICW,
        };
        unsafe extern "system" fn enum_proc(
            lpelfe: *const LOGFONTW,
            _lpntme: *const TEXTMETRICW,
            _font_type: u32,
            lparam: LPARAM,
        ) -> i32 {
            if !lpelfe.is_null() && lparam.0 != 0 {
                let lf = unsafe { &*lpelfe };
                let end = lf.lfFaceName.iter().position(|&c| c == 0).unwrap_or(lf.lfFaceName.len());
                let name = String::from_utf16_lossy(&lf.lfFaceName[..end]);
                let t = name.trim();
                if !t.is_empty() && !t.starts_with('@') && !t.starts_with('.') {
                    let set = unsafe { &mut *(lparam.0 as *mut HashSet<String>) };
                    set.insert(t.to_string());
                }
            }
            1
        }
        let mut set: HashSet<String> = HashSet::new();
        let hdc = unsafe { GetDC(None) };
        if !hdc.is_invalid() {
            let lf = LOGFONTW {
                lfCharSet: DEFAULT_CHARSET,
                ..Default::default()
            };
            unsafe {
                EnumFontFamiliesExW(
                    hdc,
                    &lf,
                    Some(enum_proc),
                    LPARAM(&mut set as *mut HashSet<String> as isize),
                    0,
                );
                let _ = ReleaseDC(None, hdc);
            }
        }
        set.into_iter().collect()
    };
    #[cfg(target_os = "linux")]
    let mut raw: Vec<String> = {
        let fc = crate::linux_sys::fc_list_font_families();
        if fc.is_empty() {
            CURATED.iter().map(|s| (*s).to_string()).collect()
        } else {
            fc
        }
    };
    #[cfg(all(not(windows), not(target_os = "linux")))]
    let mut raw: Vec<String> = CURATED.iter().map(|s| (*s).to_string()).collect();

    let mut out = Vec::with_capacity(raw.len().min(160));
    for pref in CURATED {
        if let Some(pos) = raw.iter().position(|r| r.eq_ignore_ascii_case(pref)) {
            out.push(raw.swap_remove(pos));
        }
    }
    raw.sort_by_key(|a| a.to_ascii_lowercase());
    for name in raw {
        if out.len() >= 160 {
            break;
        }
        out.push(name);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_font_enumeration_returns_unique_clean_families() {
        let fonts = list_system_font_families();
        assert!(!fonts.is_empty());
        assert!(fonts.len() <= 160);
        let mut seen = std::collections::HashSet::new();
        for f in &fonts {
            assert!(!f.is_empty() && !f.starts_with('@') && !f.starts_with('.'));
            assert!(seen.insert(f.to_ascii_lowercase()), "duplicate font `{f}`");
        }
    }
}
