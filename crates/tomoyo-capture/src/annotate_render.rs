//! Pure-Rust anti-aliased 32-bit BGRA rasterizer for editable vector annotations
//! (Rect/Rounded/Filled, Ellipse, Solid/Thin/Curved Bezier Arrow, Smooth Pen & Highlighter,
//! Pixelate Mosaic & Box Blur, Scalable Step Badges, and Selection Control Handles).

use crate::annotate::{eval_quad_bezier, AnnotColor, Annotation, ArrowStyle, ShapeStyle};
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

pub fn render_single(
    bgra: &mut [u8],
    w: i32,
    h: i32,
    ox: i32,
    oy: i32,
    ann: &Annotation,
) {
    match ann {
        Annotation::Rect { rect, color, thickness, style } => {
            let r = ScreenRect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            let t = (*thickness).clamp(1, 20) as f32;
            match style {
                ShapeStyle::Outline => stroke_rounded_rect_aa(bgra, w, h, r, 0.5, t, *color, 1.0),
                ShapeStyle::Rounded => stroke_rounded_rect_aa(bgra, w, h, r, 8.0, t, *color, 1.0),
                ShapeStyle::Filled => {
                    fill_rounded_rect_aa(bgra, w, h, r, 6.0, *color, 0.28);
                    stroke_rounded_rect_aa(bgra, w, h, r, 6.0, t, *color, 1.0);
                }
            }
        }
        Annotation::Ellipse { rect, color, thickness, style } => {
            let r = ScreenRect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_ellipse(bgra, w, h, r, *color, (*thickness).clamp(1, 20), *style == ShapeStyle::Filled);
        }
        Annotation::Arrow { x0, y0, cx, cy, x1, y1, color, thickness, style } => {
            draw_arrow(
                bgra, w, h,
                (x0 - ox) as f32, (y0 - oy) as f32,
                (cx - ox) as f32, (cy - oy) as f32,
                (x1 - ox) as f32, (y1 - oy) as f32,
                *color, (*thickness).clamp(1, 20), *style,
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
        Annotation::Text { .. } => {
            // Crisp ClearType text is rendered in the GDI pass (both live overlay and export DIB).
        }
    }
}

pub fn draw_selection_handles(bgra: &mut [u8], w: i32, h: i32, ann: &Annotation) {
    if let Annotation::Arrow { x0, y0, cx, cy, x1, y1, .. } = ann {
        draw_aa_line(bgra, w, h, *x0 as f32, *y0 as f32, *cx as f32, *cy as f32, 1.0, AnnotColor(0x60, 0xA5, 0xFA), 0.55);
        draw_aa_line(bgra, w, h, *cx as f32, *cy as f32, *x1 as f32, *y1 as f32, 1.0, AnnotColor(0x60, 0xA5, 0xFA), 0.55);
    }
    let pts = ann.control_points();
    for (idx, (hx, hy)) in pts.iter().enumerate() {
        let is_bend = matches!(ann, Annotation::Arrow { .. }) && idx == 1;
        let fill = if is_bend { AnnotColor(0xF5, 0x9E, 0x0B) } else { AnnotColor(0xFF, 0xFF, 0xFF) };
        let ring = if is_bend { AnnotColor(0xFF, 0xFF, 0xFF) } else { AnnotColor(0x25, 0x63, 0xEB) };
        fill_circle_aa(bgra, w, h, *hx as f32 + 0.5, *hy as f32 + 0.5, 5.0, fill, 1.0);
        stroke_circle_aa(bgra, w, h, *hx as f32 + 0.5, *hy as f32 + 0.5, 5.0, 1.6, ring, 1.0);
    }
}

fn draw_ellipse(bgra: &mut [u8], w: i32, h: i32, r: ScreenRect, c: AnnotColor, t: i32, filled: bool) {
    if r.w < 4 || r.h < 4 { return; }
    let (rx, ry) = (r.w as f32 * 0.5, r.h as f32 * 0.5);
    let (cx, cy) = (r.x as f32 + rx, r.y as f32 + ry);
    let half_t = t as f32 * 0.5;
    let sx = (r.x - t).max(0);
    let sy = (r.y - t).max(0);
    let ex = (r.right() + t).min(w);
    let ey = (r.bottom() + t).min(h);
    let min_r = rx.min(ry);
    for y in sy..ey {
        let dy = (y as f32 + 0.5) - cy;
        for x in sx..ex {
            let dx = (x as f32 + 0.5) - cx;
            let norm = ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
            let signed_dist = (norm - 1.0) * min_r;
            if filled && signed_dist < -half_t {
                blend_px(bgra, w, h, x, y, c, 0.28);
            }
            let dist = signed_dist.abs();
            if dist <= half_t + 0.8 {
                let cov = (half_t + 0.8 - dist).clamp(0.0, 1.0);
                blend_px(bgra, w, h, x, y, c, cov);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_arrow(
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
    style: ArrowStyle,
) {
    let chord_len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
    if chord_len < 4.0 { return; }
    let head_len = (t as f32 * 4.0).clamp(12.0, 32.0).min(chord_len * 0.55);
    let head_w = head_len * 0.58;

    // End tangent vector (from control point -> tip, falling back to start -> tip if degenerate)
    let (mut tdx, mut tdy) = (x1 - cx, y1 - cy);
    let tlen = (tdx * tdx + tdy * tdy).sqrt();
    if tlen < 2.0 {
        tdx = x1 - x0;
        tdy = y1 - y0;
    }
    let norm = (tdx * tdx + tdy * tdy).sqrt().max(1.0);
    let (ux, uy) = (tdx / norm, tdy / norm);

    // Shaft ends slightly inside arrowhead base for Solid style
    let shaft_end = if style == ArrowStyle::Thin {
        (x1, y1)
    } else {
        (x1 - ux * head_len * 0.65, y1 - uy * head_len * 0.65)
    };

    let steps = if style == ArrowStyle::Curved { 28 } else { 1 };
    let mut prev = (x0, y0);
    for i in 1..=steps {
        let frac = i as f32 / steps as f32;
        let pt = if steps == 1 {
            shaft_end
        } else {
            eval_quad_bezier((x0, y0), (cx, cy), shaft_end, frac)
        };
        draw_aa_line(bgra, w, h, prev.0, prev.1, pt.0, pt.1, t as f32, c, 1.0);
        prev = pt;
    }

    let bx = x1 - ux * head_len;
    let by = y1 - uy * head_len;
    let p2 = (bx - uy * head_w, by + ux * head_w);
    let p3 = (bx + uy * head_w, by - ux * head_w);
    if style == ArrowStyle::Thin {
        draw_aa_line(bgra, w, h, x1, y1, p2.0, p2.1, t as f32, c, 1.0);
        draw_aa_line(bgra, w, h, x1, y1, p3.0, p3.1, t as f32, c, 1.0);
    } else {
        fill_triangle_aa(bgra, w, h, (x1, y1), p2, p3, c);
    }
}

fn fill_triangle_aa(
    bgra: &mut [u8],
    w: i32,
    h: i32,
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    col: AnnotColor,
) {
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
                if (e1 >= 0.0 && e2 >= 0.0 && e3 >= 0.0) || (e1 <= 0.0 && e2 <= 0.0 && e3 <= 0.0) {
                    hits += 1;
                }
            }
            if hits > 0 {
                blend_px(bgra, w, h, x, y, col, hits as f32 * 0.25);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_smooth_pen(
    bgra: &mut [u8],
    w: i32,
    h: i32,
    ox: i32,
    oy: i32,
    points: &[(i32, i32)],
    thickness: f32,
    color: AnnotColor,
    alpha: f32,
) {
    if alpha < 0.99 {
        // For semi-transparent highlighter, rasterize per-pixel min distance to polyline
        // so overlapping joints never double-darken!
        let half = thickness * 0.5;
        let pad = half.ceil() as i32 + 2;
        let (mut lx, mut ly, mut rx, mut ry) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for &(px, py) in points {
            lx = lx.min(px - ox); ly = ly.min(py - oy);
            rx = rx.max(px - ox); ry = ry.max(py - oy);
        }
        let sx = (lx - pad).max(0);
        let sy = (ly - pad).max(0);
        let ex = (rx + pad).min(w);
        let ey = (ry + pad).min(h);
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
                if best < half + 0.8 {
                    let cov = (half + 0.5 - best).clamp(0.0, 1.0);
                    blend_px(bgra, w, h, x, y, color, cov * alpha);
                }
            }
        }
        return;
    }

    if points.len() == 2 {
        draw_aa_line(bgra, w, h, (points[0].0 - ox) as f32, (points[0].1 - oy) as f32, (points[1].0 - ox) as f32, (points[1].1 - oy) as f32, thickness, color, 1.0);
        return;
    }
    // Quadratic midpoint smoothing across raw pen samples
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
    // Horizontal pass
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
    // Vertical pass
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
    let text_col = if col.0 > 220 && col.1 > 220 && col.2 > 220 {
        AnnotColor(0x18, 0x18, 0x1B)
    } else {
        AnnotColor(0xFF, 0xFF, 0xFF)
    };
    let r = radius as f32;
    fill_circle_aa(bgra, w, h, cx as f32, cy as f32, r + 1.6, AnnotColor(0xFF, 0xFF, 0xFF), 1.0);
    fill_circle_aa(bgra, w, h, cx as f32, cy as f32, r, col, 1.0);
    let scale = if radius >= 18 { 2 } else { 1 };
    let s = number.clamp(1, 99).to_string();
    let digits: Vec<usize> = s.bytes().map(|b| (b - b'0') as usize).collect();
    let total_w = (digits.len() as i32) * (6 * scale) - scale;
    let start_x = cx - total_w / 2;
    let start_y = cy - (7 * scale) / 2;
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
