//! Inline preview-pane docking & detached window mode routing for WebView plugins.

use super::AppShell;
use crate::actions::spawn_ephemeral_webview_ex;
use crate::renderer::{FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP};
use std::ffi::c_void;
use std::path::PathBuf;
use tomoyo_core::{resolve_bundled_module_icon, TomoyoConfig};
use tomoyo_protocol::ActionKind;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, IsWindow, PostMessageW, SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOZORDER,
    WM_CLOSE,
};

fn inline_marker_path() -> PathBuf {
    tomoyo_core::logs_dir().join("active_inline_host.json")
}

fn read_active_inline_hwnd(expected_pid: Option<u32>) -> Option<(HWND, u32)> {
    let raw = std::fs::read_to_string(inline_marker_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let h_raw = v.get("hwnd").and_then(|x| x.as_i64())?;
    let pid = v.get("pid").and_then(|x| x.as_u64())? as u32;
    if let Some(exp) = expected_pid
        && exp != pid
    {
        return None;
    }
    let hwnd = HWND(h_raw as isize as *mut c_void);
    if !hwnd.is_invalid() && unsafe { IsWindow(Some(hwnd)).as_bool() } {
        Some((hwnd, pid))
    } else {
        None
    }
}

pub(crate) fn resolve_effective_display_mode(
    cfg: &TomoyoConfig,
    plugin_id: &str,
    action_id: &str,
    action_default: Option<&str>,
) -> &'static str {
    if action_id.ends_with("_window") {
        return "window";
    }
    if action_id.ends_with("_inline") {
        return "inline";
    }
    let key = plugin_id.trim().to_ascii_lowercase();
    if let Some(ov) = cfg.plugin_display_modes.get(&key) {
        if ov.eq_ignore_ascii_case("inline") {
            return "inline";
        }
        if ov.eq_ignore_ascii_case("window") {
            return "window";
        }
    }
    if action_default.is_some_and(|s| s.eq_ignore_ascii_case("inline")) {
        "inline"
    } else {
        "window"
    }
}

impl AppShell {
    pub(super) fn compute_inline_dock_rect(&self) -> (i32, i32, i32, i32) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let scale = self.dpi_scale.max(1.0);
        let w_dip = ((rc.right - rc.left) as f32 / scale).max(self.desired_width_dip() as f32);
        let h_dip = ((rc.bottom - rc.top) as f32 / scale).max(self.desired_height_dip() as f32);
        let pw = (self.geom.preview_width_dip as f32).clamp(220.0, (w_dip * 0.48).max(220.0));
        let list_w_dip = if w_dip >= 680.0 { (w_dip - pw).max(380.0) } else { w_dip * 0.54 };
        let top_dip = HEADER_HEIGHT_DIP as f32 + 2.0;
        let bot_dip = (h_dip - FOOTER_HEIGHT_DIP as f32 - 2.0).max(top_dip + 180.0);

        let mut pt = POINT {
            x: ((list_w_dip + 2.0) * scale).round() as i32,
            y: (top_dip * scale).round() as i32,
        };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut pt);
        }
        let w_px = ((w_dip - list_w_dip - 4.0) * scale).round().max(240.0) as i32;
        let h_px = ((bot_dip - top_dip) * scale).round().max(180.0) as i32;
        (pt.x, pt.y, w_px, h_px)
    }

    /// Launches a WebView plugin either docked inside Tomoyo's preview pane (`inline`) or detached (`window`).
    pub(super) fn try_execute_webview_action(
        &mut self,
        kind: &ActionKind,
        action_id: &str,
    ) -> Option<(bool, Option<String>)> {
        let ActionKind::OpenWebView {
            plugin_id,
            title,
            entry_path,
            utools_compat,
            initial_query,
            width,
            height,
            max_memory_mb,
            icon_path,
            display_mode,
        } = kind
        else {
            return None;
        };

        let resolved_icon = icon_path.clone().or_else(|| {
            resolve_bundled_module_icon(plugin_id).map(|p| p.to_string_lossy().into_owned())
        });
        let mode = self
            .config
            .read()
            .map(|c| resolve_effective_display_mode(&c, plugin_id, action_id, display_mode.as_deref()))
            .unwrap_or("window");

        if mode == "inline" {
            self.close_inline_dock();
            self.preview_enabled = true;
            self.inter.inline_dock_pid = Some(1);
            self.resize_to_fit_results();
            let rect = self.compute_inline_dock_rect();
            match spawn_ephemeral_webview_ex(
                plugin_id,
                title,
                entry_path,
                *utools_compat,
                initial_query,
                *width,
                *height,
                *max_memory_mb,
                self.theme.is_dark,
                resolved_icon.as_deref(),
                Some(self.hwnd.0 as isize),
                Some(rect),
            ) {
                Ok(pid) => {
                    self.inter.inline_dock_pid = Some(pid);
                    Some((
                        false,
                        Some(self.lang.pick(
                            "已嵌入右侧预览区 · 点击右上角「独立窗口」可分离",
                            "Docked inline · Click Pop-Out to detach window",
                        ).into()),
                    ))
                }
                Err(e) => {
                    self.inter.inline_dock_pid = None;
                    Some((false, Some(format!("WebView 内嵌启动失败: {e}"))))
                }
            }
        } else {
            self.close_inline_dock();
            match spawn_ephemeral_webview_ex(
                plugin_id,
                title,
                entry_path,
                *utools_compat,
                initial_query,
                *width,
                *height,
                *max_memory_mb,
                self.theme.is_dark,
                resolved_icon.as_deref(),
                None,
                None,
            ) {
                Ok(_) => Some((true, None)),
                Err(e) => Some((false, Some(format!("WebView 启动失败: {e}")))),
            }
        }
    }

    pub(super) fn sync_inline_dock_bounds(&self) {
        let Some(pid) = self.inter.inline_dock_pid else {
            return;
        };
        let Some((child, _)) = read_active_inline_hwnd(Some(pid)) else {
            return;
        };
        let (x, y, w, h) = self.compute_inline_dock_rect();
        unsafe {
            let _ = SetWindowPos(child, Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER);
        }
    }

    pub(super) fn is_inline_child_hwnd(&self, target: HWND) -> bool {
        if self.inter.inline_dock_pid.is_none() || target.is_invalid() {
            return false;
        }
        read_active_inline_hwnd(self.inter.inline_dock_pid).is_some_and(|(h, _)| h == target)
    }

    pub(super) fn close_inline_dock(&mut self) {
        let pid = self.inter.inline_dock_pid.take();
        if let Some(p) = pid
            && let Some((child, _)) = read_active_inline_hwnd(Some(p))
        {
            unsafe {
                let _ = PostMessageW(Some(child), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        let _ = std::fs::remove_file(inline_marker_path());
    }

    pub(super) fn on_inline_host_detached(&mut self) {
        self.inter.inline_dock_pid = None;
        let _ = std::fs::remove_file(inline_marker_path());
        if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool() } {
            self.begin_fade_hide();
        }
    }
}
