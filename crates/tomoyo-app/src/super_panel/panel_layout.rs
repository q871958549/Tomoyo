//! Minimalist single-column DIP geometry, hit-testing, DWM styling & monitor placement.

use super::{InlineTransState, SuperContext};
use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};

pub const COMPACT_W_DIP: f32 = 204.0;
pub const TRANS_W_DIP: f32 = 272.0;
pub const PAD_X: f32 = 6.0;
pub const PAD_Y: f32 = 6.0;
const ROW_H: f32 = 31.0;
const CAPTION_H: f32 = 22.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct DipRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl DipRect {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
}

#[derive(Debug, Clone, Default)]
pub struct SuperPanelLayout {
    pub w_dip: f32,
    pub h_dip: f32,
    pub caption_y: Option<f32>,
    pub trans_area: Option<DipRect>,
    pub copy_btn: Option<DipRect>,
    pub open_tr_btn: Option<DipRect>,
    pub divider_y: Option<f32>,
    pub row_rects: Vec<DipRect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperHit {
    Row(usize),
    CopyTranslation,
    OpenTranslator,
}

impl SuperPanelLayout {
    pub fn hit_test(&self, x_dip: f32, y_dip: f32) -> Option<SuperHit> {
        if let Some(rc) = self.open_tr_btn
            && rc.contains(x_dip, y_dip)
        {
            return Some(SuperHit::OpenTranslator);
        }
        if let Some(rc) = self.copy_btn
            && rc.contains(x_dip, y_dip)
        {
            return Some(SuperHit::CopyTranslation);
        }
        if let Some(rc) = self.trans_area
            && self.copy_btn.is_some()
            && rc.contains(x_dip, y_dip)
        {
            return Some(SuperHit::CopyTranslation);
        }
        for (idx, rc) in self.row_rects.iter().enumerate() {
            if rc.contains(x_dip, y_dip) {
                return Some(SuperHit::Row(idx));
            }
        }
        None
    }
}

/// Computes minimalist single-column popover geometry (272 DIP with translation, 204 DIP compact).
pub fn compute_layout(
    ctx: &SuperContext,
    trans: &InlineTransState,
    action_count: usize,
) -> SuperPanelLayout {
    let has_trans = !matches!(trans, InlineTransState::Hidden);
    let w_dip = if has_trans { TRANS_W_DIP } else { COMPACT_W_DIP };
    let mut y = PAD_Y;
    let mut caption_y = None;
    let mut divider_y = None;

    let (trans_area, copy_btn, open_tr_btn) = match trans {
        InlineTransState::Hidden => {
            if !matches!(ctx, SuperContext::Empty) {
                caption_y = Some(y);
                y += CAPTION_H;
                divider_y = Some(y + 2.0);
                y += 7.0;
            }
            (None, None, None)
        }
        InlineTransState::Loading => {
            let h = 38.0;
            let area = DipRect { x0: PAD_X, y0: y, x1: w_dip - PAD_X, y1: y + h };
            y += h;
            divider_y = Some(y + 3.0);
            y += 8.0;
            (Some(area), None, None)
        }
        InlineTransState::Error { .. } => {
            let h = 42.0;
            let area = DipRect { x0: PAD_X, y0: y, x1: w_dip - PAD_X, y1: y + h };
            let open_rc = DipRect { x0: w_dip - PAD_X - 58.0, y0: y + 2.0, x1: w_dip - PAD_X - 4.0, y1: y + 20.0 };
            y += h;
            divider_y = Some(y + 3.0);
            y += 8.0;
            (Some(area), None, Some(open_rc))
        }
        InlineTransState::Ready { translated, .. } => {
            let est_lines = estimate_wrap_lines(translated, 22).clamp(1, 4) as f32;
            let h = 22.0 + est_lines * 18.0 + 6.0;
            let area = DipRect { x0: PAD_X, y0: y, x1: w_dip - PAD_X, y1: y + h };
            let open_rc = DipRect { x0: w_dip - PAD_X - 50.0, y0: y + 1.0, x1: w_dip - PAD_X - 4.0, y1: y + 19.0 };
            let copy_rc = DipRect { x0: open_rc.x0 - 40.0, y0: y + 1.0, x1: open_rc.x0 - 4.0, y1: y + 19.0 };
            y += h;
            divider_y = Some(y + 3.0);
            y += 8.0;
            (Some(area), Some(copy_rc), Some(open_rc))
        }
    };

    let mut row_rects = Vec::with_capacity(action_count);
    for i in 0..action_count {
        let ry0 = y + (i as f32) * ROW_H;
        row_rects.push(DipRect { x0: PAD_X, y0: ry0, x1: w_dip - PAD_X, y1: ry0 + ROW_H });
    }
    let h_dip = y + (action_count as f32) * ROW_H + PAD_Y;

    SuperPanelLayout {
        w_dip,
        h_dip,
        caption_y,
        trans_area,
        copy_btn,
        open_tr_btn,
        divider_y,
        row_rects,
    }
}

fn estimate_wrap_lines(text: &str, cjk_per_line: usize) -> usize {
    let mut total = 0usize;
    let cap = (cjk_per_line * 2).max(18);
    for raw_line in text.lines() {
        let units: usize = raw_line.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
        total += units.max(1).div_ceil(cap);
    }
    total.max(1)
}

pub fn place_near_cursor(pt: POINT, w_px: i32, h_px: i32) -> (i32, i32) {
    unsafe {
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            mi.rcWork
        } else {
            RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
        };
        let mut x = pt.x + 8;
        let mut y = pt.y + 10;
        if x + w_px > work.right - 8 {
            x = (pt.x - w_px - 8).max(work.left + 8);
        }
        if y + h_px > work.bottom - 8 {
            y = (pt.y - h_px - 8).max(work.top + 8);
        }
        (
            x.clamp(work.left + 8, (work.right - w_px - 8).max(work.left + 8)),
            y.clamp(work.top + 8, (work.bottom - h_px - 8).max(work.top + 8)),
        )
    }
}

pub fn apply_panel_dwm(hwnd: HWND, is_dark: bool) {
    unsafe {
        let dark: i32 = if is_dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as *const c_void, std::mem::size_of::<i32>() as u32,
        );
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd, DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as *const c_void, std::mem::size_of_val(&round) as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::super_panel::TextKind;

    #[test]
    fn minimalist_layout_adapts_width_and_hit_tests_cleanly() {
        let empty = compute_layout(&SuperContext::Empty, &InlineTransState::Hidden, 6);
        assert_eq!(empty.w_dip, COMPACT_W_DIP);

        let ctx = SuperContext::Text { raw: "serendipity".into(), kind: TextKind::Plain };
        let ready = compute_layout(
            &ctx,
            &InlineTransState::Ready {
                translated: "意外发现珍奇事物的本领；机缘凑巧".into(),
                engine_name: "Google".into(),
                phonetic: Some("ˌserənˈdɪpəti".into()),
            },
            5,
        );
        assert_eq!(ready.w_dip, TRANS_W_DIP);
        let copy_rc = ready.copy_btn.expect("copy link");
        assert_eq!(
            ready.hit_test((copy_rc.x0 + copy_rc.x1) * 0.5, (copy_rc.y0 + copy_rc.y1) * 0.5),
            Some(SuperHit::CopyTranslation)
        );
        let r0 = ready.row_rects[0];
        assert_eq!(
            ready.hit_test((r0.x0 + r0.x1) * 0.5, (r0.y0 + r0.y1) * 0.5),
            Some(SuperHit::Row(0))
        );
    }
}
