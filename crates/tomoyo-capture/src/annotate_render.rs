//! Pure-Rust anti-aliased 32-bit BGRA rasterizer with modern tapered free-bend arrows,
//! subtle rounded rects, smooth pen/highlighter, mosaic/blur, step badges & hover/selection handles.

use crate::annotate::{eval_quad_bezier, AnnotColor, Annotation};
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
    if let Annotation::Arrow { x0, y0, cx, cy, x1, y1, .. } = ann {
        if ann.is_arrow_curved() {
            draw_aa_line(bgra, w, h, *x0 as f32, *y0 as f32, *cx as f32, *cy as f32, 1.0, AnnotColor(0x60, 0xA5, 0xFA), 0.45 * alpha);
            draw_aa_line(bgra, w, h, *cx as f32, *cy as f32, *x1 as f32, *y1 as f32, 1.0, AnnotColor(0x60, 0xA5, 0xFA), 0.45 * alpha);
        }
    }
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

/// Modern tapered arrow (Snipaste / PixPin / CleanShot style):
/// sleek pointed tail -> smoothly widens along the Bezier curve -> sharp concave swept-back head.
#[allow(clippy::too_many_arguments)]
fn draw_modern_tapered_arrow(
    bgra: &mut [u8],
    w: i32,
    h: i32,
    x0: f32,
    y0: f32,
    cx: f32,
    cy: f32,
    x1: f32,
    y1: f32,
    c: AnnotColor,
    t: i32,
) {
    let chord = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
    if chord < 4.0 { return; }

    // Approximate curve arc length
    let mut arc = 0.0f32;
    let mut p_prev = (x0, y0);
    for i in 1..=16 {
        let pt = eval_quad_bezier((x0, y0), (cx, cy), (x1, y1), i as f32 / 16.0);
        arc += ((pt.0 - p_prev.0).powi(2) + (pt.1 - p_prev.1).powi(2)).sqrt();
        p_prev = pt;
    }
    let len = arc.max(chord);

    let head_len = (t as f32 * 3.8 + len * 0.055).clamp(15.0, 46.0).min(len * 0.44);
    let head_half_w = head_len * 0.54;
    let notch_dist = head_len * 0.72;
    let tail_r = (t as f32 * 0.24).clamp(0.9, 2.6);
    let neck_r = (t as f32 * 0.72 + head_len * 0.12).clamp(2.4, head_half_w * 0.46);

    // Tangent at tip (from control point -> tip, or tail -> tip if degenerate)
    let (mut tdx, mut tdy) = (x1 - cx, y1 - cy);
    if (tdx * tdx + tdy * tdy).sqrt() < 2.0 {
        tdx = x1 - x0;
        tdy = y1 - y0;
    }
    let tnorm = (tdx * tdx + tdy * tdy).sqrt().max(1.0);
    let (ux, uy) = (tdx / tnorm, tdy / tnorm);

    // Find parameter t_end where Bezier reaches the concave arrowhead notch
    let t_end = (1.0 - (notch_dist / len)).clamp(0.25, 0.96);
    let steps = 36usize;
    let mut spine = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let u = i as f32 / steps as f32;
        let pt = eval_quad_bezier((x0, y0), (cx, cy), (x1, y1), u * t_end);
        let rad = tail_r + (neck_r - tail_r) * u.powf(0.88);
        spine.push((pt.0, pt.1, rad));
    }

    // Build tapered shaft quads from consecutive spine points
    for i in 0..steps {
        let (ax, ay, ar) = spine[i];
        let (bx, by, br) = spine[i + 1];
        let (dx, dy) = (bx - ax, by - ay);
        let sl = (dx * dx + dy * dy).sqrt();
        if sl < 0.05 { continue; }
        let (nx, ny) = (-dy / sl, dx / sl);
        let a_l = (ax + nx * ar, ay + ny * ar);
        let a_r = (ax - nx * ar, ay - ny * ar);
        let b_l = (bx + nx * br, by + ny * br);
        let b_r = (bx - nx * br, by - ny * br);
        fill_triangle_aa(bgra, w, h, a_l, b_l, b_r, c);
        fill_triangle_aa(bgra, w, h, a_l, b_r, a_r, c);
    }
    fill_circle_aa(bgra, w, h, x0, y0, tail_r, c, 1.0);

    // Swept-back concave arrowhead: Tip -> LeftWing -> Notch -> RightWing
    let tip = (x1, y1);
    let notch = (x1 - ux * notch_dist, y1 - uy * notch_dist);
    let base = (x1 - ux * head_len, y1 - uy * head_len);
    let left_wing = (base.0 - uy * head_half_w, base.1 + ux * head_half_w);
    let right_wing = (base.0 + uy * head_half_w, base.1 - ux * head_half_w);
    fill_triangle_aa(bgra, w, h, tip, left_wing, notch, c);
    fill_triangle_aa(bgra, w, h, tip, notch, right_wing, c);
}

fn draw_ellipse(bgra: &mut [u8], w: i32, h: i32, r: ScreenRect, c: AnnotColor, t: i32, filled: bool) {
    if r.w < 4 || r.h < 4 { return; }
    let (rx, ry) = (r.w as f32 * 0.5, r.h as f32 * 0.5);
    let (cx, cy) = (r.x as f32 + rx, r.y as f32 + ry);
    let half_t = t as f32 * 0.5;
    let (sx, sy, ex, ey) = ((r.x - t).max(0), (r.y - t).max(0), (r.right() + t).min(w), (r.bottom() + t).min(h));
    let min_r = rx.min(ry);
    for y in sy..ey {
        let dy = (y as f32 + 0.5) - cy;
        for x in sx..ex {
            let dx = (x as f32 + 0.5) - cx;
            let signed_dist = (((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt() - 1.0) * min_r;
            if filled && signed_dist < -half_t { blend_px(bgra, w, h, x, y, c, 0.28); }
            let dist = signed_dist.abs();
            if dist <= half_t + 0.8 { blend_px(bgra, w, h, x, y, c, (half_t + 0.8 - dist).clamp(0.0, 1.0)); }
        }
    }
}

fn fill_triangle_aa(bgra: &mut [u8], w: i32, h: i32, a: (f32, f32), b: (f32, f32), c: (f32, f32), col: AnnotColor) {
    let min_x = a.0.min(b.0).min(c.0).floor().max(0.0) as i32;
    let max_x = a.0.max(b.0).max(c.0).ceil().min((w - 1) as f32) as i32;
    let min_y = a.1.min(b.1).min(c.1).floor().max(0.0) as i32;
    let max_y = a.1.max(b.1).max(c.1).ceil().min((h - 1) as f32) as i32;
    let edge = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| (r.0 - p.0) * (q.1 - p.1) - (r.1 - p.1) * (q.0 - p.0);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let mut hits = 0u8;
            for &(ox, oy) in &[(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let pt = (x as f32 + ox, y as f32 + oy);
                let (e1, e2, e3) = (edge(a, b, pt), edge(b, c, pt), edge(c, a, pt));
                if (e1 >= 0.0 && e2 >= 0.0 && e3 >= 0.0) || (e1 <= 0.0 && e2 <= 0.0 && e3 <= 0.0) { hits += 1; }
            }
            if hits > 0 { blend_px(bgra, w, h, x, y, col, hits as f32 * 0.25); }
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
