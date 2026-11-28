//! Hyprland and Sway native Wayland tiling/probing helpers for `dev.tomoyo.winlayout`.

use crate::apply::ActiveWindowInfo;
use crate::layouts::{compute_rect, WorkArea};
use serde_json::Value;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_hyprland_active_window_json(raw: &str) -> Option<ActiveWindowInfo> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let title = v.get("title").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty())?;
    let app = v
        .get("class")
        .and_then(Value::as_str)
        .or_else(|| v.get("initialClass").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Hyprland");
    if title.to_lowercase().contains("tomoyo") || app.to_lowercase().contains("tomoyo") {
        return None;
    }
    Some(ActiveWindowInfo { app: app.into(), title: title.into() })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn find_focused_sway_node(node: &Value) -> Option<ActiveWindowInfo> {
    if node.get("focused").and_then(Value::as_bool) == Some(true)
        && let Some(title) = node.get("name").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty())
    {
        let app = node
            .get("app_id")
            .and_then(Value::as_str)
            .or_else(|| node.get("window_properties").and_then(|w| w.get("class")).and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Sway");
        if title.to_lowercase().contains("tomoyo") || app.to_lowercase().contains("tomoyo") {
            return None;
        }
        return Some(ActiveWindowInfo { app: app.into(), title: title.into() });
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(arr) = node.get(key).and_then(Value::as_array) {
            for child in arr {
                if let Some(hit) = find_focused_sway_node(child) {
                    return Some(hit);
                }
            }
        }
    }
    None
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_sway_focused_node_json(raw: &str) -> Option<ActiveWindowInfo> {
    let v: Value = serde_json::from_str(raw).ok()?;
    find_focused_sway_node(&v)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_hyprland_focused_monitor_json(raw: &str) -> Option<WorkArea> {
    let arr = serde_json::from_str::<Value>(raw).ok()?.as_array()?.clone();
    let mon = arr
        .iter()
        .find(|m| m.get("focused").and_then(Value::as_bool) == Some(true))
        .or_else(|| arr.first())?;
    let scale = mon.get("scale").and_then(Value::as_f64).filter(|&s| s > 0.1).unwrap_or(1.0);
    let raw_w = mon.get("width").and_then(Value::as_i64)?;
    let raw_h = mon.get("height").and_then(Value::as_i64)?;
    let x = mon.get("x").and_then(Value::as_i64).unwrap_or(0) as i32;
    let y = mon.get("y").and_then(Value::as_i64).unwrap_or(0) as i32;
    let mut w = (raw_w as f64 / scale).round() as i32;
    let mut h = (raw_h as f64 / scale).round() as i32;
    let mut off_x = 0i32;
    let mut off_y = 0i32;
    if let Some(res) = mon.get("reserved").and_then(Value::as_array)
        && res.len() == 4
    {
        let left = res[0].as_i64().unwrap_or(0) as i32;
        let top = res[1].as_i64().unwrap_or(0) as i32;
        let right = res[2].as_i64().unwrap_or(0) as i32;
        let bottom = res[3].as_i64().unwrap_or(0) as i32;
        off_x = left.max(0);
        off_y = top.max(0);
        w = (w - off_x - right.max(0)).max(240);
        h = (h - off_y - bottom.max(0)).max(180);
    }
    Some(WorkArea { x: x + off_x, y: y + off_y, w, h })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_sway_focused_output_json(raw: &str) -> Option<WorkArea> {
    let arr = serde_json::from_str::<Value>(raw).ok()?.as_array()?.clone();
    let out = arr
        .iter()
        .find(|o| o.get("focused").and_then(Value::as_bool) == Some(true))
        .or_else(|| arr.iter().find(|o| o.get("active").and_then(Value::as_bool) == Some(true)))?;
    let rect = out.get("rect")?;
    let x = rect.get("x").and_then(Value::as_i64)? as i32;
    let y = rect.get("y").and_then(Value::as_i64)? as i32;
    let w = (rect.get("width").and_then(Value::as_i64)? as i32).max(240);
    let h = (rect.get("height").and_then(Value::as_i64)? as i32).max(180);
    Some(WorkArea { x, y, w, h })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn hyprland_dispatch_batch(
    preset_id: &str,
    area: WorkArea,
    gap_px: i32,
    center_pct: i32,
) -> Option<Vec<String>> {
    match preset_id {
        "max" => Some(vec!["dispatch fullscreen 1".into()]),
        "always-top" => Some(vec!["dispatch pin".into()]),
        _ => {
            let r = compute_rect(preset_id, area, gap_px, center_pct)?;
            Some(vec![
                "dispatch setfloating active".into(),
                format!("dispatch resizeactive exact {} {}", r.w, r.h),
                format!("dispatch moveactive exact {} {}", r.x, r.y),
            ])
        }
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn sway_command_for_preset(
    preset_id: &str,
    area: WorkArea,
    gap_px: i32,
    center_pct: i32,
) -> Option<String> {
    match preset_id {
        "max" => Some("fullscreen toggle".into()),
        "always-top" => Some("sticky toggle".into()),
        _ => {
            let r = compute_rect(preset_id, area, gap_px, center_pct)?;
            Some(format!(
                "floating enable, resize set {} px {} px, move position {} px {} px",
                r.w, r.h, r.x, r.y
            ))
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn try_apply_hyprland(preset_id: &str, gap_px: i32, center_pct: i32) -> bool {
    if !std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    let area = std::process::Command::new("hyprctl")
        .args(["-j", "monitors"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_hyprland_focused_monitor_json(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(WorkArea { x: 0, y: 0, w: 1920, h: 1080 });
    let Some(cmds) = hyprland_dispatch_batch(preset_id, area, gap_px, center_pct) else {
        return false;
    };
    let batch = cmds.join(" ; ");
    std::process::Command::new("hyprctl")
        .args(["--batch", &batch])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(target_os = "linux")]
pub(crate) fn try_apply_sway(preset_id: &str, gap_px: i32, center_pct: i32) -> bool {
    if !std::env::var_os("SWAYSOCK").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    let area = std::process::Command::new("swaymsg")
        .args(["-t", "get_outputs"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_sway_focused_output_json(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(WorkArea { x: 0, y: 0, w: 1920, h: 1080 });
    let Some(cmd) = sway_command_for_preset(preset_id, area, gap_px, center_pct) else {
        return false;
    };
    std::process::Command::new("swaymsg")
        .arg(&cmd)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hyprland_and_sway_active_window_json() {
        let h = parse_hyprland_active_window_json(r#"{"class":"kitty","title":"nvim main.rs"}"#).unwrap();
        assert_eq!(h.app, "kitty");
        assert_eq!(h.title, "nvim main.rs");
        assert!(parse_hyprland_active_window_json(r#"{"class":"tomoyo","title":"Tomoyo"}"#).is_none());

        let sw = parse_sway_focused_node_json(
            r#"{"nodes":[{"focused":false,"nodes":[{"focused":true,"app_id":"org.kde.dolphin","name":"Downloads"}]}]}"#,
        )
        .unwrap();
        assert_eq!(sw.app, "org.kde.dolphin");
        assert_eq!(sw.title, "Downloads");
    }

    #[test]
    fn parses_monitors_and_builds_hyprland_sway_batches() {
        let hypr_mon = r#"[{"id":0,"x":0,"y":0,"width":3840,"height":2160,"scale":1.5,"reserved":[0,40,0,0],"focused":true}]"#;
        let area = parse_hyprland_focused_monitor_json(hypr_mon).unwrap();
        assert_eq!(area, WorkArea { x: 0, y: 40, w: 2560, h: 1400 });

        let batch = hyprland_dispatch_batch("left", area, 10, 72).unwrap();
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0], "dispatch setfloating active");
        assert!(batch[1].starts_with("dispatch resizeactive exact 1265 1380"));
        assert_eq!(batch[2], "dispatch moveactive exact 10 50");
        assert_eq!(hyprland_dispatch_batch("max", area, 0, 72).unwrap(), vec!["dispatch fullscreen 1"]);
        assert!(hyprland_dispatch_batch("minimize-others", area, 0, 72).is_none());

        let sway_out = r#"[{"name":"DP-1","active":true,"focused":true,"rect":{"x":1920,"y":0,"width":2560,"height":1440}}]"#;
        let sarea = parse_sway_focused_output_json(sway_out).unwrap();
        assert_eq!(sarea, WorkArea { x: 1920, y: 0, w: 2560, h: 1440 });
        let scmd = sway_command_for_preset("center", sarea, 0, 50).unwrap();
        assert!(scmd.contains("floating enable"));
        assert!(scmd.contains("resize set 1280 px 720 px"));
        assert!(scmd.contains("move position 2560 px 360 px"));
        assert_eq!(sway_command_for_preset("always-top", sarea, 0, 50).as_deref(), Some("sticky toggle"));
    }
}
