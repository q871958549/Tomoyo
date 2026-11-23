//! Process-private Alibaba PuHuiTi 3.0 TrueType font loader (`AddFontResourceExW(FR_PRIVATE)`).
//!
//! Zero system pollution and zero heap copy: Windows memory-maps the `.ttf` files read-only
//! for this process only and cleanly unloads them on drop.

use std::path::PathBuf;

/// Exact TrueType family names verified from Alibaba PuHuiTi 3.0 TTF headers.
pub const FONT_ALIBABA_REGULAR: &str = "阿里巴巴普惠体 3.0 55 Regular";
pub const FONT_ALIBABA_MEDIUM: &str = "阿里巴巴普惠体 3.0 65 Medium";
#[cfg(windows)]
pub const FONT_FALLBACK_UI: &str = "Microsoft YaHei UI";
#[cfg(not(windows))]
pub const FONT_FALLBACK_UI: &str = "Noto Sans CJK SC";

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
        "Microsoft YaHei UI",
        "Segoe UI Variable Text",
        "Segoe UI Variable Display",
        "Segoe UI",
        "PingFang SC",
        "HarmonyOS Sans SC",
        "MiSans",
        "OPPO Sans",
        "Noto Sans SC",
        "Noto Sans CJK SC",
        "Source Han Sans SC",
        "Sarasa UI SC",
        "Cascadia Mono",
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
    #[cfg(not(windows))]
    let mut raw: Vec<String> = CURATED.iter().map(|s| (*s).to_string()).collect();

    let mut out = Vec::with_capacity(raw.len().min(120));
    for pref in CURATED {
        if let Some(pos) = raw.iter().position(|r| r.eq_ignore_ascii_case(pref)) {
            out.push(raw.swap_remove(pos));
        }
    }
    raw.sort_by_key(|a| a.to_ascii_lowercase());
    for name in raw {
        if out.len() >= 120 {
            break;
        }
        out.push(name);
    }
    out
}
