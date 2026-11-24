//! Screenshot & Pin-to-Desktop (`tomoyo-capture.exe`) configuration, tool hotkeys, and sanitization.

use super::{parse_hotkey, validate_scan_dir};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

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
    #[serde(default = "hk_ocr_copy")]
    pub ocr_copy: String,
    #[serde(default = "hk_translate")]
    pub translate: String,
    #[serde(default = "hk_record")]
    pub record: String,
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
fn hk_ocr_copy() -> String { "Shift+C".into() }
fn hk_translate() -> String { "T".into() }
fn hk_record() -> String { "V".into() }

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
            ocr_copy: hk_ocr_copy(),
            translate: hk_translate(),
            record: hk_record(),
        }
    }
}

impl CaptureToolHotkeys {
    /// Normalizes an in-capture tool shortcut: single key (`A`–`Z`/`0`–`9`) or modifier combo
    /// (`Shift+C`, `Ctrl+O`, `Alt+C`, etc.), returning `None` if invalid.
    pub fn normalize_key(raw: &str) -> Option<String> {
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut key: Option<char> = None;
        for part in raw.trim().split('+') {
            let p = part.trim();
            if p.is_empty() {
                return None;
            }
            match p.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                "shift" => shift = true,
                s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphanumeric() => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(s.as_bytes()[0].to_ascii_uppercase() as char);
                }
                _ => return None,
            }
        }
        let k = key?;
        let mut out = String::new();
        if ctrl { out.push_str("Ctrl+"); }
        if alt { out.push_str("Alt+"); }
        if shift { out.push_str("Shift+"); }
        out.push(k);
        Some(out)
    }

    pub fn matches_combo(binding: &str, vk: u8, ctrl: bool, alt: bool, shift: bool) -> bool {
        let Some(norm) = Self::normalize_key(binding) else { return false; };
        let need_ctrl = norm.contains("Ctrl+");
        let need_alt = norm.contains("Alt+");
        let need_shift = norm.contains("Shift+");
        let Some(key_b) = norm.as_bytes().last().copied() else { return false; };
        ctrl == need_ctrl && alt == need_alt && shift == need_shift && key_b.eq_ignore_ascii_case(&vk)
    }

    pub fn sanitize(&mut self, notes: &mut Vec<String>) {
        let defaults = Self::default();
        let mut used: HashSet<String> = HashSet::new();
        let fields: [(&mut String, &str, &str); 14] = [
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
            (&mut self.ocr_copy, &defaults.ocr_copy, "直接提取文字"),
            (&mut self.translate, &defaults.translate, "翻译"),
            (&mut self.record, &defaults.record, "录屏"),
        ];
        for (slot, def, label) in fields {
            if let Some(norm) = Self::normalize_key(slot)
                && used.insert(norm.clone()) {
                    *slot = norm;
                    continue;
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
    /// Global shortcut to pin the latest clipboard content (image/file/text/color) to desktop (default `"F3"`, empty disables).
    #[serde(default = "default_pin_hotkey")]
    pub pin_hotkey: String,
    /// Optional global shortcut to start region capture and extract text via offline OCR directly (empty disables).
    #[serde(default)]
    pub ocr_hotkey: String,
    /// Optional global shortcut to start region screen recording directly (empty disables).
    #[serde(default)]
    pub record_hotkey: String,
    /// Optional custom save folder for PNG exports (empty = `%USERPROFILE%\Pictures\Tomoyo`).
    #[serde(default)]
    pub save_dir: String,
    /// Default opacity percentage (`30..=100`, default `100`) for pinned desktop images.
    #[serde(default = "default_pin_opacity")]
    pub pin_opacity: u8,
    /// Copy picked color hex in lowercase (`#ff6b6b` vs `#FF6B6B`).
    #[serde(default)]
    pub copy_hex_lowercase: bool,
    /// Preferred recording output format (`"mp4"` or `"gif"`).
    #[serde(default = "default_record_format")]
    pub record_format: String,
    /// Preferred recording frame rate (`15`, `30`, or `60`).
    #[serde(default = "default_record_fps")]
    pub record_fps: u8,
    /// Preferred recording audio source (`"none"`, `"system"`, `"mic"`, or `"both"`).
    #[serde(default = "default_record_audio")]
    pub record_audio: String,
    /// Include mouse cursor in screen recordings (default `true`).
    #[serde(default = "default_record_cursor")]
    pub record_cursor: bool,
    /// Customizable single-key shortcuts for in-capture annotation & action tools.
    #[serde(default)]
    pub tool_hotkeys: CaptureToolHotkeys,
}

fn default_capture_hotkey() -> String {
    "Alt+S".into()
}

fn default_pin_hotkey() -> String {
    "F3".into()
}

fn default_pin_opacity() -> u8 {
    100
}

fn default_record_format() -> String {
    "mp4".into()
}

fn default_record_fps() -> u8 {
    30
}

fn default_record_audio() -> String {
    "none".into()
}

fn default_record_cursor() -> bool {
    true
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            hotkey: default_capture_hotkey(),
            pin_hotkey: default_pin_hotkey(),
            ocr_hotkey: String::new(),
            record_hotkey: String::new(),
            save_dir: String::new(),
            pin_opacity: default_pin_opacity(),
            copy_hex_lowercase: false,
            record_format: default_record_format(),
            record_fps: default_record_fps(),
            record_audio: default_record_audio(),
            record_cursor: default_record_cursor(),
            tool_hotkeys: CaptureToolHotkeys::default(),
        }
    }
}

impl CaptureConfig {
    /// Overlays Protocol v3 `plugin_settings["dev.tomoyo.capture"]` onto legacy `[capture]` fields
    /// so existing users keep 100% of their settings while new edits in the Capture plugin drawer take effect live.
    pub fn with_plugin_overrides(
        &self,
        bag: Option<&BTreeMap<String, serde_json::Value>>,
    ) -> Self {
        let mut out = self.clone();
        let Some(map) = bag else { return out; };
        if let Some(s) = map.get("save_dir").and_then(|v| v.as_str()) {
            out.save_dir = s.trim().to_string();
        }
        if let Some(n) = map.get("pin_opacity").and_then(|v| v.as_u64()) {
            out.pin_opacity = (n as u8).clamp(30, 100);
        }
        if let Some(b) = map.get("copy_hex_lowercase").and_then(|v| v.as_bool()) {
            out.copy_hex_lowercase = b;
        }
        if let Some(s) = map.get("record_format").and_then(|v| v.as_str())
            && matches!(s, "mp4" | "gif")
        {
            out.record_format = s.to_string();
        }
        if let Some(fps) = map.get("record_fps").and_then(|v| {
            v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        }) && matches!(fps, 15 | 24 | 30 | 60)
        {
            out.record_fps = fps as u8;
        }
        if let Some(s) = map.get("record_audio").and_then(|v| v.as_str())
            && matches!(s, "none" | "system" | "mic" | "both")
        {
            out.record_audio = s.to_string();
        }
        if let Some(b) = map.get("record_cursor").and_then(|v| v.as_bool()) {
            out.record_cursor = b;
        }
        let ov_hk = |k: &str, slot: &mut String| {
            if let Some(raw) = map.get(k).and_then(|v| v.as_str())
                && let Some(norm) = CaptureToolHotkeys::normalize_key(raw)
            {
                *slot = norm;
            }
        };
        ov_hk("tool_rect", &mut out.tool_hotkeys.rect);
        ov_hk("tool_ellipse", &mut out.tool_hotkeys.ellipse);
        ov_hk("tool_arrow", &mut out.tool_hotkeys.arrow);
        ov_hk("tool_pen", &mut out.tool_hotkeys.pen);
        ov_hk("tool_highlighter", &mut out.tool_hotkeys.highlighter);
        ov_hk("tool_mosaic", &mut out.tool_hotkeys.mosaic);
        ov_hk("tool_step", &mut out.tool_hotkeys.step);
        ov_hk("tool_text", &mut out.tool_hotkeys.text);
        ov_hk("tool_scroll", &mut out.tool_hotkeys.scroll);
        ov_hk("tool_pin", &mut out.tool_hotkeys.pin);
        ov_hk("tool_ocr", &mut out.tool_hotkeys.ocr);
        ov_hk("tool_ocr_copy", &mut out.tool_hotkeys.ocr_copy);
        ov_hk("tool_translate", &mut out.tool_hotkeys.translate);
        ov_hk("tool_record", &mut out.tool_hotkeys.record);
        out
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

    let phk = cap.pin_hotkey.trim().to_string();
    if phk.is_empty() {
        cap.pin_hotkey.clear();
    } else if parse_hotkey(&phk).is_none()
        || phk.eq_ignore_ascii_case(main_hk)
        || (!clip_hk.is_empty() && phk.eq_ignore_ascii_case(clip_hk))
        || (!tr_hk.is_empty() && phk.eq_ignore_ascii_case(tr_hk))
        || (!cap.hotkey.is_empty() && phk.eq_ignore_ascii_case(&cap.hotkey))
    {
        notes.push(format!("全局贴图快捷键 `{phk}` 无效或与已有快捷键冲突，已停用"));
        cap.pin_hotkey.clear();
    } else {
        cap.pin_hotkey = phk;
    }

    let ohk = cap.ocr_hotkey.trim().to_string();
    if ohk.is_empty() {
        cap.ocr_hotkey.clear();
    } else if parse_hotkey(&ohk).is_none()
        || ohk.eq_ignore_ascii_case(main_hk)
        || (!clip_hk.is_empty() && ohk.eq_ignore_ascii_case(clip_hk))
        || (!tr_hk.is_empty() && ohk.eq_ignore_ascii_case(tr_hk))
        || (!cap.hotkey.is_empty() && ohk.eq_ignore_ascii_case(&cap.hotkey))
        || (!cap.pin_hotkey.is_empty() && ohk.eq_ignore_ascii_case(&cap.pin_hotkey))
    {
        notes.push(format!("全局截图识字快捷键 `{ohk}` 无效或与已有快捷键冲突，已停用"));
        cap.ocr_hotkey.clear();
    } else {
        cap.ocr_hotkey = ohk;
    }

    let rhk = cap.record_hotkey.trim().to_string();
    if rhk.is_empty() {
        cap.record_hotkey.clear();
    } else if parse_hotkey(&rhk).is_none()
        || rhk.eq_ignore_ascii_case(main_hk)
        || (!clip_hk.is_empty() && rhk.eq_ignore_ascii_case(clip_hk))
        || (!tr_hk.is_empty() && rhk.eq_ignore_ascii_case(tr_hk))
        || (!cap.hotkey.is_empty() && rhk.eq_ignore_ascii_case(&cap.hotkey))
        || (!cap.pin_hotkey.is_empty() && rhk.eq_ignore_ascii_case(&cap.pin_hotkey))
        || (!cap.ocr_hotkey.is_empty() && rhk.eq_ignore_ascii_case(&cap.ocr_hotkey))
    {
        notes.push(format!("全局录屏快捷键 `{rhk}` 无效或与已有快捷键冲突，已停用"));
        cap.record_hotkey.clear();
    } else {
        cap.record_hotkey = rhk;
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

    if !matches!(cap.record_format.as_str(), "mp4" | "gif") {
        cap.record_format = default_record_format();
    }
    if !matches!(cap.record_fps, 15 | 30 | 60) {
        cap.record_fps = default_record_fps();
    }
    if !matches!(cap.record_audio.as_str(), "none" | "system" | "mic" | "both") {
        cap.record_audio = default_record_audio();
    }
    cap.tool_hotkeys.sanitize(notes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_combo_handles_single_and_modified_shortcuts() {
        assert!(CaptureToolHotkeys::matches_combo("O", b'O', false, false, false));
        assert!(!CaptureToolHotkeys::matches_combo("O", b'O', false, false, true));
        assert!(CaptureToolHotkeys::matches_combo("Shift+C", b'C', false, false, true));
        assert!(!CaptureToolHotkeys::matches_combo("Shift+C", b'C', false, false, false));
        assert!(!CaptureToolHotkeys::matches_combo("Shift+C", b'C', true, false, true));
        assert!(CaptureToolHotkeys::matches_combo("Ctrl+Shift+X", b'X', true, false, true));
    }

    #[test]
    fn sanitize_normalizes_and_deduplicates_tool_hotkeys() {
        let mut hk = CaptureToolHotkeys {
            ocr_copy: "shift+c".into(),
            ocr: "o".into(),
            ..Default::default()
        };
        let mut notes = Vec::new();
        hk.sanitize(&mut notes);
        assert_eq!(hk.ocr_copy, "Shift+C");
        assert_eq!(hk.ocr, "O");
        assert!(notes.is_empty());
    }
}
