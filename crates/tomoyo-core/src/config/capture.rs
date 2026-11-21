//! Screenshot & Pin-to-Desktop (`tomoyo-capture.exe`) configuration and sanitization.

use super::{parse_hotkey, validate_scan_dir};
use serde::{Deserialize, Serialize};

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
}
