//! Responsive DIP geometry, hit-testing, and multi-monitor cursor placement for Super Panel.

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

pub const PANEL_W_DIP: f32 = 364.0;
pub const PAD_X: f32 = 10.0;
pub const PAD_Y: f32 = 10.0;
const TILE_H: f32 = 38.0;
const TILE_GAP: f32 = 8.0;
const FOOTER_H: f32 = 22.0;

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
    pub trans_card: Option<DipRect>,
    pub copy_btn: Option<DipRect>,
    pub open_tr_btn: Option<DipRect>,
    pub tile_rects: Vec<DipRect>,
    pub footer_y0: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperHit {
    Tile(usize),
    CopyTranslation,
    OpenTranslator,
}

impl SuperPanelLayout {
    pub fn hit_test(&self, x_dip: f32, y_dip: f32) -> Option<SuperHit> {
        if let Some(rc) = self.copy_btn
            && rc.contains(x_dip, y_dip)
        {
            return Some(SuperHit::CopyTranslation);
        }
        if let Some(rc) = self.open_tr_btn
            && rc.contains(x_dip, y_dip)
        {
            return Some(SuperHit::OpenTranslator);
        }
        for (idx, rc) in self.tile_rects.iter().enumerate() {
            if rc.contains(x_dip, y_dip) {
                return Some(SuperHit::Tile(idx));
            }
        }
        None
    }
}

/// Computes responsive panel geometry in DIPs, including dynamic multi-line translation card height.
pub fn compute_layout(
    ctx: &SuperContext,
    trans: &InlineTransState,
    action_count: usize,
) -> SuperPanelLayout {
    let w_dip = PANEL_W_DIP;
    let has_preview = !matches!(ctx, SuperContext::Empty);
    let header_h = if has_preview { 46.0 } else { 32.0 };
    let mut y = PAD_Y + header_h;

    let (trans_card, copy_btn, open_tr_btn) = match trans {
        InlineTransState::Hidden => (None, None, None),
        InlineTransState::Loading => {
            let h = 54.0;
            let card = DipRect { x0: PAD_X, y0: y, x1: w_dip - PAD_X, y1: y + h };
            y += h + 9.0;
            (Some(card), None, None)
        }
        InlineTransState::Error { .. } => {
            let h = 68.0;
            let card = DipRect { x0: PAD_X, y0: y, x1: w_dip - PAD_X, y1: y + h };
            let open_rc = DipRect {
                x0: w_dip - PAD_X - 104.0,
                y0: y + h - 29.0,
                x1: w_dip - PAD_X - 8.0,
                y1: y + h - 7.0,
            };
            y += h + 9.0;
            (Some(card), None, Some(open_rc))
        }
        InlineTransState::Ready { translated, .. } => {
            let est_lines = estimate_wrap_lines(translated, 34).clamp(1, 5) as f32;
            let h = 24.0 + est_lines * 18.0 + 34.0;
            let card = DipRect { x0: PAD_X, y0: y, x1: w_dip - PAD_X, y1: y + h };
            let btn_y1 = y + h - 7.0;
            let btn_y0 = btn_y1 - 22.0;
            let copy_rc = DipRect { x0: PAD_X + 10.0, y0: btn_y0, x1: PAD_X + 94.0, y1: btn_y1 };
            let open_rc = DipRect { x0: PAD_X + 102.0, y0: btn_y0, x1: PAD_X + 200.0, y1: btn_y1 };
            y += h + 9.0;
            (Some(card), Some(copy_rc), Some(open_rc))
        }
    };

    let tile_w = (w_dip - PAD_X * 2.0 - TILE_GAP) * 0.5;
    let mut tile_rects = Vec::with_capacity(action_count);
    for i in 0..action_count {
        let col = (i % 2) as f32;
        let row = (i / 2) as f32;
        let x0 = PAD_X + col * (tile_w + TILE_GAP);
        let y0 = y + row * (TILE_H + TILE_GAP);
        tile_rects.push(DipRect { x0, y0, x1: x0 + tile_w, y1: y0 + TILE_H });
    }
    let rows = action_count.div_ceil(2) as f32;
    if rows > 0.0 {
        y += rows * TILE_H + (rows - 1.0).max(0.0) * TILE_GAP + 8.0;
    }
    let footer_y0 = y;
    let h_dip = footer_y0 + FOOTER_H + 4.0;

    SuperPanelLayout {
        w_dip,
        h_dip,
        trans_card,
        copy_btn,
        open_tr_btn,
        tile_rects,
        footer_y0,
    }
}

fn estimate_wrap_lines(text: &str, cjk_per_line: usize) -> usize {
    let mut total = 0usize;
    let cap = (cjk_per_line * 2).max(20);
    for raw_line in text.lines() {
        let units: usize = raw_line.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
        total += units.max(1).div_ceil(cap);
    }
    total.max(1)
}

/// Positions the Super Panel near `pt`, flipping gracefully if near right/bottom monitor edges.
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
        let mut x = pt.x + 10;
        let mut y = pt.y + 12;
        if x + w_px > work.right - 8 {
            x = (pt.x - w_px - 10).max(work.left + 8);
        }
        if y + h_px > work.bottom - 8 {
            y = (pt.y - h_px - 10).max(work.top + 8);
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
    fn layout_grows_for_inline_translation_and_hit_tests_buttons() {
        let ctx = SuperContext::Text { raw: "serendipity".into(), kind: TextKind::Plain };
        let hidden = compute_layout(&ctx, &InlineTransState::Hidden, 6);
        let ready = compute_layout(
            &ctx,
            &InlineTransState::Ready {
                translated: "意外发现珍奇事物的本领；机缘凑巧".into(),
                engine_name: "Google".into(),
                phonetic: Some("ˌserənˈdɪpəti".into()),
                target_lang: "zh-Hans".into(),
            },
            6,
        );
        assert!(ready.h_dip > hidden.h_dip + 40.0);
        let copy_rc = ready.copy_btn.expect("copy button rect");
        assert_eq!(
            ready.hit_test((copy_rc.x0 + copy_rc.x1) * 0.5, (copy_rc.y0 + copy_rc.y1) * 0.5),
            Some(SuperHit::CopyTranslation)
        );
        let t0 = ready.tile_rects[0];
        assert_eq!(
            ready.hit_test((t0.x0 + t0.x1) * 0.5, (t0.y0 + t0.y1) * 0.5),
            Some(SuperHit::Tile(0))
        );
    }
}
