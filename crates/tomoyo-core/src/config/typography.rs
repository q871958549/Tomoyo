//! Typography & result-row density settings for native Win32/Cairo search rendering.

use serde::{Deserialize, Serialize};

fn default_title_weight() -> u16 {
    500
}
fn default_subtitle_weight() -> u16 {
    400
}
fn default_title_size() -> u8 {
    16
}
pub(crate) fn default_filter_layout() -> String {
    "wrap".into()
}

/// Configurable typography tokens for search results and input box.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypographyConfig {
    /// Custom font family name (`""` = native OS UI stack, e.g. `sans-serif` on Linux or `Microsoft YaHei UI` on Windows).
    #[serde(default)]
    pub font_family: String,
    /// Search result title weight: `300` (Light), `400` (Regular), `500` (Medium), `600` (SemiBold), `700` (Bold).
    #[serde(default = "default_title_weight")]
    pub title_weight: u16,
    /// Search result subtitle/metadata weight: `300` (Light), `400` (Regular), `500` (Medium), `600` (SemiBold).
    #[serde(default = "default_subtitle_weight")]
    pub subtitle_weight: u16,
    /// Result title size in DIPs (`14..=18`, default `16`). Subtitle scales proportionally (`-3 DIP`).
    #[serde(default = "default_title_size")]
    pub title_size: u8,
}

impl Default for TypographyConfig {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            title_weight: default_title_weight(),
            subtitle_weight: default_subtitle_weight(),
            title_size: default_title_size(),
        }
    }
}

impl TypographyConfig {
    /// Effective subtitle size in DIPs maintaining crisp hierarchy against `title_size`.
    pub fn subtitle_size_dip(&self) -> f32 {
        (self.title_size.clamp(14, 18) as f32 - 3.0).clamp(11.0, 15.0)
    }

    /// Adaptive search result row height in DIPs so larger title sizes breathe comfortably.
    pub fn row_height_dip(&self) -> i32 {
        match self.title_size.clamp(14, 18) {
            17 | 18 => 58,
            _ => 54,
        }
    }
}

/// Curated cross-platform font preset pairing Latin, CJK, and crisp screen metrics.
#[derive(Debug, Clone, Copy)]
pub struct FontPreset {
    pub id: &'static str,
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub desc_zh: &'static str,
    pub desc_en: &'static str,
    pub family: &'static str,
}

/// Built-in one-click typography presets surfaced in `:font` and Settings UI.
pub const BUILTIN_FONT_PRESETS: &[FontPreset] = &[
    FontPreset {
        id: "native",
        name_zh: "系统原生",
        name_en: "Native OS",
        desc_zh: "跟随系统桌面默认字体栈 (Inter + 思源黑体 / 微软雅黑)",
        desc_en: "Follow host OS UI font stack",
        family: "",
    },
    FontPreset {
        id: "modern",
        name_zh: "极简现代",
        name_en: "Minimal Modern",
        desc_zh: "Geist + 鸿蒙黑体 · 通透耐看、近似苹方质感",
        desc_en: "Geist + HarmonyOS Sans SC · Clean macOS-like poise",
        family: "Geist, HarmonyOS Sans SC",
    },
    FontPreset {
        id: "geek",
        name_zh: "极客清晰",
        name_en: "Crisp Geek",
        desc_zh: "IBM Plex Sans + 更纱黑体 UI · 1x 屏幕锐利标杆",
        desc_en: "IBM Plex Sans + Sarasa UI SC · Razor-sharp hinting",
        family: "IBM Plex Sans, Sarasa UI SC",
    },
    FontPreset {
        id: "tech",
        name_zh: "干练科技",
        name_en: "Sharp Tech",
        desc_zh: "Plus Jakarta Sans + MiSans · 几何利落、数字出彩",
        desc_en: "Plus Jakarta Sans + MiSans · Geometric & crisp",
        family: "Plus Jakarta Sans, MiSans",
    },
    FontPreset {
        id: "wenkai",
        name_zh: "温润文楷",
        name_en: "Warm WenKai",
        desc_zh: "霞鹜文楷屏幕阅读版 · 温润舒展、长文舒适",
        desc_en: "LXGW WenKai Screen · Warm editorial legibility",
        family: "LXGW WenKai Screen",
    },
];

/// Formats a comma-separated font stack into a safe CSS `font-family` list with `fallback`.
pub fn format_css_font_stack(family: &str, fallback: &str) -> String {
    let parts: Vec<String> = family
        .split(',')
        .filter_map(|seg| {
            let clean: String = seg
                .trim()
                .chars()
                .filter(|&c| !c.is_control() && !matches!(c, '"' | '\'' | '\\' | ';' | '{' | '}'))
                .collect();
            (!clean.is_empty()).then(|| format!("\"{clean}\""))
        })
        .collect();
    if parts.is_empty() {
        fallback.to_string()
    } else {
        format!("{}, {fallback}", parts.join(", "))
    }
}

/// Extracts the primary single face name for Win32 `CreateFontW` when a stack string is configured.
pub fn primary_gdi_face(family: &str) -> &str {
    family
        .split(',')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("")
}

/// Snaps a numeric CSS/GDI weight to `[300, 400, 500, 600, 700]` within `[min_w, max_w]`.
pub fn quantize_font_weight(w: u16, min_w: u16, max_w: u16) -> u16 {
    let clamped = w.clamp(min_w, max_w);
    match clamped {
        0..=349 => 300,
        350..=449 => 400,
        450..=549 => 500,
        550..=649 => 600,
        _ => 700,
    }
    .clamp(min_w, max_w)
}

/// Normalizes user-supplied typography fields and rejects control characters or vertical `@` faces.
pub fn sanitize_typography(t: &mut TypographyConfig) {
    let cleaned: String = t
        .font_family
        .trim()
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\'' | ';' | '{' | '}' | '\\'))
        .take(96)
        .collect();
    if cleaned.is_empty()
        || cleaned.starts_with('@')
        || cleaned.eq_ignore_ascii_case("auto")
        || cleaned.eq_ignore_ascii_case("default")
    {
        t.font_family.clear();
    } else {
        t.font_family = cleaned;
    }

    t.title_weight = quantize_font_weight(if t.title_weight == 0 { 500 } else { t.title_weight }, 300, 700);
    t.subtitle_weight =
        quantize_font_weight(if t.subtitle_weight == 0 { 400 } else { t.subtitle_weight }, 300, 600);
    t.title_size = if t.title_size == 0 {
        16
    } else {
        t.title_size.clamp(14, 18)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_typography_weights_and_bounds() {
        let mut t = TypographyConfig {
            font_family: "@MS Gothic".into(),
            title_weight: 640,
            subtitle_weight: 999,
            title_size: 42,
        };
        sanitize_typography(&mut t);
        assert!(t.font_family.is_empty());
        assert_eq!(t.title_weight, 600);
        assert_eq!(t.subtitle_weight, 600);
        assert_eq!(t.title_size, 18);
        assert_eq!(t.row_height_dip(), 58);

        let mut custom = TypographyConfig {
            font_family: "  Plus Jakarta Sans, MiSans  ".into(),
            title_weight: 700,
            subtitle_weight: 300,
            title_size: 14,
        };
        sanitize_typography(&mut custom);
        assert_eq!(custom.font_family, "Plus Jakarta Sans, MiSans");
        assert_eq!(custom.subtitle_size_dip(), 11.0);
        assert_eq!(custom.row_height_dip(), 54);
        assert_eq!(
            format_css_font_stack(&custom.font_family, "sans-serif"),
            "\"Plus Jakarta Sans\", \"MiSans\", sans-serif"
        );
        assert_eq!(primary_gdi_face(&custom.font_family), "Plus Jakarta Sans");
    }
}
