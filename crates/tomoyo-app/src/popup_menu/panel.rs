//! Popup menu panel state, geometry measurement, multi-monitor placement & keyboard stepping.

use super::{PopupEntry, RowMetric};
use crate::renderer::DibSurface;
use std::ffi::c_void;
use windows::core::w;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetWindowRect, SetWindowLongPtrW, SetWindowPos, GWLP_USERDATA, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOZORDER, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const ROW_H_DIP: f32 = 32.0;
const SEP_H_DIP: f32 = 9.0;
const PAD_Y_DIP: f32 = 5.0;

pub(super) struct PanelState {
    pub hwnd: HWND,
    pub is_child: bool,
    pub w_dip: f32,
    pub h_dip: f32,
    pub entries: Vec<PopupEntry>,
    pub metrics: Vec<RowMetric>,
    pub hovered: Option<usize>,
    pub active_sub_idx: Option<usize>,
    pub mouse_tracked: bool,
    pub surface: Option<DibSurface>,
}

impl PanelState {
    pub(super) fn from_entries(entries: Vec<PopupEntry>, is_child: bool) -> Self {
        let (w_dip, h_dip, metrics) = measure_menu(&entries);
        Self {
            hwnd: HWND::default(),
            is_child,
            w_dip,
            h_dip,
            entries,
            metrics,
            hovered: None,
            active_sub_idx: None,
            mouse_tracked: false,
            surface: None,
        }
    }

    pub(super) fn update_entries(&mut self, entries: Vec<PopupEntry>, scale: f32) {
        let (w_dip, h_dip, metrics) = measure_menu(&entries);
        let size_changed = (w_dip - self.w_dip).abs() > 1.0 || (h_dip - self.h_dip).abs() > 1.0;
        self.entries = entries;
        self.metrics = metrics;
        self.w_dip = w_dip;
        self.h_dip = h_dip;
        if size_changed && !self.hwnd.is_invalid() {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    (w_dip * scale).round() as i32,
                    (h_dip * scale).round() as i32,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }
}

pub(super) fn create_panel_window(
    x: i32,
    y: i32,
    w_px: i32,
    h_px: i32,
    is_dark: bool,
    sess_ptr: isize,
) -> Option<HWND> {
    let hinst = unsafe { GetModuleHandleW(None).ok()? };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("TomoyoThemedPopupMenu"),
            w!(""),
            WS_POPUP | WS_CLIPCHILDREN,
            x,
            y,
            w_px,
            h_px,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .ok()?
    };
    apply_panel_dwm(hwnd, is_dark);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, sess_ptr);
    }
    Some(hwnd)
}

pub(super) fn apply_panel_dwm(hwnd: HWND, is_dark: bool) {
    unsafe {
        let dark: i32 = if is_dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as *const c_void,
            std::mem::size_of_val(&round) as u32,
        );
    }
}

pub(super) fn measure_menu(entries: &[PopupEntry]) -> (f32, f32, Vec<RowMetric>) {
    let mut y = PAD_Y_DIP;
    let mut max_w = 196.0f32;
    let mut metrics = Vec::with_capacity(entries.len());
    for ent in entries {
        match ent {
            PopupEntry::Separator => {
                metrics.push(RowMetric { y0: y, y1: y + SEP_H_DIP, is_sep: true, disabled: true });
                y += SEP_H_DIP;
            }
            PopupEntry::Submenu { label, .. } => {
                let lw: f32 = label.chars().map(|c| if c.is_ascii() { 7.5 } else { 13.0 }).sum();
                max_w = max_w.max(lw + 72.0);
                metrics.push(RowMetric { y0: y, y1: y + ROW_H_DIP, is_sep: false, disabled: false });
                y += ROW_H_DIP;
            }
            PopupEntry::Item { label, shortcut, disabled, .. } => {
                let lw: f32 = label.chars().map(|c| if c.is_ascii() { 7.5 } else { 13.0 }).sum();
                let sw: f32 = shortcut.as_deref().map(|s| s.chars().count() as f32 * 7.2 + 20.0).unwrap_or(0.0);
                max_w = max_w.max(lw + sw + 54.0);
                metrics.push(RowMetric { y0: y, y1: y + ROW_H_DIP, is_sep: false, disabled: *disabled });
                y += ROW_H_DIP;
            }
        }
    }
    (max_w.clamp(205.0, 360.0), y + PAD_Y_DIP, metrics)
}

pub(super) fn place_on_monitor(pt: POINT, w_px: i32, h_px: i32, align_bottom: bool) -> (i32, i32) {
    unsafe {
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(hmon, &mut mi);
        let work = mi.rcWork;
        let raw_y = if align_bottom { pt.y - h_px } else { pt.y };
        (
            pt.x.min(work.right - w_px - 6).max(work.left + 6),
            raw_y.min(work.bottom - h_px - 6).max(work.top + 6),
        )
    }
}

pub(super) fn place_child_on_monitor(
    parent: HWND,
    row_y0_px: i32,
    w_px: i32,
    h_px: i32,
    scale: f32,
) -> (i32, i32) {
    let mut prc = RECT::default();
    unsafe { let _ = GetWindowRect(parent, &mut prc); }
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let hmon = MonitorFromPoint(POINT { x: prc.left + 8, y: prc.top + 8 }, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(hmon, &mut mi);
    }
    let work = mi.rcWork;
    let overlap = (3.0 * scale).round() as i32;
    let pad_y = (PAD_Y_DIP * scale).round() as i32;
    let x = if prc.right - overlap + w_px <= work.right - 6 {
        prc.right - overlap
    } else {
        (prc.left - w_px + overlap).max(work.left + 6)
    };
    let raw_y = prc.top + row_y0_px - pad_y;
    let y = raw_y.min(work.bottom - h_px - 6).max(work.top + 6);
    (x, y)
}

pub(super) fn hit_test_item(metrics: &[RowMetric], y_dip: f32) -> Option<usize> {
    metrics.iter().enumerate().find(|(_, m)| !m.is_sep && !m.disabled && y_dip >= m.y0 && y_dip < m.y1).map(|(i, _)| i)
}

pub(super) fn step_selection(panel: &mut PanelState, down: bool) {
    let valid: Vec<usize> = panel.metrics.iter().enumerate().filter(|(_, m)| !m.is_sep && !m.disabled).map(|(i, _)| i).collect();
    if valid.is_empty() {
        return;
    }
    panel.hovered = match panel.hovered.and_then(|cur| valid.iter().position(|&v| v == cur)) {
        Some(pos) => Some(valid[if down { (pos + 1) % valid.len() } else if pos == 0 { valid.len() - 1 } else { pos - 1 }]),
        None => if down { valid.first().copied() } else { valid.last().copied() },
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_menu_handles_submenus_separators_and_disabled_items() {
        let entries = vec![
            PopupEntry::submenu("外观主题", vec![PopupEntry::item(1, "深色 (Dark)", None, true)]),
            PopupEntry::Separator,
            PopupEntry::item_disabled(2, "清空输入", Some("Esc"), true),
        ];
        let (w, h, metrics) = measure_menu(&entries);
        assert!((205.0..=360.0).contains(&w));
        assert!(h > 70.0);
        assert_eq!(metrics.len(), 3);
        assert!(!metrics[0].is_sep && !metrics[0].disabled);
        assert!(metrics[1].is_sep && metrics[1].disabled);
        assert!(metrics[2].disabled);
        assert_eq!(hit_test_item(&metrics, metrics[0].y0 + 5.0), Some(0));
        assert_eq!(hit_test_item(&metrics, metrics[1].y0 + 2.0), None);
        assert_eq!(hit_test_item(&metrics, metrics[2].y0 + 5.0), None);
    }
}
