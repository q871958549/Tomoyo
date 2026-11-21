//! Pure-Rust vertical & horizontal scrolling long-screenshot stitching engine.
//!
//! Uses coarse-to-fine normalized SAD template matching with automatic scrollbar & sticky-bar
//! margin exclusion, 4px feather seam blending, and strict bounded canvas limits (0 idle RAM).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchOutcome {
    Appended { delta_px: u32 },
    Unchanged,
    LimitReached,
}

/// Hard safety cap along the scroll axis to prevent unbounded RAM allocation.
pub const MAX_STITCH_EXTENT: u32 = 16_384;

pub struct StitchCanvas {
    pub axis: ScrollAxis,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub frames: usize,
}

impl StitchCanvas {
    pub fn new(axis: ScrollAxis, width: u32, height: u32, first_frame: Vec<u8>) -> Self {
        Self { axis, width, height, bgra: first_frame, frames: 1 }
    }

    pub fn push_frame(&mut self, frame: &[u8], fw: u32, fh: u32) -> StitchOutcome {
        if frame.len() < (fw as usize) * (fh as usize) * 4 || fw < 16 || fh < 16 {
            return StitchOutcome::Unchanged;
        }
        match self.axis {
            ScrollAxis::Vertical => self.push_vertical(frame, fw, fh),
            ScrollAxis::Horizontal => self.push_horizontal(frame, fw, fh),
        }
    }

    fn push_vertical(&mut self, frame: &[u8], fw: u32, fh: u32) -> StitchOutcome {
        if fw != self.width {
            return StitchOutcome::Unchanged;
        }
        // Exclude right scrollbar (18px) & outer left edge (4px) and top/bottom 10% sticky bars
        let x0 = 4u32.min(fw / 8);
        let x1 = fw.saturating_sub(18).max(x0 + 8).min(fw);
        let top_guard = (fh / 10).clamp(4, 56);
        let bot_guard = (fh / 12).clamp(4, 48);
        let strip_h = (fh / 5).clamp(18, 44).min(fh.saturating_sub(top_guard + bot_guard));
        if strip_h < 10 || self.height < strip_h + bot_guard {
            return StitchOutcome::Unchanged;
        }

        // Template strip taken near bottom of existing stitched canvas (above sticky footer)
        let tpl_y = self.height - bot_guard - strip_h;

        // Quick check: if frame matches tail with zero scroll (offset == fh - bot_guard - strip_h)
        let zero_y = fh - bot_guard - strip_h;
        if sad_v(&self.bgra, self.width, tpl_y, frame, fw, zero_y, strip_h, x0, x1, 2) < 3.5 {
            return StitchOutcome::Unchanged;
        }

        // Coarse search (step 2) across candidate positions in the new frame where the template moved UP
        let mut best_y = zero_y;
        let mut best_sad = f32::MAX;
        let max_search_y = zero_y.saturating_sub(4);
        let mut cy = top_guard;
        while cy <= max_search_y {
            let s = sad_v(&self.bgra, self.width, tpl_y, frame, fw, cy, strip_h, x0, x1, 3);
            if s < best_sad {
                best_sad = s;
                best_y = cy;
            }
            cy += 2;
        }
        // Fine refinement ±3px at step 1
        let r0 = best_y.saturating_sub(3).max(top_guard);
        let r1 = (best_y + 3).min(max_search_y);
        for fy in r0..=r1 {
            let s = sad_v(&self.bgra, self.width, tpl_y, frame, fw, fy, strip_h, x0, x1, 1);
            if s < best_sad {
                best_sad = s;
                best_y = fy;
            }
        }

        if best_sad > 22.0 || best_y >= zero_y {
            return StitchOutcome::Unchanged;
        }
        let delta = zero_y - best_y;
        if delta < 4 {
            return StitchOutcome::Unchanged;
        }
        if self.height + delta > MAX_STITCH_EXTENT {
            return StitchOutcome::LimitReached;
        }

        // Append rows below the matched strip bottom (`best_y + strip_h`) replacing the old sticky footer
        let join_canvas_y = (tpl_y + strip_h) as usize;
        let src_start_y = (best_y + strip_h) as usize;
        let copy_rows = (fh as usize).saturating_sub(src_start_y);
        let new_h = join_canvas_y + copy_rows;
        let stride = (self.width as usize) * 4;
        self.bgra.resize(new_h * stride, 255);

        for row in 0..copy_rows {
            let dst_off = (join_canvas_y + row) * stride;
            let src_off = (src_start_y + row) * stride;
            if row < 4 && join_canvas_y + row < self.height as usize {
                let a = (row + 1) as u16 * 51; // 20%, 40%, 60%, 80% feather blend
                let inv = 255 - a;
                for i in 0..stride {
                    self.bgra[dst_off + i] = ((frame[src_off + i] as u16 * a + self.bgra[dst_off + i] as u16 * inv) >> 8) as u8;
                }
            } else {
                self.bgra[dst_off..dst_off + stride].copy_from_slice(&frame[src_off..src_off + stride]);
            }
        }
        self.height = new_h as u32;
        self.frames += 1;
        StitchOutcome::Appended { delta_px: delta }
    }

    fn push_horizontal(&mut self, frame: &[u8], fw: u32, fh: u32) -> StitchOutcome {
        if fh != self.height {
            return StitchOutcome::Unchanged;
        }
        let y0 = 4u32.min(fh / 8);
        let y1 = fh.saturating_sub(18).max(y0 + 8).min(fh);
        let left_guard = (fw / 10).clamp(4, 56);
        let right_guard = (fw / 12).clamp(4, 48);
        let strip_w = (fw / 5).clamp(18, 44).min(fw.saturating_sub(left_guard + right_guard));
        if strip_w < 10 || self.width < strip_w + right_guard {
            return StitchOutcome::Unchanged;
        }

        let tpl_x = self.width - right_guard - strip_w;
        let zero_x = fw - right_guard - strip_w;
        if sad_h(&self.bgra, self.width, tpl_x, frame, fw, zero_x, strip_w, y0, y1, 2) < 3.5 {
            return StitchOutcome::Unchanged;
        }

        let mut best_x = zero_x;
        let mut best_sad = f32::MAX;
        let max_search_x = zero_x.saturating_sub(4);
        let mut cx = left_guard;
        while cx <= max_search_x {
            let s = sad_h(&self.bgra, self.width, tpl_x, frame, fw, cx, strip_w, y0, y1, 3);
            if s < best_sad {
                best_sad = s;
                best_x = cx;
            }
            cx += 2;
        }
        let r0 = best_x.saturating_sub(3).max(left_guard);
        let r1 = (best_x + 3).min(max_search_x);
        for fx in r0..=r1 {
            let s = sad_h(&self.bgra, self.width, tpl_x, frame, fw, fx, strip_w, y0, y1, 1);
            if s < best_sad {
                best_sad = s;
                best_x = fx;
            }
        }

        if best_sad > 22.0 || best_x >= zero_x {
            return StitchOutcome::Unchanged;
        }
        let delta = zero_x - best_x;
        if delta < 4 {
            return StitchOutcome::Unchanged;
        }
        if self.width + delta > MAX_STITCH_EXTENT {
            return StitchOutcome::LimitReached;
        }

        let join_x = (tpl_x + strip_w) as usize;
        let src_start_x = (best_x + strip_w) as usize;
        let copy_cols = (fw as usize).saturating_sub(src_start_x);
        let new_w = join_x + copy_cols;
        let old_w = self.width as usize;
        let h = self.height as usize;
        let mut next = vec![255u8; new_w * h * 4];

        for row in 0..h {
            let keep_bytes = join_x.min(old_w) * 4;
            let dst_row = row * new_w * 4;
            let old_row = row * old_w * 4;
            next[dst_row..dst_row + keep_bytes].copy_from_slice(&self.bgra[old_row..old_row + keep_bytes]);
            let src_row = row * (fw as usize) * 4;
            let add_bytes = copy_cols * 4;
            next[dst_row + join_x * 4..dst_row + join_x * 4 + add_bytes]
                .copy_from_slice(&frame[src_row + src_start_x * 4..src_row + src_start_x * 4 + add_bytes]);
        }
        self.bgra = next;
        self.width = new_w as u32;
        self.frames += 1;
        StitchOutcome::Appended { delta_px: delta }
    }
}

#[allow(clippy::too_many_arguments)]
fn sad_v(a: &[u8], aw: u32, ay: u32, b: &[u8], bw: u32, by: u32, sh: u32, x0: u32, x1: u32, step: u32) -> f32 {
    let mut sum = 0u64;
    let mut n = 0u64;
    let mut r = 0;
    while r < sh {
        let a_row = ((ay + r) as usize) * (aw as usize) * 4;
        let b_row = ((by + r) as usize) * (bw as usize) * 4;
        let mut x = x0;
        while x < x1 {
            let ao = a_row + (x as usize) * 4;
            let bo = b_row + (x as usize) * 4;
            sum += (a[ao] as i32 - b[bo] as i32).unsigned_abs() as u64
                + (a[ao + 1] as i32 - b[bo + 1] as i32).unsigned_abs() as u64
                + (a[ao + 2] as i32 - b[bo + 2] as i32).unsigned_abs() as u64;
            n += 3;
            x += step.max(1);
        }
        r += step.max(1);
    }
    if n == 0 { 255.0 } else { sum as f32 / n as f32 }
}

#[allow(clippy::too_many_arguments)]
fn sad_h(a: &[u8], aw: u32, ax: u32, b: &[u8], bw: u32, bx: u32, sw: u32, y0: u32, y1: u32, step: u32) -> f32 {
    let mut sum = 0u64;
    let mut n = 0u64;
    let mut y = y0;
    while y < y1 {
        let a_row = (y as usize) * (aw as usize) * 4;
        let b_row = (y as usize) * (bw as usize) * 4;
        let mut c = 0;
        while c < sw {
            let ao = a_row + ((ax + c) as usize) * 4;
            let bo = b_row + ((bx + c) as usize) * 4;
            sum += (a[ao] as i32 - b[bo] as i32).unsigned_abs() as u64
                + (a[ao + 1] as i32 - b[bo + 1] as i32).unsigned_abs() as u64
                + (a[ao + 2] as i32 - b[bo + 2] as i32).unsigned_abs() as u64;
            n += 3;
            c += step.max(1);
        }
        y += step.max(1);
    }
    if n == 0 { 255.0 } else { sum as f32 / n as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_page(w: u32, h: u32) -> Vec<u8> {
        let mut buf = vec![255u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let off = ((y * w + x) * 4) as usize;
                let stripe = ((y / 11).wrapping_mul(53) ^ (x / 13).wrapping_mul(37)) as u8;
                buf[off] = stripe.wrapping_add((y & 0xFF) as u8);
                buf[off + 1] = stripe.wrapping_mul(3).wrapping_add((x & 0x7F) as u8);
                buf[off + 2] = ((y * 7 + x * 5) & 0xFF) as u8;
            }
        }
        buf
    }

    fn crop(page: &[u8], pw: u32, x0: u32, y0: u32, cw: u32, ch: u32) -> Vec<u8> {
        let mut out = vec![255u8; (cw * ch * 4) as usize];
        for r in 0..ch {
            let so = (((y0 + r) * pw + x0) * 4) as usize;
            let doff = (r * cw * 4) as usize;
            out[doff..doff + (cw * 4) as usize].copy_from_slice(&page[so..so + (cw * 4) as usize]);
        }
        out
    }

    #[test]
    fn stitches_vertical_and_horizontal_scroll_frames_accurately() {
        // Vertical test: 180x360 full document scrolled in 180x160 viewport by +80px twice
        let doc_v = synthetic_page(180, 360);
        let mut cv = StitchCanvas::new(ScrollAxis::Vertical, 180, 160, crop(&doc_v, 180, 0, 0, 180, 160));
        assert_eq!(cv.push_frame(&crop(&doc_v, 180, 0, 80, 180, 160), 180, 160), StitchOutcome::Appended { delta_px: 80 });
        assert_eq!(cv.push_frame(&crop(&doc_v, 180, 0, 160, 180, 160), 180, 160), StitchOutcome::Appended { delta_px: 80 });
        assert_eq!(cv.push_frame(&crop(&doc_v, 180, 0, 160, 180, 160), 180, 160), StitchOutcome::Unchanged);
        assert_eq!(cv.height, 320);

        // Horizontal test: 360x180 wide spreadsheet scrolled in 160x180 viewport by +72px twice
        let doc_h = synthetic_page(360, 180);
        let mut ch = StitchCanvas::new(ScrollAxis::Horizontal, 160, 180, crop(&doc_h, 360, 0, 0, 160, 180));
        assert_eq!(ch.push_frame(&crop(&doc_h, 360, 72, 0, 160, 180), 160, 180), StitchOutcome::Appended { delta_px: 72 });
        assert_eq!(ch.push_frame(&crop(&doc_h, 360, 144, 0, 160, 180), 160, 180), StitchOutcome::Appended { delta_px: 72 });
        assert_eq!(ch.width, 304);
    }
}
