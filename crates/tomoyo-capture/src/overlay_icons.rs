//! Pure-Rust anti-aliased SDF primitives and 16×16 minimalist vector icons for `tomoyo-capture`.
//!
//! Zero GPU / Direct2D / GDI+ dependencies — renders directly into the 32-bit BGRA overlay DIB.

use crate::annotate::{AnnotColor, ToolKind};
use crate::overlay_draw::ToolbarCmd;
use crate::screen::ScreenRect;

#[inline]
pub fn blend_px(dst: &mut [u8], w: i32, h: i32, x: i32, y: i32, c: AnnotColor, alpha: f32) {
    if x < 0 || y < 0 || x >= w || y >= h || alpha <= 0.005 {
        return;
    }
    let off = ((y as usize) * (w as usize) + (x as usize)) * 4;
    if off + 3 >= dst.len() {
        return;
    }
    let a = alpha.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    dst[off] = (c.2 as f32 * a + dst[off] as f32 * inv).round() as u8;
    dst[off + 1] = (c.1 as f32 * a + dst[off + 1] as f32 * inv).round() as u8;
    dst[off + 2] = (c.0 as f32 * a + dst[off + 2] as f32 * inv).round() as u8;
    dst[off + 3] = 255;
}

#[inline]
fn rounded_rect_sdf(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - r).max(0.0);
    let qy = (py - cy).abs() - (hy - r).max(0.0);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    outside + inside - r
}

pub fn draw_soft_shadow(
    dst: &mut [u8],
    w: i32,
    h: i32,
    r: ScreenRect,
    radius: f32,
    spread: i32,
    max_alpha: f32,
) {
    let sy_off = 3;
    let cx = r.x as f32 + r.w as f32 * 0.5;
    let cy = (r.y + sy_off) as f32 + r.h as f32 * 0.5;
    let hx = r.w as f32 * 0.5;
    let hy = r.h as f32 * 0.5;
    let rad = radius.min(hx).min(hy);
    let x0 = (r.x - spread).max(0);
    let y0 = (r.y + sy_off - spread).max(0);
    let x1 = (r.right() + spread).min(w);
    let y1 = (r.bottom() + sy_off + spread).min(h);
    let sp = spread.max(1) as f32;
    let black = AnnotColor(0x00, 0x00, 0x00);
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        for x in x0..x1 {
            let d = rounded_rect_sdf(x as f32 + 0.5, py, cx, cy, hx, hy, rad);
            if d > -1.0 && d < sp {
                let t = (1.0 - (d.max(0.0) / sp)).clamp(0.0, 1.0);
                blend_px(dst, w, h, x, y, black, t * t * max_alpha);
            }
        }
    }
}

pub fn fill_rounded_rect_aa(
    dst: &mut [u8],
    w: i32,
    h: i32,
    r: ScreenRect,
    radius: f32,
    c: AnnotColor,
    alpha: f32,
) {
    if r.w <= 0 || r.h <= 0 || alpha <= 0.005 {
        return;
    }
    let cx = r.x as f32 + r.w as f32 * 0.5;
    let cy = r.y as f32 + r.h as f32 * 0.5;
    let hx = r.w as f32 * 0.5;
    let hy = r.h as f32 * 0.5;
    let rad = radius.min(hx).min(hy);
    let x0 = (r.x - 1).max(0);
    let y0 = (r.y - 1).max(0);
    let x1 = (r.right() + 1).min(w);
    let y1 = (r.bottom() + 1).min(h);
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        for x in x0..x1 {
            let d = rounded_rect_sdf(x as f32 + 0.5, py, cx, cy, hx, hy, rad);
            if d < 0.75 {
                let cov = (0.5 - d).clamp(0.0, 1.0);
                blend_px(dst, w, h, x, y, c, cov * alpha);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn stroke_rounded_rect_aa(
    dst: &mut [u8],
    w: i32,
    h: i32,
    r: ScreenRect,
    radius: f32,
    thickness: f32,
    c: AnnotColor,
    alpha: f32,
) {
    if r.w <= 1 || r.h <= 1 || alpha <= 0.005 {
        return;
    }
    let cx = r.x as f32 + r.w as f32 * 0.5;
    let cy = r.y as f32 + r.h as f32 * 0.5;
    let hx = r.w as f32 * 0.5 - thickness * 0.5;
    let hy = r.h as f32 * 0.5 - thickness * 0.5;
    let rad = (radius - thickness * 0.5).max(0.0).min(hx).min(hy);
    let half = thickness * 0.5;
    let x0 = (r.x - 1).max(0);
    let y0 = (r.y - 1).max(0);
    let x1 = (r.right() + 1).min(w);
    let y1 = (r.bottom() + 1).min(h);
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        for x in x0..x1 {
            let d = rounded_rect_sdf(x as f32 + 0.5, py, cx, cy, hx, hy, rad).abs();
            if d < half + 0.8 {
                let cov = (half + 0.5 - d).clamp(0.0, 1.0);
                blend_px(dst, w, h, x, y, c, cov * alpha);
            }
        }
    }
}

pub fn fill_circle_aa(
    dst: &mut [u8],
    w: i32,
    h: i32,
    cx: f32,
    cy: f32,
    radius: f32,
    c: AnnotColor,
    alpha: f32,
) {
    let pad = radius.ceil() as i32 + 2;
    let x0 = (cx.floor() as i32 - pad).max(0);
    let y0 = (cy.floor() as i32 - pad).max(0);
    let x1 = (cx.ceil() as i32 + pad).min(w);
    let y1 = (cy.ceil() as i32 + pad).min(h);
    for y in y0..y1 {
        let dy = (y as f32 + 0.5) - cy;
        for x in x0..x1 {
            let dx = (x as f32 + 0.5) - cx;
            let d = (dx * dx + dy * dy).sqrt() - radius;
            if d < 0.75 {
                let cov = (0.5 - d).clamp(0.0, 1.0);
                blend_px(dst, w, h, x, y, c, cov * alpha);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn stroke_circle_aa(
    dst: &mut [u8],
    w: i32,
    h: i32,
    cx: f32,
    cy: f32,
    radius: f32,
    thickness: f32,
    c: AnnotColor,
    alpha: f32,
) {
    let half = thickness * 0.5;
    let pad = (radius + half).ceil() as i32 + 2;
    let x0 = (cx.floor() as i32 - pad).max(0);
    let y0 = (cy.floor() as i32 - pad).max(0);
    let x1 = (cx.ceil() as i32 + pad).min(w);
    let y1 = (cy.ceil() as i32 + pad).min(h);
    for y in y0..y1 {
        let dy = (y as f32 + 0.5) - cy;
        for x in x0..x1 {
            let dx = (x as f32 + 0.5) - cx;
            let d = ((dx * dx + dy * dy).sqrt() - radius).abs();
            if d < half + 0.8 {
                let cov = (half + 0.5 - d).clamp(0.0, 1.0);
                blend_px(dst, w, h, x, y, c, cov * alpha);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_aa_line(
    dst: &mut [u8],
    w: i32,
    h: i32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
    c: AnnotColor,
    alpha: f32,
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len_sq = dx * dx + dy * dy;
    let half = thickness * 0.5;
    let pad = half.ceil() as i32 + 2;
    let sx = (x0.min(x1).floor() as i32 - pad).max(0);
    let sy = (y0.min(y1).floor() as i32 - pad).max(0);
    let ex = (x0.max(x1).ceil() as i32 + pad).min(w);
    let ey = (y0.max(y1).ceil() as i32 + pad).min(h);
    for y in sy..ey {
        let py = y as f32 + 0.5 - y0;
        for x in sx..ex {
            let px = x as f32 + 0.5 - x0;
            let t = if len_sq > 0.0001 {
                ((px * dx + py * dy) / len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let dist = ((px - t * dx).powi(2) + (py - t * dy).powi(2)).sqrt();
            if dist < half + 0.8 {
                let cov = (half + 0.5 - dist).clamp(0.0, 1.0);
                blend_px(dst, w, h, x, y, c, cov * alpha);
            }
        }
    }
}

pub fn draw_toolbar_icon(
    dst: &mut [u8],
    w: i32,
    h: i32,
    cx: f32,
    cy: f32,
    cmd: ToolbarCmd,
    fg: AnnotColor,
) {
    let ln = |dst: &mut [u8], x0: f32, y0: f32, x1: f32, y1: f32, t: f32| {
        draw_aa_line(dst, w, h, cx + x0, cy + y0, cx + x1, cy + y1, t, fg, 1.0);
    };
    match cmd {
        ToolbarCmd::Tool(ToolKind::Rect) => {
            let r = ScreenRect { x: (cx - 6.0).round() as i32, y: (cy - 5.0).round() as i32, w: 12, h: 10 };
            stroke_rounded_rect_aa(dst, w, h, r, 2.0, 1.5, fg, 1.0);
        }
        ToolbarCmd::Tool(ToolKind::Ellipse) => {
            stroke_circle_aa(dst, w, h, cx, cy, 5.5, 1.5, fg, 1.0);
        }
        ToolbarCmd::Tool(ToolKind::Arrow) => {
            ln(dst, -5.0, 5.0, 5.0, -5.0, 1.6);
            ln(dst, 0.5, -5.0, 5.0, -5.0, 1.6);
            ln(dst, 5.0, -5.0, 5.0, -0.5, 1.6);
        }
        ToolbarCmd::Tool(ToolKind::Pen) => {
            ln(dst, -5.5, 5.5, -2.5, 4.5, 1.4);
            ln(dst, -2.5, 4.5, 4.8, -2.8, 1.6);
            ln(dst, 4.8, -2.8, 2.8, -4.8, 1.6);
            ln(dst, 2.8, -4.8, -4.5, 2.5, 1.6);
            ln(dst, -4.5, 2.5, -5.5, 5.5, 1.4);
        }
        ToolbarCmd::Tool(ToolKind::Mosaic) => {
            for row in 0..3 {
                for col in 0..3 {
                    if (row + col) % 2 == 0 {
                        let r = ScreenRect {
                            x: (cx - 5.5).round() as i32 + col * 4,
                            y: (cy - 5.5).round() as i32 + row * 4,
                            w: 3,
                            h: 3,
                        };
                        fill_rounded_rect_aa(dst, w, h, r, 0.6, fg, 0.95);
                    }
                }
            }
        }
        ToolbarCmd::Tool(ToolKind::StepNumber) => {
            stroke_circle_aa(dst, w, h, cx, cy, 6.0, 1.4, fg, 1.0);
            ln(dst, -1.2, -2.2, 0.3, -3.2, 1.4);
            ln(dst, 0.3, -3.2, 0.3, 3.0, 1.4);
            ln(dst, -1.5, 3.0, 2.0, 3.0, 1.3);
        }
        ToolbarCmd::Undo => {
            ln(dst, -5.0, -1.5, -2.0, -4.5, 1.5);
            ln(dst, -5.0, -1.5, -2.0, 1.5, 1.5);
            ln(dst, -4.5, -1.5, 2.5, -1.5, 1.5);
            ln(dst, 2.5, -1.5, 5.0, 0.5, 1.5);
            ln(dst, 5.0, 0.5, 5.0, 2.5, 1.5);
            ln(dst, 5.0, 2.5, 2.5, 4.5, 1.5);
            ln(dst, 2.5, 4.5, -1.5, 4.5, 1.5);
        }
        ToolbarCmd::Pin => {
            ln(dst, -1.5, -5.5, 5.5, 1.5, 1.5);
            ln(dst, 1.0, -3.0, -2.0, 0.0, 1.5);
            ln(dst, 3.0, -1.0, 0.0, 2.0, 1.5);
            ln(dst, -3.5, -1.5, 1.5, 3.5, 1.5);
            ln(dst, -1.0, 1.0, -5.5, 5.5, 1.6);
        }
        ToolbarCmd::Ocr => {
            // 4 viewfinder corners + 2 text scan lines
            ln(dst, -6.0, -3.0, -6.0, -6.0, 1.5);
            ln(dst, -6.0, -6.0, -3.0, -6.0, 1.5);
            ln(dst, 3.0, -6.0, 6.0, -6.0, 1.5);
            ln(dst, 6.0, -6.0, 6.0, -3.0, 1.5);
            ln(dst, -6.0, 3.0, -6.0, 6.0, 1.5);
            ln(dst, -6.0, 6.0, -3.0, 6.0, 1.5);
            ln(dst, 3.0, 6.0, 6.0, 6.0, 1.5);
            ln(dst, 6.0, 6.0, 6.0, 3.0, 1.5);
            ln(dst, -3.0, -1.5, 3.0, -1.5, 1.4);
            ln(dst, -3.0, 1.8, 1.5, 1.8, 1.4);
        }
        ToolbarCmd::Translate => {
            // Minimalist "A" (left) + clean glyph bars (right)
            ln(dst, -6.0, 4.0, -3.2, -4.2, 1.45);
            ln(dst, -3.2, -4.2, -0.4, 4.0, 1.45);
            ln(dst, -5.0, 1.4, -1.4, 1.4, 1.3);
            ln(dst, 1.2, -3.5, 6.2, -3.5, 1.4);
            ln(dst, 3.7, -5.2, 3.7, -3.5, 1.4);
            ln(dst, 5.4, -3.3, 1.6, 4.2, 1.4);
            ln(dst, 2.0, -1.8, 6.0, 4.2, 1.4);
        }
        ToolbarCmd::Save => {
            ln(dst, 0.0, -5.5, 0.0, 2.0, 1.6);
            ln(dst, -3.2, -0.8, 0.0, 2.4, 1.6);
            ln(dst, 3.2, -0.8, 0.0, 2.4, 1.6);
            ln(dst, -5.5, 2.5, -5.5, 5.5, 1.5);
            ln(dst, -5.5, 5.5, 5.5, 5.5, 1.5);
            ln(dst, 5.5, 5.5, 5.5, 2.5, 1.5);
        }
        ToolbarCmd::Cancel => {
            ln(dst, -4.5, -4.5, 4.5, 4.5, 1.65);
            ln(dst, 4.5, -4.5, -4.5, 4.5, 1.65);
        }
        ToolbarCmd::Copy => {
            ln(dst, -5.0, 0.2, -1.4, 4.0, 2.0);
            ln(dst, -1.4, 4.0, 5.4, -3.8, 2.0);
        }
        ToolbarCmd::Tool(ToolKind::Select) | ToolbarCmd::Color(_) | ToolbarCmd::Thickness(_) => {}
    }
}

pub fn draw_capsule(dst: &mut [u8], w: i32, h: i32, r: ScreenRect, radius: f32) {
    draw_soft_shadow(dst, w, h, r, radius, 10, 0.42);
    fill_rounded_rect_aa(dst, w, h, r, radius, AnnotColor(0x16, 0x18, 0x20), 0.94);
    stroke_rounded_rect_aa(dst, w, h, r, radius, 1.0, AnnotColor(0x38, 0x3D, 0x4F), 0.92);
}

pub fn fill_box(dst: &mut [u8], w: i32, h: i32, r: ScreenRect, c: AnnotColor) {
    let cl = r.clamp_to(w, h);
    for y in cl.y..cl.bottom() {
        for x in cl.x..cl.right() {
            let off = ((y as usize) * (w as usize) + (x as usize)) * 4;
            dst[off] = c.2;
            dst[off + 1] = c.1;
            dst[off + 2] = c.0;
            dst[off + 3] = 255;
        }
    }
}

pub fn fill_box_alpha(dst: &mut [u8], w: i32, h: i32, r: ScreenRect, c: AnnotColor, alpha: u8) {
    let cl = r.clamp_to(w, h);
    let a = alpha as u16;
    let inv = 255 - a;
    for y in cl.y..cl.bottom() {
        for x in cl.x..cl.right() {
            let off = ((y as usize) * (w as usize) + (x as usize)) * 4;
            dst[off] = ((c.2 as u16 * a + dst[off] as u16 * inv) >> 8) as u8;
            dst[off + 1] = ((c.1 as u16 * a + dst[off + 1] as u16 * inv) >> 8) as u8;
            dst[off + 2] = ((c.0 as u16 * a + dst[off + 2] as u16 * inv) >> 8) as u8;
            dst[off + 3] = 255;
        }
    }
}

pub fn stroke_box(dst: &mut [u8], w: i32, h: i32, r: ScreenRect, t: i32, c: AnnotColor) {
    if r.w < 2 || r.h < 2 {
        return;
    }
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.y, w: r.w, h: t }, c);
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.bottom() - t, w: r.w, h: t }, c);
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.y, w: t, h: r.h }, c);
    fill_box(dst, w, h, ScreenRect { x: r.right() - t, y: r.y, w: t, h: r.h }, c);
}
