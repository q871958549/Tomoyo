//! Safe Rust FFI bridge to macOS AppKit `NSPanel` and Carbon `RegisterEventHotKey`.

use crate::panel::{parse_mac_hotkey, MacDisplayFrame, MacPanelGeometry, NSPanelStyleConfig};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn tomoyo_macos_primary_visible_frame(ox: *mut i32, oy: *mut i32, ow: *mut i32, oh: *mut i32) -> i32;
    fn tomoyo_macos_panel_create(x: i32, y: i32, w: i32, h: i32, corner_radius: i32) -> *mut std::ffi::c_void;
    fn tomoyo_macos_panel_show(panel: *mut std::ffi::c_void, x: i32, y: i32, w: i32, h: i32);
    fn tomoyo_macos_panel_hide(panel: *mut std::ffi::c_void);
    fn tomoyo_macos_panel_destroy(panel: *mut std::ffi::c_void);
    fn tomoyo_macos_hotkey_register(
        carbon_mods: u32,
        keycode: u16,
        id: u32,
        cb: unsafe extern "C" fn(*mut std::ffi::c_void, u32),
        ctx: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn tomoyo_macos_hotkey_unregister(hk: *mut std::ffi::c_void);
}

/// Queries the active display's Cocoa `visibleFrame` (excluding Menu Bar and Dock).
pub fn query_active_display_frame() -> Option<MacDisplayFrame> {
    #[cfg(target_os = "macos")]
    {
        let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
        let ok = unsafe { tomoyo_macos_primary_visible_frame(&mut x, &mut y, &mut w, &mut h) };
        (ok == 1 && w > 0 && h > 0).then_some(MacDisplayFrame { x, y, width: w, height: h })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Borderless non-activating `NSPanel` handle.
pub struct MacNativePanel {
    #[cfg(target_os = "macos")]
    raw: *mut std::ffi::c_void,
    pub style: NSPanelStyleConfig,
}

impl MacNativePanel {
    pub fn create_centered(width: i32, height: i32) -> Result<Self, String> {
        let style = NSPanelStyleConfig::launcher_default();
        #[cfg(target_os = "macos")]
        {
            let visible = query_active_display_frame()
                .ok_or_else(|| "no active NSScreen visibleFrame".to_string())?;
            let geom = MacPanelGeometry::compute_centered_frame(visible, width, height);
            let raw = unsafe {
                tomoyo_macos_panel_create(
                    geom.x,
                    geom.y,
                    geom.width,
                    geom.height,
                    i32::from(style.corner_radius_pt),
                )
            };
            if raw.is_null() {
                return Err("failed to allocate macOS NSPanel".into());
            }
            Ok(Self { raw, style })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (width, height, style, MacPanelGeometry::compute_centered_frame);
            Err("MacNativePanel requires macOS AppKit runtime".into())
        }
    }

    pub fn show_at(&self, geom: MacPanelGeometry) {
        #[cfg(target_os = "macos")]
        if !self.raw.is_null() {
            unsafe { tomoyo_macos_panel_show(self.raw, geom.x, geom.y, geom.width, geom.height) };
        }
        #[cfg(not(target_os = "macos"))]
        let _ = geom;
    }

    pub fn hide(&self) {
        #[cfg(target_os = "macos")]
        if !self.raw.is_null() {
            unsafe { tomoyo_macos_panel_hide(self.raw) };
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacNativePanel {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { tomoyo_macos_panel_destroy(self.raw) };
            self.raw = std::ptr::null_mut();
        }
    }
}

/// Carbon global hotkey registration guard (`RegisterEventHotKey`).
pub struct MacGlobalHotkey {
    #[cfg(target_os = "macos")]
    raw: *mut std::ffi::c_void,
    pub id: u32,
}

impl MacGlobalHotkey {
    pub fn register(
        spec: &str,
        id: u32,
        #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
        cb: unsafe extern "C" fn(*mut std::ffi::c_void, u32),
        #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
        ctx: *mut std::ffi::c_void,
    ) -> Result<Self, String> {
        let parsed = parse_mac_hotkey(spec).ok_or_else(|| format!("unsupported macOS hotkey `{spec}`"))?;
        #[cfg(target_os = "macos")]
        {
            let raw = unsafe {
                tomoyo_macos_hotkey_register(parsed.carbon_modifiers, parsed.ansi_keycode, id, cb, ctx)
            };
            if raw.is_null() {
                return Err(format!("RegisterEventHotKey failed for `{spec}`"));
            }
            Ok(Self { raw, id })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (id, parsed);
            Err("MacGlobalHotkey requires macOS Carbon runtime".into())
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacGlobalHotkey {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { tomoyo_macos_hotkey_unregister(self.raw) };
            self.raw = std::ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn noop_cb(_ctx: *mut std::ffi::c_void, _id: u32) {}

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_bridge_fails_cleanly_without_panicking() {
        assert!(query_active_display_frame().is_none());
        assert!(MacNativePanel::create_centered(680, 420).is_err());
        assert!(MacGlobalHotkey::register("Alt+Space", 1, noop_cb, std::ptr::null_mut()).is_err());
        assert!(MacGlobalHotkey::register("Unsupported+Key", 2, noop_cb, std::ptr::null_mut()).is_err());
    }
}
