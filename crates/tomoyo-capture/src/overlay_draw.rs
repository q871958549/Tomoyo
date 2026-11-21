//! Pure-Rust BGRA compositing + contextual AA SDF capsule chrome + ClearType GDI labels.

use crate::annotate::{AnnotColor, Annotation, ToolKind, COLOR_PRESETS};
use crate::annotate_render::{draw_selection_handles, render_annotations, render_single};
use crate::overlay_icons::{
    draw_capsule, draw_toolbar_icon, fill_box, fill_box_alpha, fill_circle_aa,
    fill_rounded_rect_aa, stroke_box, stroke_circle_aa, stroke_rounded_rect_aa,
};
use crate::screen::{DesktopSnapshot, ScreenRect};
use tomoyo_core::CaptureToolHotkeys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarCmd {
    Tool(ToolKind),
    Color(usize),
    Thickness(i32),
    ShapeFilled(bool),
    MosaicBlur(bool),
    Undo,
    ScrollLong,
    Pin,
    Ocr,
    OcrCopy,
    Translate,
    Save,
    Copy,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct ToolbarHit {
    pub cmd: ToolbarCmd,
    pub rect: ScreenRect,
    pub tooltip: String,
    pub pill_text: &'static str,
    pub active: bool,
    pub accent: bool,
    pub danger: bool,
    pub swatch: Option<AnnotColor>,
    pub thickness_dot: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolbarLayout {
    pub main_bar: Option<ScreenRect>,
    pub sub_bar: Option<ScreenRect>,
    pub dividers: Vec<(i32, i32, i32)>,
    pub hits: Vec<ToolbarHit>,
}

pub struct ToolbarStyleState {
    pub active_tool: ToolKind,
    pub active_color: usize,
    pub active_thickness: i32,
    pub shape_filled: bool,
    pub mosaic_blur: bool,
}

fn tip_with_key(label: &str, key: &str) -> String {
    if key.is_empty() { label.to_string() } else { format!("{label} ({key})") }
}

pub fn build_toolbar_layout(
    sel: ScreenRect,
    screen_w: i32,
    screen_h: i32,
    st: &ToolbarStyleState,
    hk: &CaptureToolHotkeys,
) -> ToolbarLayout {
    enum Slot {
        Btn(ToolbarCmd, String, i32, bool, bool, bool),
        Sep,
    }
    let slots = vec![
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Rect), tip_with_key("矩形", &hk.rect), 30, st.active_tool == ToolKind::Rect, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Ellipse), tip_with_key("椭圆", &hk.ellipse), 30, st.active_tool == ToolKind::Ellipse, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Arrow), tip_with_key("箭头 · 拖中点自由弯折", &hk.arrow), 30, st.active_tool == ToolKind::Arrow, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Pen), tip_with_key("画笔", &hk.pen), 30, st.active_tool == ToolKind::Pen, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Highlighter), tip_with_key("荧光笔", &hk.highlighter), 30, st.active_tool == ToolKind::Highlighter, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Mosaic), tip_with_key("马赛克/模糊", &hk.mosaic), 30, st.active_tool == ToolKind::Mosaic, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::StepNumber), tip_with_key("序号", &hk.step), 30, st.active_tool == ToolKind::StepNumber, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Text), tip_with_key("文字", &hk.text), 30, st.active_tool == ToolKind::Text, false, false),
        Slot::Sep,
        Slot::Btn(ToolbarCmd::Undo, "撤销 (Ctrl+Z)".into(), 30, false, false, false),
        Slot::Sep,
        Slot::Btn(ToolbarCmd::ScrollLong, tip_with_key("长截图 (纵/横)", &hk.scroll), 30, false, false, false),
        Slot::Btn(ToolbarCmd::Pin, tip_with_key("贴图", &hk.pin), 30, false, false, false),
        Slot::Btn(
            ToolbarCmd::Ocr,
            format!("识字 OCR ({}) · 直接复制 ({})", hk.ocr, hk.ocr_copy),
            30,
            false,
            false,
            false,
        ),
        Slot::Btn(ToolbarCmd::Translate, tip_with_key("截图翻译", &hk.translate), 30, false, false, false),
        Slot::Btn(ToolbarCmd::Save, "保存 PNG (Ctrl+S)".into(), 30, false, false, false),
        Slot::Sep,
        Slot::Btn(ToolbarCmd::Cancel, "取消 (Esc)".into(), 30, false, false, true),
        Slot::Btn(ToolbarCmd::Copy, "复制完成 (Enter)".into(), 38, false, true, false),
    ];

    let mut total_w = 12;
    for s in &slots {
        total_w += match s { Slot::Btn(_, _, w, _, _, _) => w + 2, Slot::Sep => 9 };
    }
    let bar_h = 40;
    let show_sub = st.active_tool != ToolKind::Select;
    let sub_h = 34;
    let stack_h = if show_sub { bar_h + 6 + sub_h } else { bar_h };
    let bar_x = (sel.right() - total_w).clamp(8, (screen_w - total_w - 8).max(8));
    let below_fits = sel.bottom() + 8 + stack_h <= screen_h - 6;
    let above_fits = sel.y - 8 - stack_h >= 6;
    let (bar_y, sub_above) = if below_fits {
        (sel.bottom() + 8, false)
    } else if above_fits {
        (sel.y - 8 - bar_h, true)
    } else {
        ((sel.bottom() - bar_h - 8).max(8), true)
    };

    let main_bar = ScreenRect { x: bar_x, y: bar_y, w: total_w, h: bar_h };
    let mut hits = Vec::with_capacity(32);
    let mut dividers = Vec::with_capacity(6);
    let mut cx = bar_x + 7;
    for s in slots {
        match s {
            Slot::Btn(cmd, tooltip, w, active, accent, danger) => {
                hits.push(ToolbarHit {
                    cmd, rect: ScreenRect { x: cx, y: bar_y + 5, w, h: 30 }, tooltip,
                    pill_text: "", active, accent, danger, swatch: None, thickness_dot: None,
                });
                cx += w + 2;
            }
            Slot::Sep => {
                dividers.push((cx + 3, bar_y + 11, bar_y + bar_h - 11));
                cx += 9;
            }
        }
    }

    let sub_bar = if show_sub {
        let style_pills: Vec<(ToolbarCmd, &'static str, bool)> = match st.active_tool {
            ToolKind::Rect | ToolKind::Ellipse => vec![
                (ToolbarCmd::ShapeFilled(!st.shape_filled), "填充", st.shape_filled),
            ],
            ToolKind::Mosaic => vec![
                (ToolbarCmd::MosaicBlur(false), "马赛克", !st.mosaic_blur),
                (ToolbarCmd::MosaicBlur(true), "模糊", st.mosaic_blur),
            ],
            _ => Vec::new(),
        };
        let style_w: i32 = if style_pills.is_empty() { 0 } else { style_pills.len() as i32 * 44 + 9 };
        let sub_w = 12 + 3 * 26 + style_w + 9 + COLOR_PRESETS.len() as i32 * 24 + 4;
        let sub_x = bar_x.clamp(8, (screen_w - sub_w - 8).max(8));
        let sub_y = if sub_above { (bar_y - 6 - sub_h).max(6) } else { (bar_y + bar_h + 6).min(screen_h - sub_h - 6) };
        let mut sx = sub_x + 8;

        for (t, tip) in [(2, "细 · 滚轮可无级调节"), (4, "中 · 滚轮可无级调节"), (8, "粗 · 滚轮可无级调节")] {
            hits.push(ToolbarHit {
                cmd: ToolbarCmd::Thickness(t),
                rect: ScreenRect { x: sx, y: sub_y + 4, w: 24, h: 26 },
                tooltip: tip.into(),
                pill_text: "",
                active: (st.active_thickness - t).abs() <= 1,
                accent: false, danger: false, swatch: None, thickness_dot: Some(t),
            });
            sx += 26;
        }
        if !style_pills.is_empty() {
            dividers.push((sx + 3, sub_y + 9, sub_y + sub_h - 9));
            sx += 9;
            for (cmd, lbl, act) in style_pills {
                hits.push(ToolbarHit {
                    cmd, rect: ScreenRect { x: sx, y: sub_y + 5, w: 42, h: 24 },
                    tooltip: String::new(), pill_text: lbl, active: act,
                    accent: false, danger: false, swatch: None, thickness_dot: None,
                });
                sx += 44;
            }
        }
        dividers.push((sx + 3, sub_y + 9, sub_y + sub_h - 9));
        sx += 9;
        for (i, &col) in COLOR_PRESETS.iter().enumerate() {
            hits.push(ToolbarHit {
                cmd: ToolbarCmd::Color(i),
                rect: ScreenRect { x: sx, y: sub_y + 5, w: 22, h: 24 },
                tooltip: String::new(), pill_text: "", active: st.active_color == i,
                accent: false, danger: false, swatch: Some(col), thickness_dot: None,
            });
            sx += 24;
        }
        Some(ScreenRect { x: sub_x, y: sub_y, w: sub_w, h: sub_h })
    } else {
        None
    };

    ToolbarLayout { main_bar: Some(main_bar), sub_bar, dividers, hits }
}

pub fn dim_badge_rect(sel: ScreenRect, screen_w: i32) -> ScreenRect {
    let (w, h) = (96, 24);
    let x = sel.x.clamp(8, (screen_w - w - 8).max(8));
    let y = if sel.y >= 32 { sel.y - h - 6 } else { sel.y + 8 };
    ScreenRect { x, y, w, h }
}

pub fn tooltip_rect_for(btn: &ToolbarHit, screen_w: i32, screen_h: i32) -> Option<ScreenRect> {
    if btn.tooltip.is_empty() { return None; }
    let text_w: i32 = btn.tooltip.chars().map(|c| if c.is_ascii() { 7 } else { 13 }).sum();
    let (w, h) = ((text_w + 16).clamp(56, 210), 24);
    let x = (btn.rect.x + btn.rect.w / 2 - w / 2).clamp(8, (screen_w - w - 8).max(8));
    let y = if btn.rect.y >= 36 { btn.rect.y - h - 7 } else { (btn.rect.bottom() + 7).min(screen_h - h - 6) };
    Some(ScreenRect { x, y, w, h })
}

#[allow(clippy::too_many_arguments)]
pub fn compose_overlay_pixels(
    dst: &mut [u8],
    snap: &DesktopSnapshot,
    dimmed: &[u8],
    sel: Option<ScreenRect>,
    hover_snap: Option<ScreenRect>,
    annotations: &[Annotation],
    selected_ann: Option<usize>,
    hovered_ann: Option<usize>,
    live_ann: Option<&Annotation>,
    cursor: (i32, i32),
    show_loupe: bool,
    layout: &ToolbarLayout,
    hovered_cmd: Option<ToolbarCmd>,
    wheel_hud: Option<&str>,
) {
    let (w, h) = (snap.width, snap.height);
    let stride = (w as usize) * 4;
    dst.copy_from_slice(dimmed);

    if let Some(r_raw) = sel.or(hover_snap) {
        let r = r_raw.clamp_to(w, h);
        if r.w > 0 && r.h > 0 {
            for y in r.y..r.bottom() {
                let off = (y as usize) * stride + (r.x as usize) * 4;
                let len = (r.w as usize) * 4;
                dst[off..off + len].copy_from_slice(&snap.bgra[off..off + len]);
            }
            if sel.is_some() {
                render_annotations(dst, w, h, 0, 0, annotations);
                if let Some(a) = live_ann { render_single(dst, w, h, 0, 0, a); }
                if let Some(idx) = hovered_ann.filter(|&i| Some(i) != selected_ann)
                    && let Some(a) = annotations.get(idx) { draw_selection_handles(dst, w, h, a, true); }
                if let Some(idx) = selected_ann
                    && let Some(a) = annotations.get(idx) { draw_selection_handles(dst, w, h, a, false); }
            }
            let border_col = if sel.is_some() { AnnotColor(0x3B, 0x82, 0xF6) } else { AnnotColor(0x60, 0xA5, 0xFA) };
            stroke_box(dst, w, h, r, 2, border_col);
            if sel.is_some() {
                for (hx, hy) in handle_points(r) {
                    fill_circle_aa(dst, w, h, hx as f32 + 0.5, hy as f32 + 0.5, 4.3, AnnotColor(0xFF, 0xFF, 0xFF), 1.0);
                    stroke_circle_aa(dst, w, h, hx as f32 + 0.5, hy as f32 + 0.5, 4.3, 1.5, border_col, 1.0);
                }
                let badge = dim_badge_rect(r, w);
                fill_rounded_rect_aa(dst, w, h, badge, 6.0, AnnotColor(0x14, 0x16, 0x1E), 0.90);
                stroke_rounded_rect_aa(dst, w, h, badge, 6.0, 1.0, AnnotColor(0x38, 0x3D, 0x4F), 0.9);
            }
        }
    }

    if show_loupe { draw_magnifier_pixels(dst, snap, cursor); }
    if let Some(br) = layout.main_bar { draw_capsule(dst, w, h, br, 10.0); }
    if let Some(sr) = layout.sub_bar { draw_capsule(dst, w, h, sr, 9.0); }
    for &(dx, y0, y1) in &layout.dividers {
        fill_box_alpha(dst, w, h, ScreenRect { x: dx, y: y0, w: 1, h: (y1 - y0).max(1) }, AnnotColor(0x3F, 0x45, 0x58), 210);
    }

    for btn in &layout.hits {
        let is_hov = hovered_cmd == Some(btn.cmd);
        let (cx, cy) = (btn.rect.x as f32 + btn.rect.w as f32 * 0.5, btn.rect.y as f32 + btn.rect.h as f32 * 0.5);
        if let Some(sw) = btn.swatch {
            if btn.active { stroke_circle_aa(dst, w, h, cx, cy, 8.2, 1.8, AnnotColor(0xFF, 0xFF, 0xFF), 0.98); }
            else if is_hov { stroke_circle_aa(dst, w, h, cx, cy, 8.2, 1.2, AnnotColor(0x94, 0xA3, 0xB8), 0.8); }
            fill_circle_aa(dst, w, h, cx, cy, 6.2, sw, 1.0);
            if sw == AnnotColor(0x18, 0x18, 0x1B) { stroke_circle_aa(dst, w, h, cx, cy, 6.2, 1.0, AnnotColor(0x64, 0x74, 0x8B), 0.9); }
        } else if let Some(t) = btn.thickness_dot {
            if btn.active { fill_rounded_rect_aa(dst, w, h, btn.rect, 6.0, AnnotColor(0x3B, 0x82, 0xF6), 0.24); }
            else if is_hov { fill_rounded_rect_aa(dst, w, h, btn.rect, 6.0, AnnotColor(0xFF, 0xFF, 0xFF), 0.09); }
            let rad = match t { 2 => 2.3, 4 => 3.6, _ => 5.0 };
            let col = if btn.active { AnnotColor(0x60, 0xA5, 0xFA) } else { AnnotColor(0xE2, 0xE8, 0xF0) };
            fill_circle_aa(dst, w, h, cx, cy, rad, col, 1.0);
        } else if !btn.pill_text.is_empty() {
            if btn.active {
                fill_rounded_rect_aa(dst, w, h, btn.rect, 6.0, AnnotColor(0x3B, 0x82, 0xF6), 0.26);
                stroke_rounded_rect_aa(dst, w, h, btn.rect, 6.0, 1.0, AnnotColor(0x60, 0xA5, 0xFA), 0.55);
            } else if is_hov {
                fill_rounded_rect_aa(dst, w, h, btn.rect, 6.0, AnnotColor(0xFF, 0xFF, 0xFF), 0.10);
            }
        } else {
            let fg = if btn.accent {
                let bg = if is_hov { AnnotColor(0x1D, 0x4E, 0xD8) } else { AnnotColor(0x25, 0x63, 0xEB) };
                fill_rounded_rect_aa(dst, w, h, btn.rect, 7.0, bg, 1.0);
                AnnotColor(0xFF, 0xFF, 0xFF)
            } else if btn.active {
                fill_rounded_rect_aa(dst, w, h, btn.rect, 7.0, AnnotColor(0x3B, 0x82, 0xF6), 0.24);
                stroke_rounded_rect_aa(dst, w, h, btn.rect, 7.0, 1.0, AnnotColor(0x60, 0xA5, 0xFA), 0.55);
                AnnotColor(0x60, 0xA5, 0xFA)
            } else if is_hov && btn.danger {
                fill_rounded_rect_aa(dst, w, h, btn.rect, 7.0, AnnotColor(0xEF, 0x44, 0x44), 0.22);
                AnnotColor(0xF8, 0x71, 0x71)
            } else if is_hov {
                fill_rounded_rect_aa(dst, w, h, btn.rect, 7.0, AnnotColor(0xFF, 0xFF, 0xFF), 0.11);
                AnnotColor(0xF8, 0xFA, 0xFC)
            } else {
                AnnotColor(0xCB, 0xD5, 0xE1)
            };
            draw_toolbar_icon(dst, w, h, cx, cy, btn.cmd, fg);
        }
    }

    if let Some(cmd) = hovered_cmd
        && let Some(btn) = layout.hits.iter().find(|b| b.cmd == cmd)
            && let Some(tr) = tooltip_rect_for(btn, w, h) {
                fill_rounded_rect_aa(dst, w, h, tr, 6.0, AnnotColor(0x0E, 0x10, 0x16), 0.95);
                stroke_rounded_rect_aa(dst, w, h, tr, 6.0, 1.0, AnnotColor(0x38, 0x3D, 0x4F), 0.9);
            }
    if wheel_hud.is_some() {
        let r = wheel_hud_rect(cursor, w, h);
        fill_rounded_rect_aa(dst, w, h, r, 6.0, AnnotColor(0x0E, 0x10, 0x16), 0.92);
        stroke_rounded_rect_aa(dst, w, h, r, 6.0, 1.0, AnnotColor(0x3B, 0x82, 0xF6), 0.85);
    }
}

pub fn wheel_hud_rect(cursor: (i32, i32), w: i32, h: i32) -> ScreenRect {
    ScreenRect {
        x: (cursor.0 + 18).clamp(8, (w - 104).max(8)),
        y: (cursor.1 - 30).clamp(8, (h - 32).max(8)),
        w: 96,
        h: 24,
    }
}

pub fn precompute_dimmed(bgra: &[u8]) -> Vec<u8> {
    let mut out = bgra.to_vec();
    for px in out.as_chunks_mut::<4>().0 {
        px[0] = ((px[0] as u16 * 112) >> 8) as u8;
        px[1] = ((px[1] as u16 * 112) >> 8) as u8;
        px[2] = ((px[2] as u16 * 112) >> 8) as u8;
        px[3] = 255;
    }
    out
}

pub fn handle_points(r: ScreenRect) -> [(i32, i32); 8] {
    let (mx, my) = (r.x + r.w / 2, r.y + r.h / 2);
    [
        (r.x, r.y), (mx, r.y), (r.right(), r.y),
        (r.x, my),             (r.right(), my),
        (r.x, r.bottom()), (mx, r.bottom()), (r.right(), r.bottom()),
    ]
}

pub fn loupe_rect(cursor: (i32, i32), screen_w: i32, screen_h: i32) -> ScreenRect {
    let (lw, lh) = (164, 192);
    let x = if cursor.0 + 22 + lw < screen_w { cursor.0 + 22 } else { (cursor.0 - 22 - lw).max(6) };
    let y = if cursor.1 + 22 + lh < screen_h { cursor.1 + 22 } else { (cursor.1 - 22 - lh).max(6) };
    ScreenRect { x, y, w: lw, h: lh }
}

fn draw_magnifier_pixels(dst: &mut [u8], snap: &DesktopSnapshot, cursor: (i32, i32)) {
    let (w, h) = (snap.width, snap.height);
    let lr = loupe_rect(cursor, w, h);
    draw_capsule(dst, w, h, lr, 10.0);
    let (grid, cell) = (11, 12);
    let (gx0, gy0) = (lr.x + (lr.w - grid * cell) / 2, lr.y + 10);
    for dy in 0..grid {
        for dx in 0..grid {
            let (r, g, b) = snap.pixel_rgb(cursor.0 + dx - grid / 2, cursor.1 + dy - grid / 2);
            fill_box(dst, w, h, ScreenRect { x: gx0 + dx * cell, y: gy0 + dy * cell, w: cell, h: cell }, AnnotColor(r, g, b));
        }
    }
    stroke_box(dst, w, h, ScreenRect { x: gx0, y: gy0, w: grid * cell, h: grid * cell }, 1, AnnotColor(0x38, 0x3D, 0x4F));
    let mid = grid / 2;
    fill_box_alpha(dst, w, h, ScreenRect { x: gx0 + mid * cell + cell / 2, y: gy0, w: 1, h: grid * cell }, AnnotColor(0xFF, 0xFF, 0xFF), 75);
    fill_box_alpha(dst, w, h, ScreenRect { x: gx0, y: gy0 + mid * cell + cell / 2, w: grid * cell, h: 1 }, AnnotColor(0xFF, 0xFF, 0xFF), 75);
    stroke_box(dst, w, h, ScreenRect { x: gx0 + mid * cell, y: gy0 + mid * cell, w: cell, h: cell }, 2, AnnotColor(0x3B, 0x82, 0xF6));
    let (r, g, b) = snap.pixel_rgb(cursor.0, cursor.1);
    fill_circle_aa(dst, w, h, (lr.x + 22) as f32, (lr.y + 157) as f32, 5.5, AnnotColor(r, g, b), 1.0);
    stroke_circle_aa(dst, w, h, (lr.x + 22) as f32, (lr.y + 157) as f32, 5.5, 1.1, AnnotColor(0x94, 0xA3, 0xB8), 0.9);
}

pub fn hit_test_handle(sel: ScreenRect, x: i32, y: i32) -> Option<usize> {
    handle_points(sel).into_iter().position(|(hx, hy)| (x - hx).abs() <= 6 && (y - hy).abs() <= 6)
}

pub fn resize_rect_by_handle(orig: ScreenRect, idx: usize, x: i32, y: i32) -> ScreenRect {
    let (mut l, mut t, mut r, mut b) = (orig.x, orig.y, orig.right(), orig.bottom());
    match idx {
        0 => { l = x; t = y; } 1 => t = y, 2 => { r = x; t = y; }
        3 => l = x, 4 => r = x, 5 => { l = x; b = y; } 6 => b = y, 7 => { r = x; b = y; }
        _ => {}
    }
    ScreenRect::from_points(l, t, r, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toolbar_and_minimalist_subbar_stay_inside_viewport() {
        let st = ToolbarStyleState {
            active_tool: ToolKind::Arrow, active_color: 0, active_thickness: 4,
            shape_filled: false, mosaic_blur: false,
        };
        let lay = build_toolbar_layout(ScreenRect { x: 50, y: 1050, w: 200, h: 20 }, 1920, 1080, &st, &CaptureToolHotkeys::default());
        let bar = lay.main_bar.expect("main bar");
        assert!(bar.x >= 0 && bar.right() <= 1920 && bar.y >= 0 && bar.bottom() <= 1080);
        let sub = lay.sub_bar.expect("sub bar");
        assert!(sub.x >= 0 && sub.right() <= 1920 && sub.y >= 0 && sub.bottom() <= 1080);
        assert!(lay.hits.iter().any(|h| h.cmd == ToolbarCmd::ScrollLong));
        // Arrow sub-bar stays ultra-minimalist: only 3 thickness dots + 6 color swatches (zero style-button bloat)
        assert!(lay.hits.iter().filter(|h| !h.pill_text.is_empty()).count() == 0);
    }
}
