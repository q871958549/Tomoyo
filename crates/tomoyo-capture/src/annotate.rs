//! Pure-Rust 32-bit BGRA software annotation engine (Rect, Ellipse, Arrow, Pen, Mosaic, Step Badge)
//! with bounded Undo/Redo history and zero Direct2D/GDI+ dependencies.

use crate::screen::ScreenRect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Select,
    Rect,
    Ellipse,
    Arrow,
    Pen,
    Mosaic,
    StepNumber,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotColor(pub u8, pub u8, pub u8);

pub const COLOR_PRESETS: [AnnotColor; 5] = [
    AnnotColor(0xEF, 0x44, 0x44), // Red
    AnnotColor(0xF5, 0x9E, 0x0B), // Amber
    AnnotColor(0x10, 0xB9, 0x81), // Green
    AnnotColor(0x3B, 0x82, 0xF6), // Blue
    AnnotColor(0xFF, 0xFF, 0xFF), // White
];

#[derive(Debug, Clone)]
pub enum Annotation {
    Rect { rect: ScreenRect, color: AnnotColor, thickness: i32 },
    Ellipse { rect: ScreenRect, color: AnnotColor, thickness: i32 },
    Arrow { x0: i32, y0: i32, x1: i32, y1: i32, color: AnnotColor, thickness: i32 },
    Pen { points: Vec<(i32, i32)>, color: AnnotColor, thickness: i32 },
    Mosaic { rect: ScreenRect, block: i32 },
    StepNumber { cx: i32, cy: i32, number: u32, color: AnnotColor },
}

#[derive(Debug, Default)]
pub struct AnnotationStack {
    items: Vec<Annotation>,
    redo_buf: Vec<Annotation>,
    next_step: u32,
}

impl AnnotationStack {
    pub fn push(&mut self, item: Annotation) {
        if let Annotation::StepNumber { number, .. } = item {
            self.next_step = self.next_step.max(number);
        }
        self.redo_buf.clear();
        if self.items.len() < 256 {
            self.items.push(item);
        }
    }

    pub fn next_step_number(&self) -> u32 {
        self.next_step + 1
    }

    pub fn undo(&mut self) -> bool {
        if let Some(last) = self.items.pop() {
            if let Annotation::StepNumber { number, .. } = last {
                if self.next_step == number {
                    self.next_step = number.saturating_sub(1);
                }
            }
            self.redo_buf.push(last);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(item) = self.redo_buf.pop() {
            if let Annotation::StepNumber { number, .. } = item {
                self.next_step = self.next_step.max(number);
            }
            self.items.push(item);
            true
        } else {
            false
        }
    }

    pub fn items(&self) -> &[Annotation] {
        &self.items
    }
}

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
        Annotation::Rect { rect, color, thickness } => {
            let r = ScreenRect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_rect_outline(bgra, w, h, r, *color, (*thickness).clamp(1, 12));
        }
        Annotation::Ellipse { rect, color, thickness } => {
            let r = ScreenRect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_ellipse_outline(bgra, w, h, r, *color, (*thickness).clamp(1, 12));
        }
        Annotation::Arrow { x0, y0, x1, y1, color, thickness } => {
            draw_arrow(bgra, w, h, x0 - ox, y0 - oy, x1 - ox, y1 - oy, *color, (*thickness).clamp(2, 10));
        }
        Annotation::Pen { points, color, thickness } => {
            let rad = ((*thickness).clamp(1, 12) as f32) * 0.55;
            for seg in points.windows(2) {
                draw_thick_segment(bgra, w, h, seg[0].0 - ox, seg[0].1 - oy, seg[1].0 - ox, seg[1].1 - oy, rad, *color);
            }
            if let Some(&(px, py)) = points.first() {
                fill_circle(bgra, w, h, px - ox, py - oy, rad.max(1.5), *color);
            }
        }
        Annotation::Mosaic { rect, block } => {
            let r = ScreenRect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            apply_mosaic(bgra, w, h, r, (*block).clamp(6, 24));
        }
        Annotation::StepNumber { cx, cy, number, color } => {
            draw_step_badge(bgra, w, h, cx - ox, cy - oy, *number, *color);
        }
    }
}

#[inline]
fn put_pixel(bgra: &mut [u8], w: i32, h: i32, x: i32, y: i32, c: AnnotColor, alpha: f32) {
    if x < 0 || y < 0 || x >= w || y >= h || alpha <= 0.01 {
        return;
    }
    let off = ((y as usize) * (w as usize) + (x as usize)) * 4;
    if off + 3 >= bgra.len() {
        return;
    }
    let a = alpha.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    bgra[off] = (c.2 as f32 * a + bgra[off] as f32 * inv).round() as u8;
    bgra[off + 1] = (c.1 as f32 * a + bgra[off + 1] as f32 * inv).round() as u8;
    bgra[off + 2] = (c.0 as f32 * a + bgra[off + 2] as f32 * inv).round() as u8;
    bgra[off + 3] = 255;
}

fn fill_rect(bgra: &mut [u8], w: i32, h: i32, x0: i32, y0: i32, x1: i32, y1: i32, c: AnnotColor) {
    let sx = x0.max(0);
    let sy = y0.max(0);
    let ex = x1.min(w);
    let ey = y1.min(h);
    for y in sy..ey {
        for x in sx..ex {
            put_pixel(bgra, w, h, x, y, c, 1.0);
        }
    }
}

fn draw_rect_outline(bgra: &mut [u8], w: i32, h: i32, r: ScreenRect, c: AnnotColor, t: i32) {
    if r.w < 2 || r.h < 2 {
        return;
    }
    fill_rect(bgra, w, h, r.x, r.y, r.right(), r.y + t, c);
    fill_rect(bgra, w, h, r.x, r.bottom() - t, r.right(), r.bottom(), c);
    fill_rect(bgra, w, h, r.x, r.y, r.x + t, r.bottom(), c);
    fill_rect(bgra, w, h, r.right() - t, r.y, r.right(), r.bottom(), c);
}

fn draw_ellipse_outline(bgra: &mut [u8], w: i32, h: i32, r: ScreenRect, c: AnnotColor, t: i32) {
    if r.w < 4 || r.h < 4 {
        return;
    }
    let rx = r.w as f32 * 0.5;
    let ry = r.h as f32 * 0.5;
    let cx = r.x as f32 + rx;
    let cy = r.y as f32 + ry;
    let half_t = t as f32 * 0.5;
    let sx = (r.x - t).max(0);
    let sy = (r.y - t).max(0);
    let ex = (r.right() + t).min(w);
    let ey = (r.bottom() + t).min(h);
    for y in sy..ey {
        let dy = (y as f32 + 0.5) - cy;
        for x in sx..ex {
            let dx = (x as f32 + 0.5) - cx;
            let norm = ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
            let dist = ((norm - 1.0) * rx.min(ry)).abs();
            if dist <= half_t + 0.8 {
                let cov = (half_t + 0.8 - dist).clamp(0.0, 1.0);
                put_pixel(bgra, w, h, x, y, c, cov);
            }
        }
    }
}

fn fill_circle(bgra: &mut [u8], w: i32, h: i32, cx: i32, cy: i32, radius: f32, c: AnnotColor) {
    let r_i = radius.ceil() as i32 + 1;
    for y in (cy - r_i).max(0)..=(cy + r_i).min(h - 1) {
        let dy = (y - cy) as f32;
        for x in (cx - r_i).max(0)..=(cx + r_i).min(w - 1) {
            let dx = (x - cx) as f32;
            let d = (dx * dx + dy * dy).sqrt();
            if d <= radius + 0.7 {
                put_pixel(bgra, w, h, x, y, c, (radius + 0.7 - d).clamp(0.0, 1.0));
            }
        }
    }
}

fn draw_thick_segment(
    bgra: &mut [u8],
    w: i32,
    h: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    radius: f32,
    c: AnnotColor,
) {
    let dx = (x1 - x0) as f32;
    let dy = (y1 - y0) as f32;
    let len_sq = dx * dx + dy * dy;
    let pad = radius.ceil() as i32 + 2;
    let sx = x0.min(x1).saturating_sub(pad).max(0);
    let sy = y0.min(y1).saturating_sub(pad).max(0);
    let ex = (x0.max(x1) + pad).min(w - 1);
    let ey = (y0.max(y1) + pad).min(h - 1);
    for y in sy..=ey {
        for x in sx..=ex {
            let px = (x - x0) as f32;
            let py = (y - y0) as f32;
            let t = if len_sq > 0.001 {
                ((px * dx + py * dy) / len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let dist = ((px - t * dx).powi(2) + (py - t * dy).powi(2)).sqrt();
            if dist <= radius + 0.75 {
                put_pixel(bgra, w, h, x, y, c, (radius + 0.75 - dist).clamp(0.0, 1.0));
            }
        }
    }
}

fn draw_arrow(bgra: &mut [u8], w: i32, h: i32, x0: i32, y0: i32, x1: i32, y1: i32, c: AnnotColor, t: i32) {
    let dx = (x1 - x0) as f32;
    let dy = (y1 - y0) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 3.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let head_len = (t as f32 * 4.2).clamp(11.0, 24.0).min(len * 0.55);
    let head_w = head_len * 0.58;
    let sx = (x1 as f32 - ux * head_len * 0.75).round() as i32;
    let sy = (y1 as f32 - uy * head_len * 0.75).round() as i32;
    draw_thick_segment(bgra, w, h, x0, y0, sx, sy, t as f32 * 0.55, c);

    let bx = x1 as f32 - ux * head_len;
    let by = y1 as f32 - uy * head_len;
    let p1 = (x1 as f32, y1 as f32);
    let p2 = (bx - uy * head_w, by + ux * head_w);
    let p3 = (bx + uy * head_w, by - ux * head_w);
    fill_triangle(bgra, w, h, p1, p2, p3, c);
}

fn fill_triangle(bgra: &mut [u8], w: i32, h: i32, a: (f32, f32), b: (f32, f32), c: (f32, f32), col: AnnotColor) {
    let min_x = a.0.min(b.0).min(c.0).floor().max(0.0) as i32;
    let max_x = a.0.max(b.0).max(c.0).ceil().min((w - 1) as f32) as i32;
    let min_y = a.1.min(b.1).min(c.1).floor().max(0.0) as i32;
    let max_y = a.1.max(b.1).max(c.1).ceil().min((h - 1) as f32) as i32;
    let edge = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| (r.0 - p.0) * (q.1 - p.1) - (r.1 - p.1) * (q.0 - p.0);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let pt = (x as f32 + 0.5, y as f32 + 0.5);
            let e1 = edge(a, b, pt);
            let e2 = edge(b, c, pt);
            let e3 = edge(c, a, pt);
            let inside = (e1 >= 0.0 && e2 >= 0.0 && e3 >= 0.0) || (e1 <= 0.0 && e2 <= 0.0 && e3 <= 0.0);
            if inside {
                put_pixel(bgra, w, h, x, y, col, 1.0);
            }
        }
    }
}

fn apply_mosaic(bgra: &mut [u8], w: i32, h: i32, r: ScreenRect, block: i32) {
    let c = r.clamp_to(w, h);
    if c.w <= 0 || c.h <= 0 {
        return;
    }
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
                    sb += bgra[off] as u64;
                    sg += bgra[off + 1] as u64;
                    sr += bgra[off + 2] as u64;
                    n += 1;
                }
            }
            if n > 0 {
                let (ab, ag, ar) = ((sb / n) as u8, (sg / n) as u8, (sr / n) as u8);
                for py in y..y_end {
                    for px in x..x_end {
                        let off = ((py as usize) * (w as usize) + (px as usize)) * 4;
                        bgra[off] = ab;
                        bgra[off + 1] = ag;
                        bgra[off + 2] = ar;
                    }
                }
            }
            x += block;
        }
        y += block;
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

fn draw_step_badge(bgra: &mut [u8], w: i32, h: i32, cx: i32, cy: i32, number: u32, col: AnnotColor) {
    let text_col = if col.0 > 220 && col.1 > 220 && col.2 > 220 {
        AnnotColor(0x18, 0x18, 0x1B)
    } else {
        AnnotColor(0xFF, 0xFF, 0xFF)
    };
    fill_circle(bgra, w, h, cx, cy, 13.5, AnnotColor(0xFF, 0xFF, 0xFF));
    fill_circle(bgra, w, h, cx, cy, 12.0, col);
    let s = number.clamp(1, 99).to_string();
    let digits: Vec<usize> = s.bytes().map(|b| (b - b'0') as usize).collect();
    let total_w = (digits.len() as i32) * 7 - 2;
    let start_x = cx - total_w / 2;
    let start_y = cy - 4;
    for (di, &d) in digits.iter().enumerate() {
        let gx = start_x + (di as i32) * 7;
        for (ry, &row_bits) in DIGIT_FONT[d].iter().enumerate() {
            for rx in 0..5 {
                if (row_bits & (1 << (4 - rx))) != 0 {
                    put_pixel(bgra, w, h, gx + rx, start_y + ry as i32, text_col, 1.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_six_annotation_tools_and_supports_undo_redo() {
        let mut buf = vec![0x80u8; 120 * 100 * 4];
        let mut stack = AnnotationStack::default();
        stack.push(Annotation::Rect {
            rect: ScreenRect { x: 10, y: 10, w: 30, h: 20 },
            color: COLOR_PRESETS[0],
            thickness: 3,
        });
        stack.push(Annotation::Ellipse {
            rect: ScreenRect { x: 50, y: 10, w: 30, h: 20 },
            color: COLOR_PRESETS[1],
            thickness: 2,
        });
        stack.push(Annotation::Arrow {
            x0: 10, y0: 50, x1: 60, y1: 60,
            color: COLOR_PRESETS[2],
            thickness: 3,
        });
        stack.push(Annotation::Pen {
            points: vec![(15, 75), (25, 80), (40, 72)],
            color: COLOR_PRESETS[3],
            thickness: 3,
        });
        stack.push(Annotation::Mosaic {
            rect: ScreenRect { x: 70, y: 50, w: 30, h: 30 },
            block: 10,
        });
        stack.push(Annotation::StepNumber { cx: 95, cy: 25, number: 1, color: COLOR_PRESETS[0] });
        assert_eq!(stack.items().len(), 6);
        assert_eq!(stack.next_step_number(), 2);
        assert!(stack.undo());
        assert_eq!(stack.next_step_number(), 1);
        assert!(stack.redo());
        assert_eq!(stack.next_step_number(), 2);
        render_annotations(&mut buf, 120, 100, 0, 0, stack.items());
        // Rect top-left (10,10) must now be Red (BGR: 0x44, 0x44, 0xEF)
        let off = (10 * 120 + 10) * 4;
        assert_eq!(buf[off + 2], 0xEF);
    }
}
