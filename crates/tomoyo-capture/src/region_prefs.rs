//! Persisted last-capture region (`--repeat-region`) and cross-process Pin Boss-Key broadcast (`--toggle-pins`).

#![cfg_attr(not(windows), allow(dead_code))]

use crate::screen::ScreenRect;
use serde_json::json;
use std::path::{Path, PathBuf};

fn last_region_path() -> PathBuf {
    tomoyo_core::data_dir().join("capture_last_region.json")
}

pub fn save_last_region(origin_x: i32, origin_y: i32, sel: ScreenRect) {
    save_last_region_to(&last_region_path(), origin_x, origin_y, sel);
}

pub fn load_last_region() -> Option<(i32, i32, ScreenRect)> {
    load_last_region_from(&last_region_path())
}

pub fn save_last_region_to(path: &Path, origin_x: i32, origin_y: i32, sel: ScreenRect) {
    if !sel.is_valid() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = json!({
        "origin_x": origin_x,
        "origin_y": origin_y,
        "x": sel.x,
        "y": sel.y,
        "w": sel.w,
        "h": sel.h,
    });
    let _ = std::fs::write(path, payload.to_string());
}

pub fn load_last_region_from(path: &Path) -> Option<(i32, i32, ScreenRect)> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let origin_x = v.get("origin_x")?.as_i64()? as i32;
    let origin_y = v.get("origin_y")?.as_i64()? as i32;
    let rect = ScreenRect {
        x: v.get("x")?.as_i64()? as i32,
        y: v.get("y")?.as_i64()? as i32,
        w: v.get("w")?.as_i64()? as i32,
        h: v.get("h")?.as_i64()? as i32,
    };
    rect.is_valid().then_some((origin_x, origin_y, rect))
}

#[cfg(windows)]
pub fn toggle_all_pins_msg() -> u32 {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
    unsafe { RegisterWindowMessageW(w!("TomoyoToggleAllPinsMsg")) }
}

#[cfg(windows)]
pub fn broadcast_toggle_all_pins() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HWND_BROADCAST};
    let msg = toggle_all_pins_msg();
    if msg != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND_BROADCAST), msg, WPARAM(0), LPARAM(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_reloads_last_capture_region() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_cap_reg_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let r = ScreenRect { x: 120, y: 80, w: 640, h: 360 };
        save_last_region_to(&tmp, -1920, 0, r);
        assert_eq!(load_last_region_from(&tmp), Some((-1920, 0, r)));

        let _ = std::fs::remove_file(&tmp);
    }
}
