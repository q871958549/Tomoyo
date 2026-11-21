//! Pure-Rust vertical & horizontal scrolling long-screenshot stitching engine.
//!
//! Scale-invariant relative-SAD matching calibrated for BOTH dense media AND sparse light/dark
//! desktop UIs (Settings, Web, Docs, Chat), with automatic fixed-sidebar/header/footer masking,
//! full-overlap pixel verification, and clean sidebar background extension.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis { Vertical, Horizontal }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchOutcome { Appended { delta_px: u32 }, Unchanged, LimitReached }

pub const MAX_STITCH_EXTENT: u32 = 16_384;

pub struct StitchCanvas {
    pub axis: ScrollAxis,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub frames: usize,
    prev_frame: Vec<u8>,
    first_frame: Vec<u8>,
    chrome_locked: bool,
    act_x0: u32,
    act_x1: u32,
    act_y0: u32,
    act_y1: u32,
    sidebar_fill_row: Vec<u8>,
}

impl StitchCanvas {
    pub fn new(axis: ScrollAxis, width: u32, height: u32, first_frame: Vec<u8>) -> Self {
        Self {
            axis, width, height,
            prev_frame: first_frame.clone(), first_frame: first_frame.clone(), bgra: first_frame,
            frames: 1, chrome_locked: false,
            act_x0: 0, act_x1: width, act_y0: 0, act_y1: height, sidebar_fill_row: Vec::new(),
        }
    }

    pub fn push_frame(&mut self, frame: &[u8], fw: u32, fh: u32) -> StitchOutcome {
        if frame.len() < (fw as usize) * (fh as usize) * 4 || fw < 32 || fh < 32 {
            return StitchOutcome::Unchanged;
        }
        match self.axis {
            ScrollAxis::Vertical => self.push_vertical(frame, fw, fh),
            ScrollAxis::Horizontal => self.push_horizontal(frame, fw, fh),
        }
    }

    fn push_vertical(&mut self, frame: &[u8], fw: u32, fh: u32) -> StitchOutcome {
        if fw != self.width || self.prev_frame.len() != (fw as usize) * (fh as usize) * 4 {
            return StitchOutcome::Unchanged;
        }
        let Some((dx0, dx1, dy0, dy1)) = detect_active_rect(&self.prev_frame, frame, fw, fh, true) else {
            return StitchOutcome::Unchanged;
        };
        if !self.chrome_locked {
            self.act_x0 = dx0;
            self.act_x1 = dx1;
            self.act_y0 = if dy0 < fh / 3 { dy0 } else { 0 };
            self.act_y1 = if fh.saturating_sub(dy1) < fh / 3 { dy1 } else { fh };
            self.sidebar_fill_row = build_clean_sidebar_row(&self.first_frame, fw, self.act_y0, self.act_y1, self.act_x0, self.act_x1);
        } else {
            self.act_x0 = self.act_x0.min(dx0);
            self.act_x1 = self.act_x1.max(dx1).min(fw.saturating_sub(18).max(dx1));
        }

        let (ax0, ax1, ay0, ay1) = (self.act_x0, self.act_x1, self.act_y0, self.act_y1);
        let (act_w, act_h) = (ax1.saturating_sub(ax0), ay1.saturating_sub(ay0));
        if act_w < 24 || act_h < 48 { return StitchOutcome::Unchanged; }

        let strip_h = (act_h / 3).clamp(24, 96).min(act_h / 2);
        let tpl_y = pick_v_template(&self.prev_frame, fw, ax0, ax1, ay0, ay1, strip_h);
        if v_strip_energy(&self.prev_frame, fw, tpl_y, strip_h, ax0, ax1) < 0.12 {
            return StitchOutcome::Unchanged;
        }
        let zero_sad = sad_v(&self.prev_frame, fw, tpl_y, frame, fw, tpl_y, strip_h, ax0, ax1, 2);
        if zero_sad < 0.28 { return StitchOutcome::Unchanged; }

        let max_cy = tpl_y.saturating_sub(4);
        if max_cy <= ay0 { return StitchOutcome::Unchanged; }

        let (best_y, best_sad, second_best) = search_best_1d(ay0, max_cy, |y, step| {
            sad_v(&self.prev_frame, fw, tpl_y, frame, fw, y, strip_h, ax0, ax1, step)
        });
        let delta = tpl_y.saturating_sub(best_y);
        if delta < 4 || delta + 24 > act_h || best_sad > 10.5 || best_sad >= zero_sad * 0.50 {
            return StitchOutcome::Unchanged;
        }

        // Scale-invariant full-overlap verification across the ENTIRE scrolling active region
        let trim = 4u32.min((act_h - delta) / 8);
        let ov_h = (act_h - delta).saturating_sub(trim * 2).max(12);
        let full_sad = sad_v(&self.prev_frame, fw, ay0 + delta + trim, frame, fw, ay0 + trim, ov_h, ax0, ax1, 2);
        let full_zero = sad_v(&self.prev_frame, fw, ay0 + trim, frame, fw, ay0 + trim, ov_h, ax0, ax1, 2);
        let much_better = full_sad < full_zero * 0.45;
        let confident = much_better && (full_sad < 2.8 || (full_sad < 8.5 && second_best > best_sad * 1.35 + 0.40));
        if !confident { return StitchOutcome::Unchanged; }
        if self.height + delta > MAX_STITCH_EXTENT { return StitchOutcome::LimitReached; }

        self.chrome_locked = true;
        let bot_static = fh.saturating_sub(ay1) as usize;
        let stride = (fw as usize) * 4;
        let (old_h, new_h) = (self.height as usize, self.height as usize + delta as usize);
        self.bgra.resize(new_h * stride, 255);

        let cx0 = if ax0 >= 44 { ax0 as usize } else { 0 };
        let cx1 = if fw.saturating_sub(ax1) >= 44 { ax1 as usize } else { fw as usize };
        let insert_y = old_h.saturating_sub(bot_static);
        let src_base = (ay1 - delta) as usize;
        for row in 0..delta as usize {
            let (dst_off, src_off) = ((insert_y + row) * stride, (src_base + row) * stride);
            if !self.sidebar_fill_row.is_empty() {
                if cx0 > 0 { self.bgra[dst_off..dst_off + cx0 * 4].copy_from_slice(&self.sidebar_fill_row[..cx0 * 4]); }
                if cx1 < fw as usize { self.bgra[dst_off + cx1 * 4..dst_off + stride].copy_from_slice(&self.sidebar_fill_row[cx1 * 4..stride]); }
            }
            self.bgra[dst_off + cx0 * 4..dst_off + cx1 * 4].copy_from_slice(&frame[src_off + cx0 * 4..src_off + cx1 * 4]);
        }
        for row in 0..bot_static {
            let (dst_off, src_off) = ((new_h - bot_static + row) * stride, (ay1 as usize + row) * stride);
            self.bgra[dst_off..dst_off + stride].copy_from_slice(&frame[src_off..src_off + stride]);
        }
        self.prev_frame.copy_from_slice(&frame[..stride * (fh as usize)]);
        self.height = new_h as u32;
        self.frames += 1;
        StitchOutcome::Appended { delta_px: delta }
    }

    fn push_horizontal(&mut self, frame: &[u8], fw: u32, fh: u32) -> StitchOutcome {
        if fh != self.height || self.prev_frame.len() != (fw as usize) * (fh as usize) * 4 {
            return StitchOutcome::Unchanged;
        }
        let Some((dx0, dx1, dy0, dy1)) = detect_active_rect(&self.prev_frame, frame, fw, fh, false) else {
            return StitchOutcome::Unchanged;
        };
        if !self.chrome_locked {
            self.act_x0 = if dx0 < fw / 3 { dx0 } else { 0 };
            self.act_x1 = if fw.saturating_sub(dx1) < fw / 3 { dx1 } else { fw };
            self.act_y0 = dy0;
            self.act_y1 = dy1;
        }
        let (ax0, ax1, ay0, ay1) = (self.act_x0, self.act_x1, self.act_y0, self.act_y1);
        let (act_w, act_h) = (ax1.saturating_sub(ax0), ay1.saturating_sub(ay0));
        if act_w < 48 || act_h < 24 { return StitchOutcome::Unchanged; }

        let strip_w = (act_w / 3).clamp(24, 96).min(act_w / 2);
        let tpl_x = ax1.saturating_sub(strip_w + 4).max(ax0 + 4);
        let zero_sad = sad_h(&self.prev_frame, fw, tpl_x, frame, fw, tpl_x, strip_w, ay0, ay1, 2);
        let max_cx = tpl_x.saturating_sub(4);
        if max_cx <= ax0 || zero_sad < 0.28 { return StitchOutcome::Unchanged; }

        let (best_x, best_sad, second_best) = search_best_1d(ax0, max_cx, |x, step| {
            sad_h(&self.prev_frame, fw, tpl_x, frame, fw, x, strip_w, ay0, ay1, step)
        });
        let delta = tpl_x.saturating_sub(best_x);
        if delta < 4 || delta + 24 > act_w || best_sad > 10.5 || best_sad >= zero_sad * 0.50 {
            return StitchOutcome::Unchanged;
        }

        let trim = 4u32.min((act_w - delta) / 8);
        let ov_w = (act_w - delta).saturating_sub(trim * 2).max(12);
        let full_sad = sad_h(&self.prev_frame, fw, ax0 + delta + trim, frame, fw, ax0 + trim, ov_w, ay0, ay1, 2);
        let full_zero = sad_h(&self.prev_frame, fw, ax0 + trim, frame, fw, ax0 + trim, ov_w, ay0, ay1, 2);
        if !(full_sad < full_zero * 0.45 && (full_sad < 2.8 || (full_sad < 8.5 && second_best > best_sad * 1.35 + 0.40))) {
            return StitchOutcome::Unchanged;
        }
        if self.width + delta > MAX_STITCH_EXTENT { return StitchOutcome::LimitReached; }

        self.chrome_locked = true;
        let right_static = fw.saturating_sub(ax1) as usize;
        let (old_w, new_w, h) = (self.width as usize, self.width as usize + delta as usize, self.height as usize);
        let (insert_x, src_base) = (old_w.saturating_sub(right_static), (ax1 - delta) as usize);
        let mut next = vec![255u8; new_w * h * 4];
        for row in 0..h {
            let (dr, or, sr) = (row * new_w * 4, row * old_w * 4, row * (fw as usize) * 4);
            next[dr..dr + insert_x * 4].copy_from_slice(&self.bgra[or..or + insert_x * 4]);
            next[dr + insert_x * 4..dr + (insert_x + delta as usize) * 4]
                .copy_from_slice(&frame[sr + src_base * 4..sr + (src_base + delta as usize) * 4]);
            if right_static > 0 {
                next[dr + (new_w - right_static) * 4..dr + new_w * 4]
                    .copy_from_slice(&frame[sr + (ax1 as usize) * 4..sr + (fw as usize) * 4]);
            }
        }
        self.bgra = next;
        self.prev_frame.copy_from_slice(&frame[..(fw as usize) * h * 4]);
        self.width = new_w as u32;
        self.frames += 1;
        StitchOutcome::Appended { delta_px: delta }
    }
}

fn search_best_1d(lo: u32, hi: u32, mut eval: impl FnMut(u32, u32) -> f32) -> (u32, f32, f32) {
    let mut scores = Vec::with_capacity(((hi - lo) / 2 + 2) as usize);
    let mut p = lo;
    while p <= hi { scores.push((p, eval(p, 3))); p += 2; }
    let coarse = scores.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|&(x, _)| x).unwrap_or(lo);
    let (mut best_p, mut best_s) = (coarse, f32::MAX);
    for fp in coarse.saturating_sub(3).max(lo)..=(coarse + 3).min(hi) {
        let s = eval(fp, 1);
        if s < best_s { best_s = s; best_p = fp; }
    }
    let second = scores.iter().filter(|&&(x, _)| x.abs_diff(best_p) >= 14).map(|&(_, s)| s).fold(f32::MAX, f32::min);
    (best_p, best_s, second)
}

pub fn frame_mean_diff(a: &[u8], b: &[u8], w: u32, h: u32) -> f32 {
    let n_bytes = (w as usize) * (h as usize) * 4;
    if a.len() < n_bytes || b.len() < n_bytes { return 255.0; }
    let (mut sum, mut cnt, mut y) = (0u64, 0u64, 0u32);
    while y < h {
        let row = (y as usize) * (w as usize) * 4;
        let mut x = 0u32;
        while x < w {
            let o = row + (x as usize) * 4;
            sum += (a[o] as i32 - b[o] as i32).unsigned_abs() as u64
                + (a[o + 1] as i32 - b[o + 1] as i32).unsigned_abs() as u64
                + (a[o + 2] as i32 - b[o + 2] as i32).unsigned_abs() as u64;
            cnt += 3; x += 4;
        }
        y += 4;
    }
    if cnt == 0 { 0.0 } else { sum as f32 / cnt as f32 }
}

fn detect_active_rect(a: &[u8], b: &[u8], w: u32, h: u32, vertical: bool) -> Option<(u32, u32, u32, u32)> {
    let sb_r = if vertical { w.saturating_sub(18).max(w * 3 / 4) } else { w };
    let sb_b = if !vertical { h.saturating_sub(18).max(h * 3 / 4) } else { h };
    let mut row_ch = vec![false; h as usize];
    for y in 0..sb_b {
        let row = (y as usize) * (w as usize) * 4;
        let (mut diff, mut x) = (0u32, 2u32);
        while x < sb_r {
            let o = row + (x as usize) * 4;
            if (a[o] as i32 - b[o] as i32).unsigned_abs() + (a[o + 1] as i32 - b[o + 1] as i32).unsigned_abs() + (a[o + 2] as i32 - b[o + 2] as i32).unsigned_abs() > 14 {
                diff += 1;
                if diff >= 3 { row_ch[y as usize] = true; break; }
            }
            x += 2;
        }
    }
    let y0 = row_ch.iter().position(|&v| v)? as u32;
    let y1 = (row_ch.iter().rposition(|&v| v)? as u32) + 1;
    if y1.saturating_sub(y0) < 20 { return None; }

    let mut col_ch = vec![false; sb_r as usize];
    for x in 0..sb_r {
        let (mut diff, mut y) = (0u32, y0);
        while y < y1 {
            let o = ((y as usize) * (w as usize) + (x as usize)) * 4;
            if (a[o] as i32 - b[o] as i32).unsigned_abs() + (a[o + 1] as i32 - b[o + 1] as i32).unsigned_abs() + (a[o + 2] as i32 - b[o + 2] as i32).unsigned_abs() > 14 {
                diff += 1;
                if diff >= 3 { col_ch[x as usize] = true; break; }
            }
            y += 2;
        }
    }
    let x0 = col_ch.iter().position(|&v| v)? as u32;
    let x1 = (col_ch.iter().rposition(|&v| v)? as u32) + 1;
    if x1.saturating_sub(x0) < 20 { return None; }
    Some((x0, x1, y0, y1))
}

fn build_clean_sidebar_row(first: &[u8], fw: u32, ay0: u32, ay1: u32, ax0: u32, ax1: u32) -> Vec<u8> {
    let stride = (fw as usize) * 4;
    if ax0 < 44 && fw.saturating_sub(ax1) < 44 { return Vec::new(); }
    let (sx0, sx1) = if ax0 >= 44 { (4, ax0.saturating_sub(4)) } else { (ax1 + 4, fw.saturating_sub(4)) };
    let (mut best_y, mut best_grad) = (ay0, u64::MAX);
    for y in ay0..ay1 {
        let row = (y as usize) * stride;
        let mut g = 0u64;
        for x in sx0..sx1 {
            let o = row + (x as usize) * 4;
            g += (first[o] as i32 - first[o + 4] as i32).unsigned_abs() as u64
                + (first[o + 1] as i32 - first[o + 5] as i32).unsigned_abs() as u64
                + (first[o + 2] as i32 - first[o + 6] as i32).unsigned_abs() as u64;
        }
        if g < best_grad { best_grad = g; best_y = y; }
    }
    first[(best_y as usize) * stride..(best_y as usize + 1) * stride].to_vec()
}

fn v_strip_energy(buf: &[u8], w: u32, y0: u32, sh: u32, x0: u32, x1: u32) -> f32 {
    let (mut sum, mut cnt, mut y) = (0u64, 0u64, y0);
    while y + 2 < y0 + sh {
        let (r0, r1) = ((y as usize) * (w as usize) * 4, ((y + 2) as usize) * (w as usize) * 4);
        let mut x = x0;
        while x + 2 < x1 {
            let o = (x as usize) * 4;
            sum += (buf[r0 + o] as i32 - buf[r1 + o] as i32).unsigned_abs() as u64
                + (buf[r0 + o + 1] as i32 - buf[r0 + o + 9] as i32).unsigned_abs() as u64;
            cnt += 2; x += 4;
        }
        y += 2;
    }
    if cnt == 0 { 0.0 } else { sum as f32 / cnt as f32 }
}

fn pick_v_template(buf: &[u8], w: u32, x0: u32, x1: u32, ay0: u32, ay1: u32, sh: u32) -> u32 {
    let bot = ay1.saturating_sub(sh + 4);
    let min_y = bot.saturating_sub(((ay1 - ay0) / 4).min(48)).max(ay0 + 12);
    let (mut best_y, mut best_e) = (bot, -1.0f32);
    let mut y = bot;
    while y >= min_y {
        let e = v_strip_energy(buf, w, y, sh, x0, x1);
        if e >= 0.8 { return y; }
        if e > best_e { best_e = e; best_y = y; }
        if y < min_y + 8 { break; }
        y -= 8;
    }
    best_y
}

#[allow(clippy::too_many_arguments)]
fn sad_v(a: &[u8], aw: u32, ay: u32, b: &[u8], bw: u32, by: u32, sh: u32, x0: u32, x1: u32, step: u32) -> f32 {
    let (mut sum, mut n, mut r, st) = (0u64, 0u64, 0u32, step.max(1));
    while r < sh {
        let (ar, br) = (((ay + r) as usize) * (aw as usize) * 4, ((by + r) as usize) * (bw as usize) * 4);
        let mut x = x0;
        while x < x1 {
            let (ao, bo) = (ar + (x as usize) * 4, br + (x as usize) * 4);
            sum += (a[ao] as i32 - b[bo] as i32).unsigned_abs() as u64
                + (a[ao + 1] as i32 - b[bo + 1] as i32).unsigned_abs() as u64
                + (a[ao + 2] as i32 - b[bo + 2] as i32).unsigned_abs() as u64;
            n += 3; x += st;
        }
        r += st;
    }
    if n == 0 { 255.0 } else { sum as f32 / n as f32 }
}

#[allow(clippy::too_many_arguments)]
fn sad_h(a: &[u8], aw: u32, ax: u32, b: &[u8], bw: u32, bx: u32, sw: u32, y0: u32, y1: u32, step: u32) -> f32 {
    let (mut sum, mut n, mut y, st) = (0u64, 0u64, y0, step.max(1));
    while y < y1 {
        let (ar, br) = ((y as usize) * (aw as usize) * 4, (y as usize) * (bw as usize) * 4);
        let mut c = 0;
        while c < sw {
            let (ao, bo) = (ar + ((ax + c) as usize) * 4, br + ((bx + c) as usize) * 4);
            sum += (a[ao] as i32 - b[bo] as i32).unsigned_abs() as u64
                + (a[ao + 1] as i32 - b[bo + 1] as i32).unsigned_abs() as u64
                + (a[ao + 2] as i32 - b[bo + 2] as i32).unsigned_abs() as u64;
            n += 3; c += st;
        }
        y += st;
    }
    if n == 0 { 255.0 } else { sum as f32 / n as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sparse_white_settings_doc(w: u32, h: u32) -> Vec<u8> {
        let mut buf = vec![252u8; (w * h * 4) as usize];
        // Draw sparse realistic thin gray text lines on pure white cards (95% white!)
        for row_idx in 0..(h / 26) {
            let y = row_idx * 26 + 8;
            let bar_w = 40 + ((row_idx * 37) % (w - 80).max(20));
            let x_start = 16 + ((row_idx * 13) % 24);
            for dy in 0..3 {
                for x in x_start..(x_start + bar_w).min(w - 12) {
                    if (x + row_idx * 7) % 5 != 0 {
                        let o = (((y + dy) * w + x) * 4) as usize;
                        buf[o] = 95; buf[o + 1] = 100; buf[o + 2] = 112;
                    }
                }
            }
        }
        buf
    }

    fn compose_white_app(doc: &[u8], dw: u32, scroll_y: u32, vw: u32, vh: u32) -> Vec<u8> {
        let mut out = vec![255u8; (vw * vh * 4) as usize];
        let (hdr, ftr, side) = (24u32, 24u32, 64u32);
        for y in 0..hdr { for x in 0..vw { let o = ((y * vw + x) * 4) as usize; out[o] = 240; out[o + 1] = 242; out[o + 2] = 246; } }
        for y in (vh - ftr)..vh { for x in 0..vw { let o = ((y * vw + x) * 4) as usize; out[o] = 240; out[o + 1] = 242; out[o + 2] = 246; } }
        for y in hdr..(vh - ftr) {
            for x in 0..side {
                let o = ((y * vw + x) * 4) as usize;
                let nav = y < hdr + 60 && (y % 14 < 4) && x > 10 && x < side - 12;
                let c = if nav { 70 } else { 255 };
                out[o] = c; out[o + 1] = c; out[o + 2] = c;
            }
            let dy = scroll_y + (y - hdr);
            for x in 0..(vw - side) {
                let (so, doff) = (((dy * dw + x) * 4) as usize, ((y * vw + side + x) * 4) as usize);
                out[doff..doff + 4].copy_from_slice(&doc[so..so + 4]);
            }
        }
        out
    }

    #[test]
    fn stitches_sparse_white_settings_window_with_fixed_sidebar_without_duplication() {
        let doc = sparse_white_settings_doc(240, 600);
        let f0 = compose_white_app(&doc, 240, 0, 304, 220);
        let f1 = compose_white_app(&doc, 240, 72, 304, 220);
        let f2 = compose_white_app(&doc, 240, 144, 304, 220);
        let mut cv = StitchCanvas::new(ScrollAxis::Vertical, 304, 220, f0);
        assert_eq!(cv.push_frame(&f1, 304, 220), StitchOutcome::Appended { delta_px: 72 });
        assert_eq!(cv.push_frame(&f2, 304, 220), StitchOutcome::Appended { delta_px: 72 });
        assert_eq!(cv.push_frame(&f2, 304, 220), StitchOutcome::Unchanged);
        assert_eq!(cv.height, 364);
        assert_eq!(cv.bgra[((230 * 304 + 20) * 4) as usize], 255);
    }

    #[test]
    fn stitches_horizontal_scroll_accurately() {
        let doc = sparse_white_settings_doc(480, 180);
        let crop = |x0: u32| -> Vec<u8> {
            let mut out = vec![255u8; 200 * 180 * 4];
            for r in 0..180u32 {
                let (so, doff) = (((r * 480 + x0) * 4) as usize, (r * 200 * 4) as usize);
                out[doff..doff + 200 * 4].copy_from_slice(&doc[so..so + 200 * 4]);
            }
            out
        };
        let mut ch = StitchCanvas::new(ScrollAxis::Horizontal, 200, 180, crop(0));
        assert_eq!(ch.push_frame(&crop(64), 200, 180), StitchOutcome::Appended { delta_px: 64 });
        assert_eq!(ch.push_frame(&crop(128), 200, 180), StitchOutcome::Appended { delta_px: 64 });
        assert_eq!(ch.width, 328);
    }
}
