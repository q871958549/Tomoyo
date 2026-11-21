//! Closed-form analytic Cardano cubic quadratic Bezier closest-point solver & single-pass
//! SDF modern tapered arrow rasterizer (zero polygonal facets even on sharp 180-degree bends).

use crate::annotate::{bezier_ctrl_from_midpoint, eval_quad_bezier, AnnotColor};
use crate::overlay_icons::blend_px;

/// Closed-form analytic quadratic Bezier closest-point solver (Inigo Quilez Cardano cubic).
/// Returns `(euclidean_dist, closest_t)` for `t` clamped to `[0.0, t_end]`.
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
        let n = v.sin() * 1.732_050_8;
        let r1 = eval((m + m) * z - kx);
        let r2 = eval((-n - m) * z - kx);
        let r3 = eval((n - m) * z - kx);
        let r12 = if r1.0 <= r2.0 { r1 } else { r2 };
        if r12.0 <= r3.0 { r12 } else { r3 }
    };
    (best_d2.sqrt(), best_t)
}

/// Exact analytic signed distance to a 4-vertex simple polygon (handles concave swept-back heads).
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
#[allow(clippy::too_many_arguments)]
pub fn draw_modern_tapered_arrow(
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

    let head_len = (t as f32 * 3.8 + total_len * 0.052).clamp(14.0, 46.0).min(total_len * 0.46);
    let head_half_w = head_len * 0.52;
    let notch_dist = head_len * 0.72;
    let tail_r = (t as f32 * 0.22).clamp(0.75, 2.4).min(head_half_w * 0.20).max(0.5);
    let neck_r = (t as f32 * 0.65 + head_len * 0.11).min(head_half_w * 0.44).max(tail_r + 0.35);

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

    let pad = neck_r + 2.0;
    let mut boxes = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 4];
    for (bi, slot) in boxes.iter_mut().enumerate() {
        let (s0, s1) = (bi * 16, (bi + 1) * 16);
        let (mut lx, mut ly, mut rx, mut ry) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(px, py) in &curve[s0..=s1] {
            lx = lx.min(px - pad); ly = ly.min(py - pad);
            rx = rx.max(px + pad); ry = ry.max(py + pad);
        }
        *slot = (lx, ly, rx, ry);
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
            if !in_shaft && !in_head { continue; }
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
