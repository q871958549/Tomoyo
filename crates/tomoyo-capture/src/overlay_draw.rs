//! Pure-Rust BGRA compositing + contextual AA SDF capsule chrome + magnifier loupe.

use crate::annotate::{AnnotColor, Annotation};
use crate::annotate_render::{draw_selection_handles, render_annotations, render_single};
use crate::overlay_icons::{
    draw_capsule, draw_toolbar_icon, fill_box, fill_box_alpha, fill_circle_aa,
    fill_rounded_rect_aa, stroke_box, stroke_circle_aa, stroke_rounded_rect_aa,
};
use crate::screen::{DesktopSnapshot, ScreenRect};

pub use crate::overlay_toolbar::{
    build_toolbar_layout, dim_badge_rect, rec_hint_rect, snap_badge_rect, tooltip_rect_for,
    ToolbarCmd, ToolbarLayout, ToolbarStyleState,
};

#[cfg_attr(not(windows), allow(dead_code))]
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
    compose_overlay_pixels_ex(
        dst, snap, dimmed, sel, hover_snap, annotations, selected_ann, hovered_ann,
        live_ann, cursor, show_loupe, layout, hovered_cmd, wheel_hud, false,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn compose_overlay_pixels_ex(
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
    rec_first_hint: bool,
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
            let border_col = if sel.is_some() { AnnotColor(0x3B, 0x82, 0xF6) } else { AnnotColor(0x38, 0xBD, 0xF8) };
            let outer = ScreenRect::from_points(
                (r.x - 1).max(0),
                (r.y - 1).max(0),
                (r.right() + 1).min(w),
                (r.bottom() + 1).min(h),
            );
            stroke_box(dst, w, h, outer, 1, AnnotColor(0x0B, 0x0F, 0x19));
            stroke_box(dst, w, h, r, 2, border_col);
            if sel.is_some() {
                for (hx, hy) in handle_points(r) {
                    fill_circle_aa(dst, w, h, hx as f32 + 0.5, hy as f32 + 0.5, 4.3, AnnotColor(0xFF, 0xFF, 0xFF), 1.0);
                    stroke_circle_aa(dst, w, h, hx as f32 + 0.5, hy as f32 + 0.5, 4.3, 1.5, border_col, 1.0);
                }
                let badge = dim_badge_rect(r, w);
                fill_rounded_rect_aa(dst, w, h, badge, 6.0, AnnotColor(0x14, 0x16, 0x1E), 0.90);
                stroke_rounded_rect_aa(dst, w, h, badge, 6.0, 1.0, AnnotColor(0x38, 0x3D, 0x4F), 0.9);
            } else {
                draw_hover_snap_accents(dst, w, h, r);
            }
        }
    }

    if show_loupe {
        draw_crosshair_guides(dst, w, h, cursor, loupe_rect(cursor, w, h));
        draw_magnifier_pixels(dst, snap, cursor);
    }
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
            if btn.danger {
                let bg = if is_hov { AnnotColor(0xEF, 0x44, 0x44) } else { AnnotColor(0xDC, 0x26, 0x26) };
                fill_rounded_rect_aa(dst, w, h, btn.rect, 6.0, bg, 1.0);
            } else if btn.active {
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
    if rec_first_hint && let Some(sr) = layout.sub_bar {
        let hr = rec_hint_rect(sr, w, h);
        fill_rounded_rect_aa(dst, w, h, hr, 6.0, AnnotColor(0x14, 0x18, 0x24), 0.93);
        stroke_rounded_rect_aa(dst, w, h, hr, 6.0, 1.0, AnnotColor(0xF5, 0x9E, 0x0B), 0.85);
    }
}

fn draw_hover_snap_accents(dst: &mut [u8], w: i32, h: i32, r: ScreenRect) {
    let badge = snap_badge_rect(r, w);
    fill_rounded_rect_aa(dst, w, h, badge, 6.0, AnnotColor(0x10, 0x14, 0x1E), 0.94);
    stroke_rounded_rect_aa(dst, w, h, badge, 6.0, 1.0, AnnotColor(0x38, 0xBD, 0xF8), 0.9);
    let arm = (r.w.min(r.h) / 5).clamp(8, 16);
    let c = AnnotColor(0xF0, 0xF9, 0xFF);
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.y, w: arm, h: 3 }, c);
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.y, w: 3, h: arm }, c);
    fill_box(dst, w, h, ScreenRect { x: r.right() - arm, y: r.y, w: arm, h: 3 }, c);
    fill_box(dst, w, h, ScreenRect { x: r.right() - 3, y: r.y, w: 3, h: arm }, c);
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.bottom() - 3, w: arm, h: 3 }, c);
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.bottom() - arm, w: 3, h: arm }, c);
    fill_box(dst, w, h, ScreenRect { x: r.right() - arm, y: r.bottom() - 3, w: arm, h: 3 }, c);
    fill_box(dst, w, h, ScreenRect { x: r.right() - 3, y: r.bottom() - arm, w: 3, h: arm }, c);
}

fn draw_crosshair_guides(dst: &mut [u8], w: i32, h: i32, cursor: (i32, i32), avoid: ScreenRect) {
    let (cx, cy) = (cursor.0.clamp(0, w - 1), cursor.1.clamp(0, h - 1));
    let stride = (w as usize) * 4;
    let blend = |px: &mut [u8]| {
        px[0] = ((px[0] as u16 * 184 + 253 * 72) >> 8) as u8;
        px[1] = ((px[1] as u16 * 184 + 189 * 72) >> 8) as u8;
        px[2] = ((px[2] as u16 * 184 + 56 * 72) >> 8) as u8;
    };
    let row_off = (cy as usize) * stride;
    for x in 0..w {
        if !avoid.contains(x, cy) {
            let off = row_off + (x as usize) * 4;
            blend(&mut dst[off..off + 3]);
        }
    }
    let col_off = (cx as usize) * 4;
    for y in 0..h {
        if y != cy && !avoid.contains(cx, y) {
            let off = (y as usize) * stride + col_off;
            blend(&mut dst[off..off + 3]);
        }
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
        px[0] = ((px[0] as u16 * 154) >> 8) as u8;
        px[1] = ((px[1] as u16 * 154) >> 8) as u8;
        px[2] = ((px[2] as u16 * 154) >> 8) as u8;
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
