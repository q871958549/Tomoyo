//! XDG Desktop Portal `GlobalShortcuts` fallback + hotkey keycode translation.

use std::ffi::{c_char, c_void, CString};
use tomoyo_core::parse_hotkey;

unsafe extern "C" {
    fn tomoyo_kga_is_available() -> i32;
    fn tomoyo_portal_hk_connect(
        cb: unsafe extern "C" fn(*mut c_void, *const c_char),
        ctx: *mut c_void,
    ) -> *mut c_void;
    fn tomoyo_portal_hk_bind(
        phk: *mut c_void,
        ids: *const *const c_char,
        descs: *const *const c_char,
        triggers: *const *const c_char,
        count: usize,
    ) -> i32;
    fn tomoyo_portal_hk_destroy(phk: *mut c_void);
}

pub(super) struct PortalSlot {
    pub id: String,
    pub desc: String,
    pub trigger: String,
}

pub(super) struct PortalHotkeyHandle {
    raw: *mut c_void,
}

impl PortalHotkeyHandle {
    pub fn connect(
        cb: unsafe extern "C" fn(*mut c_void, *const c_char),
        ctx: *mut c_void,
    ) -> Option<Self> {
        let raw = unsafe { tomoyo_portal_hk_connect(cb, ctx) };
        (!raw.is_null()).then_some(Self { raw })
    }

    pub fn bind(&self, slots: &[PortalSlot]) -> bool {
        if self.raw.is_null() || slots.is_empty() {
            return false;
        }
        let ids: Vec<CString> = slots
            .iter()
            .filter_map(|s| CString::new(s.id.as_str()).ok())
            .collect();
        let descs: Vec<CString> = slots
            .iter()
            .filter_map(|s| CString::new(s.desc.as_str()).ok())
            .collect();
        let triggers: Vec<CString> = slots
            .iter()
            .filter_map(|s| CString::new(s.trigger.as_str()).ok())
            .collect();
        let count = ids.len().min(descs.len()).min(triggers.len());
        if count == 0 {
            return false;
        }
        let id_ptrs: Vec<*const c_char> = ids.iter().take(count).map(|c| c.as_ptr()).collect();
        let desc_ptrs: Vec<*const c_char> = descs.iter().take(count).map(|c| c.as_ptr()).collect();
        let trig_ptrs: Vec<*const c_char> = triggers.iter().take(count).map(|c| c.as_ptr()).collect();
        unsafe {
            tomoyo_portal_hk_bind(
                self.raw,
                id_ptrs.as_ptr(),
                desc_ptrs.as_ptr(),
                trig_ptrs.as_ptr(),
                count,
            ) == 1
        }
    }
}

impl Drop for PortalHotkeyHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { tomoyo_portal_hk_destroy(self.raw); }
            self.raw = std::ptr::null_mut();
        }
    }
}

pub(super) fn kga_service_available() -> bool {
    unsafe { tomoyo_kga_is_available() == 1 }
}

/// Converts a Tomoyo hotkey string (`Alt+Space`, `Ctrl+Shift+V`) to a Qt keycode for KGlobalAccel.
pub(super) fn spec_to_qt_key(spec: &str) -> Option<i32> {
    let p = parse_hotkey(spec)?;
    let mut qt: i32 = 0;
    if (p.modifiers & 0x0004) != 0 { qt |= 0x0200_0000; }
    if (p.modifiers & 0x0002) != 0 { qt |= 0x0400_0000; }
    if (p.modifiers & 0x0001) != 0 { qt |= 0x0800_0000; }
    if (p.modifiers & 0x0008) != 0 { qt |= 0x1000_0000; }
    let key: i32 = match p.vk {
        0x20 => 0x20,
        0x30..=0x39 | 0x41..=0x5A => p.vk as i32,
        0x09 => 0x0100_0001,
        0x0D => 0x0100_0004,
        0x2D => 0x0100_0006,
        0x2E => 0x0100_0007,
        0x2C => 0x0100_0009,
        0x24 => 0x0100_0010,
        0x23 => 0x0100_0011,
        0x25 => 0x0100_0012,
        0x26 => 0x0100_0013,
        0x27 => 0x0100_0014,
        0x28 => 0x0100_0015,
        0x21 => 0x0100_0016,
        0x22 => 0x0100_0017,
        0x70..=0x7B => 0x0100_0030 + (p.vk as i32 - 0x70),
        0xC0 => 0x60,
        0xBD => 0x2D,
        0xBB => 0x3D,
        0xDB => 0x5B,
        0xDD => 0x5D,
        0xDC => 0x5C,
        0xBA => 0x3B,
        0xDE => 0x27,
        0xBC => 0x2C,
        0xBE => 0x2E,
        0xBF => 0x2F,
        _ => return None,
    };
    Some(qt | key)
}

/// Converts a Tomoyo hotkey string to an XDG GlobalShortcuts `preferred_trigger` string.
pub(super) fn spec_to_portal_trigger(spec: &str) -> Option<String> {
    let p = parse_hotkey(spec)?;
    let mut parts: Vec<String> = Vec::with_capacity(5);
    if (p.modifiers & 0x0002) != 0 { parts.push("CTRL".into()); }
    if (p.modifiers & 0x0001) != 0 { parts.push("ALT".into()); }
    if (p.modifiers & 0x0004) != 0 { parts.push("SHIFT".into()); }
    if (p.modifiers & 0x0008) != 0 { parts.push("LOGO".into()); }
    let key = match p.vk {
        0x20 => "space".into(),
        0x09 => "Tab".into(),
        0x0D => "Return".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x2C => "Print".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x21 => "Page_Up".into(),
        0x22 => "Page_Down".into(),
        0xC0 => "grave".into(),
        0xBD => "minus".into(),
        0xBB => "equal".into(),
        0xDB => "bracketleft".into(),
        0xDD => "bracketright".into(),
        0xDC => "backslash".into(),
        0xBA => "semicolon".into(),
        0xDE => "apostrophe".into(),
        0xBC => "comma".into(),
        0xBE => "period".into(),
        0xBF => "slash".into(),
        0x30..=0x39 | 0x41..=0x5A => ((p.vk as u8) as char).to_ascii_lowercase().to_string(),
        0x70..=0x7B => format!("F{}", p.vk - 0x6F),
        _ => return None,
    };
    parts.push(key);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::{spec_to_portal_trigger, spec_to_qt_key, PortalHotkeyHandle, PortalSlot};

    #[test]
    fn maps_hotkeys_to_xdg_portal_and_qt_triggers() {
        assert_eq!(spec_to_portal_trigger("Alt+Space").as_deref(), Some("ALT+space"));
        assert_eq!(spec_to_portal_trigger("Ctrl+Shift+V").as_deref(), Some("CTRL+SHIFT+v"));
        assert_eq!(spec_to_portal_trigger("Super+Left").as_deref(), Some("LOGO+Left"));
        assert_eq!(spec_to_portal_trigger("F2").as_deref(), Some("F2"));
        assert!(spec_to_qt_key("Alt+Space").is_some());
    }

    #[test]
    fn inert_portal_handle_ignores_bind_and_drop_safely() {
        let h = PortalHotkeyHandle { raw: std::ptr::null_mut() };
        let slots = [PortalSlot {
            id: "main".into(),
            desc: "Tomoyo".into(),
            trigger: "ALT+space".into(),
        }];
        assert!(!h.bind(&slots));
    }
}
