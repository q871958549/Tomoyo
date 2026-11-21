//! Pure-Rust anti-aliased 32-bit BGRA rasterizer with analytic-SDF modern tapered free-bend
//! arrows, true-Euclidean uniform-stroke ellipses, rounded rects, smooth pen/highlighter,
//! mosaic/blur, step badges & hover/selection handles.

use crate::annotate::{bezier_ctrl_from_midpoint, eval_quad_bezier, AnnotColor, Annotation};
use crate::overlay_icons::{
    blend_px, draw_aa_line, fill_circle_aa, fill_rounded_rect_aa, stroke_circle_aa,
    stroke_rounded_rect_aa,
};
use crate::screen::ScreenRect;

pub fn render_annotations(
    bgra: &mut [u8],
    width: i32,
    height: i32,
    offset_x: i32,
    offset_y: i32,
    annotations: &[Annotation],
) {
    for ann in annotations {
        render_single(bgra, width, height, offset_x, offset_y, ann);
    }
}

pub fn render_single(bgra: &mut [u8], w: i32, h: i32, ox: i32, oy: i32, ann: &Annotation) {
    match ann {
        Annotation::Rect { rect, color, thickness, filled } => {
            let r = ScreenRect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            let t = (*thickness).clamp(1, 20) as f32;
            if *filled {
                fill_rounded_rect_aa(bgra, w, h, r, 4.0, *color, 0.28);
            }
            stroke_rounded_rect_aa(bgra, w, h, r, 3.0, t, *color, 1.0);
        }
        Annotation::Ellipse { rect, color, thickness, filled } => {
            let r = ScreenRect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_ellipse(bgra, w, h, r, *color, (*thickness).clamp(1, 20), *filled);
        }
        Annotation::Arrow { x0, y0, cx, cy, x1, y1, color, thickness } => {
            draw_modern_tapered_arrow(
                bgra, w, h,
                (x0 - ox) as f32, (y0 - oy) as f32,
                (cx - ox) as f32, (cy - oy) as f32,
                (x1 - ox) as f32, (y1 - oy) as f32,
                *color, (*thickness).clamp(1, 20),
            );
        }
        Annotation::Pen { points, color, thickness, highlighter } => {
            let t = (*thickness).clamp(2, 36) as f32;
            let alpha = if *highlighter { 0.38 } else { 1.0 };
            if points.len() == 1 {
                fill_circle_aa(bgra, w, h, (points[0].0 - ox) as f32, (points[0].1 - oy) as f32, t * 0.5, *color, alpha);
            } else {
                draw_smooth_pen(bgra, w, h, ox, oy, points, t, *color, alpha);
            }
        }
        Annotation::Mosaic { rect, block, blur } => {
            let r = ScreenRect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            if *blur {
                apply_box_blur(bgra, w, h, r, ((*block) / 2).clamp(3, 18));
            } else {
                apply_mosaic(bgra, w, h, r, (*block).clamp(6, 36));
            }
        }
        Annotation::StepNumber { cx, cy, number, radius, color } => {
            draw_step_badge(bgra, w, h, cx - ox, cy - oy, *number, (*radius).clamp(10, 26), *color);
        }
        Annotation::Text { .. } => {}
    }
}

pub fn draw_selection_handles(bgra: &mut [u8], w: i32, h: i32, ann: &Annotation, is_hover_only: bool) {
    let alpha = if is_hover_only { 0.72 } else { 1.0 };
    let pts = ann.control_points();
    for (idx, (hx, hy)) in pts.iter().enumerate() {
        let is_bend = matches!(ann, Annotation::Arrow { .. }) && idx == 1;
        let rad = if is_hover_only { 4.2 } else { 5.0 };
        let fill = if is_bend { AnnotColor(0xF5, 0x9E, 0x0B) } else { AnnotColor(0xFF, 0xFF, 0xFF) };
        let ring = if is_bend { AnnotColor(0xFF, 0xFF, 0xFF) } else { AnnotColor(0x25, 0x63, 0xEB) };
        fill_circle_aa(bgra, w, h, *hx as f32 + 0.5, *hy as f32 + 0.5, rad, fill, alpha);
        stroke_circle_aa(bgra, w, h, *hx as f32 + 0.5, *hy as f32 + 0.5, rad, 1.6, ring, alpha);
    }
}

/// Closed-form analytic quadratic Bezier closest-point solver (Inigo Quilez Cardano cubic).
/// Returns `(euclidean_dist, closest_t)` for `t` clamped to `[0.0, t_end]`, yielding infinite
/// curve smoothness with zero polygonal facets even on sharp 180-degree hairpin bends!
#[inline]
fn closest_quad_bezier(
    p: (f32, f32),
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    t_end: f32,
) -> (f32, f32) {
    let eval = |raw_t: f32| -> (f32, f32) {
        let t = raw_t.clamp(0.0, t_end);
        let q = eval_quad_bezier(a, b, c, t);
        let (dx, dy) = (p.0 - q.0, p.1 - q.1);
        (dx * dx + dy * dy, t)
    };
    let (ax, ay) = (b.0 - a.0, b.1 - a.1);
    let (bx, by) = (a.0 - 2.0 * b.0 + c.0, a.1 - 2.0 * b.1 + c.1);
    let bb = bx * bx + by * by;
    if bb < 1e-4 {
        let (vx, vy) = (c.0 - a.0, c.1 - a.1);
        let l2 = (vx * vx + vy * vy).max(1e-6);
        let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / l2).clamp(0.0, t_end);
        let (d2, ct) = eval(t);
        return (d2.sqrt(), ct);
    }
    let (dx, dy) = (a.0 - p.0, a.1 - p.1);
    let kk = 1.0 / bb;
    let kx = kk * (ax * bx + ay * by);
    let ky = kk * (2.0 * (ax * ax + ay * ay) + (dx * bx + dy * by)) / 3.0;
    let kz = kk * (dx * ax + dy * ay);
    let p_c = ky - kx * kx;
    let p3 = p_c * p_c * p_c;
    let q_c = kx * (2.0 * kx * kx - 3.0 * ky) + kz;
    let h = q_c * q_c + 4.0 * p3;

    let (best_d2, best_t) = if h >= 0.0 {
        let sh = h.sqrt();
        let x1 = (sh - q_c) * 0.5;
        let x2 = (-sh - q_c) * 0.5;
        let uv = x1.cbrt() + x2.cbrt();
        eval(uv - kx)
    } else {
        let z = (-p_c).max(1e-12).sqrt();
        let v = (q_c / (p_c * z * 2.0)).clamp(-1.0, 1.0).acos() / 3.0;
        let m = v.cos();
        let n = v.sin() * 1.732050808;
        let r1 = eval((m + m) * z - kx);
        let r2 = eval((-n - m) * z - kx);
        let r3 = eval((n - m) * z - kx);
        let r12 = if r1.0 <= r2.0 { r1 } else { r2 };
        if r12.0 <= r3.0 { r12 } else { r3 }
    };
    (best_d2.sqrt(), best_t)
}

/// Exact analytic signed distance to a 4-vertex simple polygon (works cleanly for concave
/// swept-back arrowheads with zero internal diagonals or CSG edge artifacts).
#[inline]
fn sd_quad_poly(p: (f32, f32), v: [(f32, f32); 4]) -> f32 {
    let mut min_d2 = f32::MAX;
    let mut inside = false;
    let mut j = 3usize;
    for i in 0..4 {
        let (ax, ay) = v[j];
        let (bx, by) = v[i];
        let (ex, ey) = (bx - ax, by - ay);
        let (wx, wy) = (p.0 - ax, p.1 - ay);
        let t = ((wx * ex + wy * ey) / (ex * ex + ey * ey).max(1e-6)).clamp(0.0, 1.0);
        let d2 = (wx - ex * t).powi(2) + (wy - ey * t).powi(2);
        if d2 < min_d2 { min_d2 = d2; }
        if (ay > p.1) != (by > p.1) && p.0 < (bx - ax) * (p.1 - ay) / (by - ay) + ax {
            inside = !inside;
        }
        j = i;
    }
    let dist = min_d2.sqrt();
    if inside { -dist } else { dist }
}

/// Modern tapered arrow (Snipaste / PixPin / CleanShot style):
/// sleek pointed tail -> smoothly widens along Bezier curve -> sharp concave swept-back head.
/// Rendered via single-pass analytic SDF (zero seams, zero double-blending, safe at all lengths).
#[allow(clippy::too_many_arguments)]
fn draw_modern_tapered_arrow(
    bgra: &mut [u8],
    w: i32,
    h: i32,
    x0: f32,
    y0: f32,
    mx: f32,
    my: f32,
    x1: f32,
    y1: f32,
    c: AnnotColor,
    t: i32,
) {
    let p0 = (x0, y0);
    let p1 = (x1, y1);
    let chord = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
    if chord < 3.0 { return; }

    let pc = bezier_ctrl_from_midpoint(p0, (mx, my), p1);
    const SAMPLES: usize = 64;
    let mut curve = [(0.0f32, 0.0f32); SAMPLES + 1];
    let mut total_len = 0.0f32;
    curve[0] = p0;
    for i in 1..=SAMPLES {
        let pt = eval_quad_bezier(p0, pc, p1, i as f32 / SAMPLES as f32);
        total_len += ((pt.0 - curve[i - 1].0).powi(2) + (pt.1 - curve[i - 1].1).powi(2)).sqrt();
        curve[i] = pt;
    }
    total_len = total_len.max(chord);

    // Safe monotonic sizing at ALL lengths from 3px to 4000px (never panics on clamp!)
    let head_len = (t as f32 * 3.8 + total_len * 0.052).min(46.0).max(14.0).min(total_len * 0.46);
    let head_half_w = head_len * 0.52;
    let notch_dist = head_len * 0.72;
    let tail_r = (t as f32 * 0.22).min(2.4).max(0.75).min(head_half_w * 0.20).max(0.5);
    let neck_r = (t as f32 * 0.65 + head_len * 0.11).min(head_half_w * 0.44).max(tail_r + 0.35);

    // Find exact `t_end` where Euclidean distance from `B(t_end)` to `tip` reaches `notch_dist`
    let mut t_end = 0.0f32;
    let mut prev_d = 0.0f32;
    for i in (0..SAMPLES).rev() {
        let d = ((p1.0 - curve[i].0).powi(2) + (p1.1 - curve[i].1).powi(2)).sqrt();
        if d >= notch_dist {
            let frac = ((notch_dist - prev_d) / (d - prev_d).max(1e-5)).clamp(0.0, 1.0);
            t_end = ((i + 1) as f32 - frac) / SAMPLES as f32;
            break;
        }
        prev_d = d;
    }
    t_end = t_end.clamp(0.05, 0.98);
    let notch_pt = eval_quad_bezier(p0, pc, p1, t_end);

    // Align arrowhead tangent smoothly with `tip - notch_pt`
    let (mut tdx, mut tdy) = (p1.0 - notch_pt.0, p1.1 - notch_pt.1);
    if (tdx * tdx + tdy * tdy).sqrt() < 1.0 {
        tdx = p1.0 - p0.0;
        tdy = p1.1 - p0.1;
    }
    let norm = (tdx * tdx + tdy * tdy).sqrt().max(1e-4);
    let (ux, uy) = (tdx / norm, tdy / norm);

    let tip = p1;
    let base = (tip.0 - ux * head_len, tip.1 - uy * head_len);
    let left_wing = (base.0 - uy * head_half_w, base.1 + ux * head_half_w);
    let right_wing = (base.0 + uy * head_half_w, base.1 - ux * head_half_w);

    // 4 tight spatial bounding boxes along the curve solely for fast pixel culling
    let pad = neck_r + 2.0;
    let mut boxes = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 4];
    for bi in 0..4 {
        let (s0, s1) = (bi * 16, (bi + 1) * 16);
        let (mut lx, mut ly, mut rx, mut ry) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(px, py) in &curve[s0..=s1] {
            lx = lx.min(px - pad); ly = ly.min(py - pad);
            rx = rx.max(px + pad); ry = ry.max(py + pad);
        }
        boxes[bi] = (lx, ly, rx, ry);
    }
    let h_lx = tip.0.min(left_wing.0).min(right_wing.0).min(notch_pt.0) - 1.5;
    let h_ly = tip.1.min(left_wing.1).min(right_wing.1).min(notch_pt.1) - 1.5;
    let h_rx = tip.0.max(left_wing.0).max(right_wing.0).max(notch_pt.0) + 1.5;
    let h_ry = tip.1.max(left_wing.1).max(right_wing.1).max(notch_pt.1) + 1.5;

    let min_x = boxes.iter().fold(h_lx, |a, b| a.min(b.0)).floor().max(0.0) as i32;
    let min_y = boxes.iter().fold(h_ly, |a, b| a.min(b.1)).floor().max(0.0) as i32;
    let max_x = boxes.iter().fold(h_rx, |a, b| a.max(b.2)).ceil().min((w - 1) as f32) as i32;
    let max_y = boxes.iter().fold(h_ry, |a, b| a.max(b.3)).ceil().min((h - 1) as f32) as i32;

    for y in min_y..=max_y {
        let py = y as f32 + 0.5;
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let in_shaft = boxes.iter().any(|&(lx, ly, rx, ry)| px >= lx && px <= rx && py >= ly && py <= ry);
            let in_head = px >= h_lx && px <= h_rx && py >= h_ly && py <= h_ry;
            if !in_shaft && !in_head {
                continue;
            }
            let mut d = f32::MAX;
            if in_shaft {
                let (cd, ct) = closest_quad_bezier((px, py), p0, pc, p1, t_end);
                let u = (ct / t_end).clamp(0.0, 1.0);
                let rad = tail_r + (neck_r - tail_r) * u.powf(0.85);
                d = cd - rad;
            }
            if in_head {
                let d_head = sd_quad_poly((px, py), [tip, left_wing, notch_pt, right_wing]);
                d = d.min(d_head);
            }
            if d < 0.75 {
                blend_px(bgra, w, h, x, y, c, (0.5 - d).clamp(0.0, 1.0));
            }
        }
    }
}

/// True Euclidean gradient-normalized ellipse SDF: uniform stroke width `t` around the entire
/// perimeter at ANY aspect ratio (even 500×20), with generous unclipped bounding box.
fn draw_ellipse(bgra: &mut [u8], w: i32, h: i32, r: ScreenRect, c: AnnotColor, t: i32, filled: bool) {
    if r.w < 4 || r.h < 4 { return; }
    let (rx, ry) = (r.w as f32 * 0.5, r.h as f32 * 0.5);
    let (cx, cy) = (r.x as f32 + rx, r.y as f32 + ry);
    let half_t = t as f32 * 0.5;
    let pad = (t + 4).max(6);
    let (sx, sy) = ((r.x - pad).max(0), (r.y - pad).max(0));
    let (ex, ey) = ((r.right() + pad).min(w), (r.bottom() + pad).min(h));
    for y in sy..ey {
        let dy = (y as f32 + 0.5) - cy;
        let uy = dy / ry;
        let gy = uy / ry;
        for x in sx..ex {
            let dx = (x as f32 + 0.5) - cx;
            let ux = dx / rx;
            let k1 = (ux * ux + uy * uy).sqrt();
            let gx = ux / rx;
            let k2 = (gx * gx + gy * gy).sqrt();
            let signed_dist = if k2 > 1e-6 { k1 * (k1 - 1.0) / k2 } else { -rx.min(ry) };
            if filled && signed_dist < -half_t {
                blend_px(bgra, w, h, x, y, c, 0.28);
            }
            let dist = signed_dist.abs();
            if dist <= half_t + 0.85 {
                blend_px(bgra, w, h, x, y, c, (half_t + 0.5 - dist).clamp(0.0, 1.0));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_smooth_pen(
    bgra: &mut [u8], w: i32, h: i32, ox: i32, oy: i32,
    points: &[(i32, i32)], thickness: f32, color: AnnotColor, alpha: f32,
) {
    if alpha < 0.99 {
        let half = thickness * 0.5;
        let pad = half.ceil() as i32 + 2;
        let (mut lx, mut ly, mut rx, mut ry) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for &(px, py) in points { lx = lx.min(px - ox); ly = ly.min(py - oy); rx = rx.max(px - ox); ry = ry.max(py - oy); }
        let (sx, sy, ex, ey) = ((lx - pad).max(0), (ly - pad).max(0), (rx + pad).min(w), (ry + pad).min(h));
        for y in sy..ey {
            let fy = y as f32 + 0.5;
            for x in sx..ex {
                let fx = x as f32 + 0.5;
                let mut best = f32::MAX;
                for seg in points.windows(2) {
                    let (x0, y0) = ((seg[0].0 - ox) as f32, (seg[0].1 - oy) as f32);
                    let (x1, y1) = ((seg[1].0 - ox) as f32, (seg[1].1 - oy) as f32);
                    let (dx, dy) = (x1 - x0, y1 - y0);
                    let lsq = dx * dx + dy * dy;
                    let t = if lsq > 0.001 { (((fx - x0) * dx + (fy - y0) * dy) / lsq).clamp(0.0, 1.0) } else { 0.0 };
                    let d = ((fx - (x0 + t * dx)).powi(2) + (fy - (y0 + t * dy)).powi(2)).sqrt();
                    if d < best { best = d; }
                }
                if best < half + 0.8 { blend_px(bgra, w, h, x, y, color, (half + 0.5 - best).clamp(0.0, 1.0) * alpha); }
            }
        }
        return;
    }
    if points.len() == 2 {
        draw_aa_line(bgra, w, h, (points[0].0 - ox) as f32, (points[0].1 - oy) as f32, (points[1].0 - ox) as f32, (points[1].1 - oy) as f32, thickness, color, 1.0);
        return;
    }
    let mut prev = ((points[0].0 - ox) as f32, (points[0].1 - oy) as f32);
    for i in 1..points.len() - 1 {
        let cur = ((points[i].0 - ox) as f32, (points[i].1 - oy) as f32);
        let nxt = ((points[i + 1].0 - ox) as f32, (points[i + 1].1 - oy) as f32);
        let mid = ((cur.0 + nxt.0) * 0.5, (cur.1 + nxt.1) * 0.5);
        for s in 1..=4 {
            let p = eval_quad_bezier(prev, cur, mid, s as f32 * 0.25);
            draw_aa_line(bgra, w, h, prev.0, prev.1, p.0, p.1, thickness, color, 1.0);
            prev = p;
        }
    }
    let last = ((points[points.len() - 1].0 - ox) as f32, (points[points.len() - 1].1 - oy) as f32);
    draw_aa_line(bgra, w, h, prev.0, prev.1, last.0, last.1, thickness, color, 1.0);
}

fn apply_mosaic(bgra: &mut [u8], w: i32, h: i32, r: ScreenRect, block: i32) {
    let c = r.clamp_to(w, h);
    if c.w <= 0 || c.h <= 0 { return; }
    let mut y = c.y;
    while y < c.bottom() {
        let y_end = (y + block).min(c.bottom());
        let mut x = c.x;
        while x < c.right() {
            let x_end = (x + block).min(c.right());
            let (mut sb, mut sg, mut sr, mut n) = (0u64, 0u64, 0u64, 0u64);
            for py in y..y_end {
                for px in x..x_end {
                    let off = ((py as usize) * (w as usize) + (px as usize)) * 4;
                    sb += bgra[off] as u64; sg += bgra[off + 1] as u64; sr += bgra[off + 2] as u64; n += 1;
                }
            }
            if n > 0 {
                let (ab, ag, ar) = ((sb / n) as u8, (sg / n) as u8, (sr / n) as u8);
                for py in y..y_end {
                    for px in x..x_end {
                        let off = ((py as usize) * (w as usize) + (px as usize)) * 4;
                        bgra[off] = ab; bgra[off + 1] = ag; bgra[off + 2] = ar;
                    }
                }
            }
            x += block;
        }
        y += block;
    }
}

fn apply_box_blur(bgra: &mut [u8], w: i32, h: i32, r: ScreenRect, radius: i32) {
    let c = r.clamp_to(w, h);
    if c.w <= 2 || c.h <= 2 { return; }
    let (rw, rh) = (c.w as usize, c.h as usize);
    let mut tmp = vec![0u8; rw * rh * 4];
    for row in 0..rh {
        let sy = c.y + row as i32;
        for col in 0..rw {
            let (mut sb, mut sg, mut sr, mut n) = (0u32, 0u32, 0u32, 0u32);
            let x0 = (c.x + col as i32 - radius).max(c.x);
            let x1 = (c.x + col as i32 + radius).min(c.right() - 1);
            for sx in x0..=x1 {
                let off = ((sy as usize) * (w as usize) + (sx as usize)) * 4;
                sb += bgra[off] as u32; sg += bgra[off + 1] as u32; sr += bgra[off + 2] as u32; n += 1;
            }
            let doff = (row * rw + col) * 4;
            tmp[doff] = (sb / n) as u8; tmp[doff + 1] = (sg / n) as u8; tmp[doff + 2] = (sr / n) as u8;
        }
    }
    for row in 0..rh {
        let y0 = (row as i32 - radius).max(0) as usize;
        let y1 = (row as i32 + radius).min(rh as i32 - 1) as usize;
        for col in 0..rw {
            let (mut sb, mut sg, mut sr, mut n) = (0u32, 0u32, 0u32, 0u32);
            for ry in y0..=y1 {
                let soff = (ry * rw + col) * 4;
                sb += tmp[soff] as u32; sg += tmp[soff + 1] as u32; sr += tmp[soff + 2] as u32; n += 1;
            }
            let off = (((c.y + row as i32) as usize) * (w as usize) + ((c.x + col as i32) as usize)) * 4;
            bgra[off] = (sb / n) as u8; bgra[off + 1] = (sg / n) as u8; bgra[off + 2] = (sr / n) as u8;
        }
    }
}

const DIGIT_FONT: [[u8; 7]; 10] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
    [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
];

fn draw_step_badge(bgra: &mut [u8], w: i32, h: i32, cx: i32, cy: i32, number: u32, radius: i32, col: AnnotColor) {
    let text_col = if col.0 > 220 && col.1 > 220 && col.2 > 220 { AnnotColor(0x18, 0x18, 0x1B) } else { AnnotColor(0xFF, 0xFF, 0xFF) };
    let r = radius as f32;
    fill_circle_aa(bgra, w, h, cx as f32, cy as f32, r + 1.6, AnnotColor(0xFF, 0xFF, 0xFF), 1.0);
    fill_circle_aa(bgra, w, h, cx as f32, cy as f32, r, col, 1.0);
    let scale = if radius >= 18 { 2 } else { 1 };
    let digits: Vec<usize> = number.clamp(1, 99).to_string().bytes().map(|b| (b - b'0') as usize).collect();
    let total_w = (digits.len() as i32) * (6 * scale) - scale;
    let (start_x, start_y) = (cx - total_w / 2, cy - (7 * scale) / 2);
    for (di, &d) in digits.iter().enumerate() {
        let gx = start_x + (di as i32) * (6 * scale);
        for (ry, &row_bits) in DIGIT_FONT[d].iter().enumerate() {
            for rx in 0..5 {
                if (row_bits & (1 << (4 - rx))) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            blend_px(bgra, w, h, gx + rx * scale + sx, start_y + (ry as i32) * scale + sy, text_col, 1.0);
                        }
                    }
                }
            }
        }
    }
}
