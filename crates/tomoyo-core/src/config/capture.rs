//! Screenshot & Pin-to-Desktop (`tomoyo-capture.exe`) configuration, tool hotkeys, and sanitization.

use super::{parse_hotkey, validate_scan_dir};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureToolHotkeys {
    #[serde(default = "hk_rect")]
    pub rect: String,
    #[serde(default = "hk_ellipse")]
    pub ellipse: String,
    #[serde(default = "hk_arrow")]
    pub arrow: String,
    #[serde(default = "hk_pen")]
    pub pen: String,
    #[serde(default = "hk_highlighter")]
    pub highlighter: String,
    #[serde(default = "hk_mosaic")]
    pub mosaic: String,
    #[serde(default = "hk_step")]
    pub step: String,
    #[serde(default = "hk_text")]
    pub text: String,
    #[serde(default = "hk_scroll")]
    pub scroll: String,
    #[serde(default = "hk_pin")]
    pub pin: String,
    #[serde(default = "hk_ocr")]
    pub ocr: String,
    #[serde(default = "hk_translate")]
    pub translate: String,
}

fn hk_rect() -> String { "R".into() }
fn hk_ellipse() -> String { "E".into() }
fn hk_arrow() -> String { "A".into() }
fn hk_pen() -> String { "B".into() }
fn hk_highlighter() -> String { "H".into() }
fn hk_mosaic() -> String { "M".into() }
fn hk_step() -> String { "N".into() }
fn hk_text() -> String { "X".into() }
fn hk_scroll() -> String { "L".into() }
fn hk_pin() -> String { "P".into() }
fn hk_ocr() -> String { "O".into() }
fn hk_translate() -> String { "T".into() }

impl Default for CaptureToolHotkeys {
    fn default() -> Self {
        Self {
            rect: hk_rect(),
            ellipse: hk_ellipse(),
            arrow: hk_arrow(),
            pen: hk_pen(),
            highlighter: hk_highlighter(),
            mosaic: hk_mosaic(),
            step: hk_step(),
            text: hk_text(),
            scroll: hk_scroll(),
            pin: hk_pin(),
            ocr: hk_ocr(),
            translate: hk_translate(),
        }
    }
}

impl CaptureToolHotkeys {
    /// Normalizes a single-key tool binding (`A`–`Z` / `0`–`9`), returning `None` if invalid.
    pub fn normalize_key(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.len() == 1 {
            let b = trimmed.as_bytes()[0].to_ascii_uppercase();
            if b.is_ascii_alphanumeric() {
                return Some((b as char).to_string());
            }
        }
        None
    }

    pub fn matches_vk(binding: &str, vk: u8) -> bool {
        binding.len() == 1 && binding.as_bytes()[0].eq_ignore_ascii_case(&vk)
    }

    pub fn sanitize(&mut self, notes: &mut Vec<String>) {
        let defaults = Self::default();
        let mut used: HashSet<String> = HashSet::new();
        let fields: [(&mut String, &str, &str); 12] = [
            (&mut self.rect, &defaults.rect, "矩形"),
            (&mut self.ellipse, &defaults.ellipse, "椭圆"),
            (&mut self.arrow, &defaults.arrow, "箭头"),
            (&mut self.pen, &defaults.pen, "画笔"),
            (&mut self.highlighter, &defaults.highlighter, "荧光笔"),
            (&mut self.mosaic, &defaults.mosaic, "马赛克"),
            (&mut self.step, &defaults.step, "序号"),
            (&mut self.text, &defaults.text, "文字"),
            (&mut self.scroll, &defaults.scroll, "长截图"),
            (&mut self.pin, &defaults.pin, "贴图"),
            (&mut self.ocr, &defaults.ocr, "识字 OCR"),
            (&mut self.translate, &defaults.translate, "翻译"),
        ];
        for (slot, def, label) in fields {
            if let Some(norm) = Self::normalize_key(slot) {
                if used.insert(norm.clone()) {
                    *slot = norm;
                    continue;
                }
            }
            let fb = def.to_string();
            if used.insert(fb.clone()) {
                *slot = fb;
            } else {
                slot.clear();
            }
            notes.push(format!("截图工具「{label}」快捷键无效或重复，已恢复默认"));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    /// Global shortcut to start region/window screenshot (default `"Alt+S"`, empty disables).
    #[serde(default = "default_capture_hotkey")]
    pub hotkey: String,
    /// Optional custom save folder for PNG exports (empty = `%USERPROFILE%\Pictures\Tomoyo`).
    #[serde(default)]
    pub save_dir: String,
    /// Default opacity percentage (`30..=100`, default `100`) for pinned desktop images.
    #[serde(default = "default_pin_opacity")]
    pub pin_opacity: u8,
    /// Copy picked color hex in lowercase (`#ff6b6b` vs `#FF6B6B`).
    #[serde(default)]
    pub copy_hex_lowercase: bool,
    /// Customizable single-key shortcuts for in-capture annotation & action tools.
    #[serde(default)]
    pub tool_hotkeys: CaptureToolHotkeys,
}

fn default_capture_hotkey() -> String {
    "Alt+S".into()
}

fn default_pin_opacity() -> u8 {
    100
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            hotkey: default_capture_hotkey(),
            save_dir: String::new(),
            pin_opacity: default_pin_opacity(),
            copy_hex_lowercase: false,
            tool_hotkeys: CaptureToolHotkeys::default(),
        }
    }
}

pub fn sanitize_capture_config(
    cap: &mut CaptureConfig,
    main_hk: &str,
    clip_hk: &str,
    tr_hk: &str,
    notes: &mut Vec<String>,
) {
    let hk = cap.hotkey.trim().to_string();
    if hk.is_empty() {
        cap.hotkey.clear();
    } else if parse_hotkey(&hk).is_none()
        || hk.eq_ignore_ascii_case(main_hk)
        || (!clip_hk.is_empty() && hk.eq_ignore_ascii_case(clip_hk))
        || (!tr_hk.is_empty() && hk.eq_ignore_ascii_case(tr_hk))
    {
        let fallback = if main_hk.eq_ignore_ascii_case("Alt+S")
            || clip_hk.eq_ignore_ascii_case("Alt+S")
            || tr_hk.eq_ignore_ascii_case("Alt+S")
        {
            String::new()
        } else {
            "Alt+S".into()
        };
        notes.push(format!("截图快捷键 `{hk}` 无效或与已有快捷键冲突，已重置"));
        cap.hotkey = fallback;
    } else {
        cap.hotkey = hk;
    }

    let op = cap.pin_opacity.clamp(30, 100);
    if op != cap.pin_opacity {
        cap.pin_opacity = op;
        notes.push(format!("贴图默认透明度已约束为 {op}%"));
    }

    let raw_dir = cap.save_dir.trim().to_string();
    if raw_dir.is_empty() {
        cap.save_dir.clear();
    } else {
        match validate_scan_dir(&raw_dir) {
            Ok(valid) => cap.save_dir = valid,
            Err(reason) => {
                notes.push(format!("截图保存目录 `{raw_dir}` 非法（{reason}），已恢复默认目录"));
                cap.save_dir.clear();
            }
        }
    }

    cap.tool_hotkeys.sanitize(notes);
}
