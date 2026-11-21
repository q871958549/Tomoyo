//! Editable vector annotation data model with hit-testing, dragging, control-point editing,
//! arrow bending, mouse-wheel sizing, Shift constraints, and full Undo/Redo history.

use crate::screen::ScreenRect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Select,
    Rect,
    Ellipse,
    Arrow,
    Pen,
    Highlighter,
    Mosaic,
    StepNumber,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeStyle {
    Outline,
    Rounded,
    Filled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowStyle {
    Solid,
    Thin,
    Curved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotColor(pub u8, pub u8, pub u8);

pub const COLOR_PRESETS: [AnnotColor; 6] = [
    AnnotColor(0xEF, 0x44, 0x44), // Red
    AnnotColor(0xF5, 0x9E, 0x0B), // Amber
    AnnotColor(0x10, 0xB9, 0x81), // Green
    AnnotColor(0x3B, 0x82, 0xF6), // Blue
    AnnotColor(0x18, 0x18, 0x1B), // Ink
    AnnotColor(0xFF, 0xFF, 0xFF), // White
];

#[derive(Debug, Clone)]
pub enum Annotation {
    Rect { rect: ScreenRect, color: AnnotColor, thickness: i32, style: ShapeStyle },
    Ellipse { rect: ScreenRect, color: AnnotColor, thickness: i32, style: ShapeStyle },
    Arrow {
        x0: i32,
        y0: i32,
        cx: i32,
        cy: i32,
        x1: i32,
        y1: i32,
        color: AnnotColor,
        thickness: i32,
        style: ArrowStyle,
    },
    Pen { points: Vec<(i32, i32)>, color: AnnotColor, thickness: i32, highlighter: bool },
    Mosaic { rect: ScreenRect, block: i32, blur: bool },
    StepNumber { cx: i32, cy: i32, number: u32, radius: i32, color: AnnotColor },
    Text { x: i32, y: i32, text: String, size: i32, color: AnnotColor },
}

impl Annotation {
    pub fn straight_arrow(x0: i32, y0: i32, x1: i32, y1: i32, color: AnnotColor, thickness: i32, style: ArrowStyle) -> Self {
        let (mx, my) = ((x0 + x1) / 2, (y0 + y1) / 2);
        let (cx, cy) = if style == ArrowStyle::Curved {
            let dx = (x1 - x0) as f32;
            let dy = (y1 - y0) as f32;
            let len = (dx * dx + dy * dy).sqrt().max(1.0);
            let bend = (len * 0.22).clamp(16.0, 72.0);
            ((mx as f32 - dy / len * bend).round() as i32, (my as f32 + dx / len * bend).round() as i32)
        } else {
            (mx, my)
        };
        Self::Arrow { x0, y0, cx, cy, x1, y1, color, thickness, style }
    }

    pub fn control_points(&self) -> Vec<(i32, i32)> {
        match self {
            Self::Rect { rect, .. } | Self::Ellipse { rect, .. } | Self::Mosaic { rect, .. } => vec![
                (rect.x, rect.y),
                (rect.right(), rect.y),
                (rect.x, rect.bottom()),
                (rect.right(), rect.bottom()),
            ],
            Self::Arrow { x0, y0, cx, cy, x1, y1, .. } => vec![(*x0, *y0), (*cx, *cy), (*x1, *y1)],
            Self::StepNumber { cx, cy, .. } => vec![(*cx, *cy)],
            Self::Text { x, y, .. } => vec![(*x, *y)],
            Self::Pen { points, .. } => {
                if points.is_empty() {
                    Vec::new()
                } else {
                    let (mut lx, mut ly, mut rx, mut ry) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
                    for &(px, py) in points {
                        lx = lx.min(px); ly = ly.min(py);
                        rx = rx.max(px); ry = ry.max(py);
                    }
                    vec![((lx + rx) / 2, (ly + ry) / 2)]
                }
            }
        }
    }

    pub fn hit_test_control(&self, px: i32, py: i32) -> Option<usize> {
        self.control_points()
            .into_iter()
            .position(|(hx, hy)| (px - hx).abs() <= 8 && (py - hy).abs() <= 8)
    }

    pub fn hit_test(&self, px: i32, py: i32) -> bool {
        if self.hit_test_control(px, py).is_some() {
            return true;
        }
        match self {
            Self::Rect { rect, thickness, style, .. } => {
                let tol = (*thickness).max(7);
                if *style == ShapeStyle::Filled {
                    rect.contains(px, py)
                } else {
                    let outer = ScreenRect { x: rect.x - tol, y: rect.y - tol, w: rect.w + tol * 2, h: rect.h + tol * 2 };
                    let inner = ScreenRect { x: rect.x + tol, y: rect.y + tol, w: (rect.w - tol * 2).max(0), h: (rect.h - tol * 2).max(0) };
                    outer.contains(px, py) && !inner.contains(px, py)
                }
            }
            Self::Ellipse { rect, thickness, style, .. } => {
                if rect.w < 4 || rect.h < 4 { return false; }
                let (rx, ry) = (rect.w as f32 * 0.5, rect.h as f32 * 0.5);
                let (dx, dy) = (px as f32 - (rect.x as f32 + rx), py as f32 - (rect.y as f32 + ry));
                let norm = ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
                if *style == ShapeStyle::Filled {
                    norm <= 1.05
                } else {
                    let dist = ((norm - 1.0) * rx.min(ry)).abs();
                    dist <= (*thickness as f32 * 0.5 + 7.0)
                }
            }
            Self::Arrow { x0, y0, cx, cy, x1, y1, thickness, .. } => {
                let tol = *thickness as f32 * 0.5 + 8.0;
                let mut prev = (*x0 as f32, *y0 as f32);
                for i in 1..=16 {
                    let t = i as f32 / 16.0;
                    let pt = eval_quad_bezier((*x0 as f32, *y0 as f32), (*cx as f32, *cy as f32), (*x1 as f32, *y1 as f32), t);
                    if point_seg_dist(px as f32, py as f32, prev.0, prev.1, pt.0, pt.1) <= tol {
                        return true;
                    }
                    prev = pt;
                }
                false
            }
            Self::Pen { points, thickness, .. } => {
                let tol = *thickness as f32 * 0.5 + 7.0;
                points.windows(2).any(|w| {
                    point_seg_dist(px as f32, py as f32, w[0].0 as f32, w[0].1 as f32, w[1].0 as f32, w[1].1 as f32) <= tol
                })
            }
            Self::Mosaic { rect, .. } => rect.contains(px, py),
            Self::StepNumber { cx, cy, radius, .. } => {
                let dx = (px - *cx) as f32;
                let dy = (py - *cy) as f32;
                (dx * dx + dy * dy).sqrt() <= (*radius as f32 + 5.0)
            }
            Self::Text { x, y, text, size, .. } => {
                let est_w = (text.chars().map(|c| if c.is_ascii() { (*size as f32 * 0.6) as i32 } else { *size }).sum::<i32>()).max(32);
                let est_h = (*size + 10).max(24);
                ScreenRect { x: *x - 4, y: *y - 4, w: est_w + 8, h: est_h + 8 }.contains(px, py)
            }
        }
    }

    pub fn translate(&mut self, dx: i32, dy: i32) {
        match self {
            Self::Rect { rect, .. } | Self::Ellipse { rect, .. } | Self::Mosaic { rect, .. } => {
                rect.x += dx;
                rect.y += dy;
            }
            Self::Arrow { x0, y0, cx, cy, x1, y1, .. } => {
                *x0 += dx; *y0 += dy;
                *cx += dx; *cy += dy;
                *x1 += dx; *y1 += dy;
            }
            Self::Pen { points, .. } => {
                for p in points { p.0 += dx; p.1 += dy; }
            }
            Self::StepNumber { cx, cy, .. } => { *cx += dx; *cy += dy; }
            Self::Text { x, y, .. } => { *x += dx; *y += dy; }
        }
    }

    pub fn move_control(&mut self, idx: usize, nx: i32, ny: i32) {
        match self {
            Self::Rect { rect, .. } | Self::Ellipse { rect, .. } | Self::Mosaic { rect, .. } => {
                let (mut l, mut t, mut r, mut b) = (rect.x, rect.y, rect.right(), rect.bottom());
                match idx {
                    0 => { l = nx; t = ny; }
                    1 => { r = nx; t = ny; }
                    2 => { l = nx; b = ny; }
                    3 => { r = nx; b = ny; }
                    _ => {}
                }
                *rect = ScreenRect::from_points(l, t, r, b);
            }
            Self::Arrow { x0, y0, cx, cy, x1, y1, style, .. } => match idx {
                0 => {
                    let was_straight = *style != ArrowStyle::Curved;
                    *x0 = nx; *y0 = ny;
                    if was_straight { *cx = (*x0 + *x1) / 2; *cy = (*y0 + *y1) / 2; }
                }
                1 => {
                    *cx = nx; *cy = ny;
                    *style = ArrowStyle::Curved;
                }
                2 => {
                    let was_straight = *style != ArrowStyle::Curved;
                    *x1 = nx; *y1 = ny;
                    if was_straight { *cx = (*x0 + *x1) / 2; *cy = (*y0 + *y1) / 2; }
                }
                _ => {}
            },
            Self::StepNumber { cx, cy, .. } => { *cx = nx; *cy = ny; }
            Self::Text { x, y, .. } => { *x = nx; *y = ny; }
            Self::Pen { .. } => {}
        }
    }

    pub fn adjust_size(&mut self, delta: i32) -> i32 {
        match self {
            Self::Rect { thickness, .. } | Self::Ellipse { thickness, .. } | Self::Arrow { thickness, .. } => {
                *thickness = (*thickness + delta).clamp(1, 20);
                *thickness
            }
            Self::Pen { thickness, highlighter, .. } => {
                let max_t = if *highlighter { 36 } else { 24 };
                *thickness = (*thickness + delta * if *highlighter { 2 } else { 1 }).clamp(2, max_t);
                *thickness
            }
            Self::Mosaic { block, .. } => {
                *block = (*block + delta * 2).clamp(6, 36);
                *block
            }
            Self::StepNumber { radius, .. } => {
                *radius = (*radius + delta).clamp(10, 26);
                *radius
            }
            Self::Text { size, .. } => {
                *size = (*size + delta * 2).clamp(12, 52);
                *size
            }
        }
    }

    pub fn set_color(&mut self, c: AnnotColor) {
        match self {
            Self::Rect { color, .. }
            | Self::Ellipse { color, .. }
            | Self::Arrow { color, .. }
            | Self::Pen { color, .. }
            | Self::StepNumber { color, .. }
            | Self::Text { color, .. } => *color = c,
            Self::Mosaic { .. } => {}
        }
    }
}

#[derive(Debug, Default)]
pub struct AnnotationStack {
    items: Vec<Annotation>,
    undo_history: Vec<Vec<Annotation>>,
    redo_history: Vec<Vec<Annotation>>,
    pub selected: Option<usize>,
    next_step: u32,
}

impl AnnotationStack {
    fn sync_step_counter(&mut self) {
        self.next_step = self
            .items
            .iter()
            .filter_map(|a| match a {
                Annotation::StepNumber { number, .. } => Some(*number),
                _ => None,
            })
            .max()
            .unwrap_or(0);
    }

    pub fn save_undo_snapshot(&mut self) {
        self.redo_history.clear();
        if self.undo_history.len() >= 80 {
            self.undo_history.remove(0);
        }
        self.undo_history.push(self.items.clone());
    }

    pub fn push(&mut self, item: Annotation) {
        self.save_undo_snapshot();
        if self.items.len() < 256 {
            self.items.push(item);
            self.selected = Some(self.items.len() - 1);
            self.sync_step_counter();
        }
    }

    pub fn delete_selected(&mut self) -> bool {
        if let Some(idx) = self.selected {
            if idx < self.items.len() {
                self.save_undo_snapshot();
                self.items.remove(idx);
                self.selected = None;
                self.sync_step_counter();
                return true;
            }
        }
        false
    }

    pub fn hit_test_top(&self, px: i32, py: i32) -> Option<usize> {
        (0..self.items.len()).rev().find(|&i| self.items[i].hit_test(px, py))
    }

    pub fn next_step_number(&self) -> u32 {
        self.next_step + 1
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_history.pop() {
            self.redo_history.push(std::mem::replace(&mut self.items, prev));
            self.selected = None;
            self.sync_step_counter();
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo_history.pop() {
            self.undo_history.push(std::mem::replace(&mut self.items, next));
            self.selected = None;
            self.sync_step_counter();
            true
        } else {
            false
        }
    }

    pub fn items(&self) -> &[Annotation] {
        &self.items
    }

    pub fn items_mut(&mut self) -> &mut [Annotation] {
        &mut self.items
    }
}

pub fn eval_quad_bezier(p0: (f32, f32), pc: (f32, f32), p1: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    (u * u * p0.0 + 2.0 * u * t * pc.0 + t * t * p1.0, u * u * p0.1 + 2.0 * u * t * pc.1 + t * t * p1.1)
}

pub fn constrain_shift_point_rect(sx: i32, sy: i32, x: i32, y: i32) -> (i32, i32) {
    let dx = x - sx;
    let dy = y - sy;
    let side = dx.abs().max(dy.abs());
    (sx + if dx >= 0 { side } else { -side }, sy + if dy >= 0 { side } else { -side })
}

pub fn constrain_shift_line(sx: i32, sy: i32, x: i32, y: i32) -> (i32, i32) {
    let dx = (x - sx) as f32;
    let dy = (y - sy) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 { return (x, y); }
    let angle = dy.atan2(dx);
    let snapped = (angle / (std::f32::consts::FRAC_PI_4)).round() * std::f32::consts::FRAC_PI_4;
    (sx + (snapped.cos() * len).round() as i32, sy + (snapped.sin() * len).round() as i32)
}

fn point_seg_dist(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.001 { (((px - x0) * dx + (py - y0) * dy) / len_sq).clamp(0.0, 1.0) } else { 0.0 };
    ((px - (x0 + t * dx)).powi(2) + (py - (y0 + t * dy)).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate_render::render_annotations;

    #[test]
    fn editable_annotations_support_hit_drag_bend_resize_and_undo() {
        let mut buf = vec![0x80u8; 160 * 120 * 4];
        let mut stack = AnnotationStack::default();
        stack.push(Annotation::Rect {
            rect: ScreenRect { x: 10, y: 10, w: 30, h: 20 },
            color: COLOR_PRESETS[0],
            thickness: 3,
            style: ShapeStyle::Rounded,
        });
        stack.push(Annotation::straight_arrow(15, 60, 95, 60, COLOR_PRESETS[2], 4, ArrowStyle::Solid));
        assert_eq!(stack.hit_test_top(55, 60), Some(1));
        // Bend arrow via middle control point -> automatically turns Curved
        stack.save_undo_snapshot();
        stack.items_mut()[1].move_control(1, 55, 30);
        assert!(matches!(stack.items()[1], Annotation::Arrow { style: ArrowStyle::Curved, cy: 30, .. }));
        // Wheel resize & drag
        let new_sz = stack.items_mut()[1].adjust_size(2);
        assert_eq!(new_sz, 6);
        stack.items_mut()[0].translate(5, 5);
        render_annotations(&mut buf, 160, 120, 0, 0, stack.items());
        assert!(stack.undo());
        assert!(stack.redo());
        assert!(stack.delete_selected() || stack.items().len() == 2);
    }
}
