//! Pure-Rust macOS `NSPanel` geometry, Carbon hotkey & screen-frame contracts.

/// Visible usable screen rectangle in Cocoa logical points (origin bottom-left).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacDisplayFrame {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Computed `NSPanel` placement inside a display's `visibleFrame` (respecting Menu Bar & Dock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacPanelGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl MacPanelGeometry {
    /// Places the launcher horizontally centered and at ~28% from the top of `visible`.
    pub fn compute_centered_frame(visible: MacDisplayFrame, width: i32, height: i32) -> Self {
        let w = width.clamp(360, visible.width.max(360));
        let h = height.clamp(64, visible.height.max(64));
        let x = visible.x + (visible.width - w).max(0) / 2;
        // Cocoa Y=0 is bottom; topOffset = 22%..28% places panel top neatly above center.
        let top_offset = (visible.height as f32 * 0.24).round() as i32;
        let top_y = visible.y + (visible.height - top_offset).clamp(h, visible.height);
        let y = (top_y - h).max(visible.y);
        Self { x, y, width: w, height: h }
    }

    /// Clamps a user-dragged saved `(x, y)` inside `visible` bounds so displays that detach never orphan the panel.
    pub fn clamp_saved_position(visible: MacDisplayFrame, saved_x: i32, saved_y: i32, width: i32, height: i32) -> Self {
        let w = width.clamp(360, visible.width.max(360));
        let h = height.clamp(64, visible.height.max(64));
        let max_x = visible.x + (visible.width - w).max(0);
        let max_y = visible.y + (visible.height - h).max(0);
        Self {
            x: saved_x.clamp(visible.x, max_x),
            y: saved_y.clamp(visible.y, max_y),
            width: w,
            height: h,
        }
    }
}

/// Declarative AppKit `NSPanel` style configuration (`NSWindowCollectionBehavior` + non-activating floating).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NSPanelStyleConfig {
    pub floating_level: bool,
    pub non_activating_panel: bool,
    pub can_join_all_spaces: bool,
    pub full_screen_auxiliary: bool,
    pub hides_on_deactivate: bool,
    pub movable_by_background: bool,
    pub corner_radius_pt: u16,
}

impl NSPanelStyleConfig {
    pub const fn launcher_default() -> Self {
        Self {
            floating_level: true,
            non_activating_panel: true,
            can_join_all_spaces: true,
            full_screen_auxiliary: true,
            hides_on_deactivate: true,
            movable_by_background: false,
            corner_radius_pt: 14,
        }
    }
}

/// Carbon `RegisterEventHotKey` mask & ANSI virtual keycode representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacHotkeySpec {
    /// Carbon modifier bits (`cmdKey=0x0100`, `shiftKey=0x0200`, `optionKey=0x0800`, `controlKey=0x1000`).
    pub carbon_modifiers: u32,
    /// HIToolbox `kVK_*` virtual keycode.
    pub ansi_keycode: u16,
}

pub fn parse_mac_hotkey(spec: &str) -> Option<MacHotkeySpec> {
    let mut mods = 0u32;
    let mut kc: Option<u16> = None;
    for raw in spec.split('+') {
        let part = raw.trim().to_ascii_lowercase();
        match part.as_str() {
            "cmd" | "command" | "super" | "meta" | "win" => mods |= 0x0100,
            "shift" => mods |= 0x0200,
            "alt" | "option" | "opt" => mods |= 0x0800,
            "ctrl" | "control" => mods |= 0x1000,
            "space" => kc = Some(0x31),
            "return" | "enter" => kc = Some(0x24),
            "tab" => kc = Some(0x30),
            "left" => kc = Some(0x7B),
            "right" => kc = Some(0x7C),
            "down" => kc = Some(0x7D),
            "up" => kc = Some(0x7E),
            s if s.len() == 1 => {
                kc = ansi_char_keycode(s.as_bytes()[0].to_ascii_uppercase());
                kc?;
            }
            s if s.starts_with('f') && s.len() <= 3 => {
                let n = s[1..].parse::<u8>().ok()?;
                kc = ansi_fkey_keycode(n);
                kc?;
            }
            _ => return None,
        }
    }
    Some(MacHotkeySpec {
        carbon_modifiers: mods,
        ansi_keycode: kc?,
    })
}

const fn ansi_char_keycode(ch: u8) -> Option<u16> {
    Some(match ch {
        b'A' => 0x00, b'S' => 0x01, b'D' => 0x02, b'F' => 0x03, b'H' => 0x04, b'G' => 0x05,
        b'Z' => 0x06, b'X' => 0x07, b'C' => 0x08, b'V' => 0x09, b'B' => 0x0B, b'Q' => 0x0C,
        b'W' => 0x0D, b'E' => 0x0E, b'R' => 0x0F, b'Y' => 0x10, b'T' => 0x11, b'1' => 0x12,
        b'2' => 0x13, b'3' => 0x14, b'4' => 0x15, b'6' => 0x16, b'5' => 0x17, b'9' => 0x19,
        b'7' => 0x1A, b'8' => 0x1C, b'0' => 0x1D, b'O' => 0x1F, b'U' => 0x20, b'I' => 0x22,
        b'P' => 0x23, b'L' => 0x25, b'J' => 0x26, b'K' => 0x28, b'N' => 0x2D, b'M' => 0x2E,
        _ => return None,
    })
}

const fn ansi_fkey_keycode(n: u8) -> Option<u16> {
    Some(match n {
        1 => 0x7A, 2 => 0x78, 3 => 0x63, 4 => 0x76, 5 => 0x60, 6 => 0x61,
        7 => 0x62, 8 => 0x64, 9 => 0x65, 10 => 0x6D, 11 => 0x67, 12 => 0x6F,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_and_clamps_panel_within_cocoa_visible_frame() {
        let screen = MacDisplayFrame { x: 0, y: 74, width: 1728, height: 1007 };
        let centered = MacPanelGeometry::compute_centered_frame(screen, 680, 420);
        assert_eq!(centered.x, (1728 - 680) / 2);
        assert!(centered.y >= screen.y);
        assert!(centered.y + centered.height <= screen.y + screen.height);

        let clamped = MacPanelGeometry::clamp_saved_position(screen, 2500, -100, 680, 420);
        assert_eq!(clamped.x, 1728 - 680);
        assert_eq!(clamped.y, 74);
    }

    #[test]
    fn parses_tomoyo_hotkeys_into_carbon_keycodes() {
        let alt_space = parse_mac_hotkey("Alt+Space").unwrap();
        assert_eq!(alt_space, MacHotkeySpec { carbon_modifiers: 0x0800, ansi_keycode: 0x31 });
        let cmd_shift_v = parse_mac_hotkey("Cmd+Shift+V").unwrap();
        assert_eq!(cmd_shift_v, MacHotkeySpec { carbon_modifiers: 0x0300, ansi_keycode: 0x09 });
        let f2 = parse_mac_hotkey("F2").unwrap();
        assert_eq!(f2, MacHotkeySpec { carbon_modifiers: 0, ansi_keycode: 0x78 });
        assert_eq!(NSPanelStyleConfig::launcher_default().corner_radius_pt, 14);
    }
}
