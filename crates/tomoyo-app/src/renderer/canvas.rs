//! Zero-dependency pure-Rust SDF anti-aliased 32-bit BGRA software rasterizer & RGBA thumbnail blitter.

use super::theme::Rgb;

pub struct Canvas<'a> {
    pub buf: &'a mut [u8],
    pub w: i32,
    pub h: i32,
    pub scale: f32,
}

impl<'a> Canvas<'a> {
    pub fn clear(&mut self, c: Rgb) {
        let px = [c.2, c.1, c.0, 255];
        for chunk in self.buf.chunks_exact_mut(4) {
            chunk.copy_from_slice(&px);
        }
    }

    pub fn fill_rect_dip(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb) {
        let ix0 = (x0 * self.scale).floor().max(0.0) as i32;
        let iy0 = (y0 * self.scale).floor().max(0.0) as i32;
        let ix1 = (x1 * self.scale).ceil().min(self.w as f32) as i32;
        let iy1 = (y1 * self.scale).ceil().min(self.h as f32) as i32;
        let px = [c.2, c.1, c.0, 255];
        for y in iy0..iy1 {
            let row = (y * self.w) as usize * 4;
            for x in ix0..ix1 {
                let off = row + (x as usize) * 4;
                self.buf[off..off + 4].copy_from_slice(&px);
            }
        }
    }

    pub fn fill_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r_dip: f32, c: Rgb) {
        let (px0, py0, px1, py1) = (x0 * self.scale, y0 * self.scale, x1 * self.scale, y1 * self.scale);
        let r = (r_dip * self.scale)
            .min((px1 - px0) * 0.5)
            .min((py1 - py0) * 0.5);
        let (cx, cy) = ((px0 + px1) * 0.5, (py0 + py1) * 0.5);
        let (hx, hy) = ((px1 - px0) * 0.5 - r, (py1 - py0) * 0.5 - r);

        let ix0 = (px0.floor() as i32 - 1).max(0);
        let iy0 = (py0.floor() as i32 - 1).max(0);
        let ix1 = (px1.ceil() as i32 + 1).min(self.w);
        let iy1 = (py1.ceil() as i32 + 1).min(self.h);

        for y in iy0..iy1 {
            let dy = (y as f32 + 0.5 - cy).abs() - hy;
            let row = (y * self.w) as usize * 4;
            for x in ix0..ix1 {
                let dx = (x as f32 + 0.5 - cx).abs() - hx;
                let dist =
                    (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - r;
                let alpha = (0.5 - dist).clamp(0.0, 1.0);
                if alpha > 0.0 {
                    blend_pixel(&mut self.buf[row + (x as usize) * 4..], c, alpha);
                }
            }
        }
    }

    pub fn stroke_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r_dip: f32, c: Rgb) {
        let (px0, py0, px1, py1) = (x0 * self.scale, y0 * self.scale, x1 * self.scale, y1 * self.scale);
        let r = (r_dip * self.scale)
            .min((px1 - px0) * 0.5)
            .min((py1 - py0) * 0.5);
        let half_stroke = 0.65 * self.scale;
        let (cx, cy) = ((px0 + px1) * 0.5, (py0 + py1) * 0.5);
        let (hx, hy) = ((px1 - px0) * 0.5 - r, (py1 - py0) * 0.5 - r);

        let ix0 = (px0.floor() as i32 - 2).max(0);
        let iy0 = (py0.floor() as i32 - 2).max(0);
        let ix1 = (px1.ceil() as i32 + 2).min(self.w);
        let iy1 = (py1.ceil() as i32 + 2).min(self.h);

        for y in iy0..iy1 {
            let dy = (y as f32 + 0.5 - cy).abs() - hy;
            let row = (y * self.w) as usize * 4;
            for x in ix0..ix1 {
                let dx = (x as f32 + 0.5 - cx).abs() - hx;
                let dist =
                    (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - r;
                let edge_dist = dist.abs() - half_stroke;
                let alpha = (0.5 - edge_dist).clamp(0.0, 1.0);
                if alpha > 0.0 {
                    blend_pixel(&mut self.buf[row + (x as usize) * 4..], c, alpha);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_rgba_image(
        &mut self,
        x0_dip: f32,
        y0_dip: f32,
        w_dip: f32,
        h_dip: f32,
        src_w: u32,
        src_h: u32,
        rgba: &[u8],
    ) {
        if src_w == 0 || src_h == 0 || rgba.len() < (src_w as usize * src_h as usize * 4) {
            return;
        }
        let px0 = (x0_dip * self.scale).round() as i32;
        let py0 = (y0_dip * self.scale).round() as i32;
        let pw = (w_dip * self.scale).round().max(1.0) as i32;
        let ph = (h_dip * self.scale).round().max(1.0) as i32;

        let (x_start, y_start) = (px0.max(0), py0.max(0));
        let (x_end, y_end) = ((px0 + pw).min(self.w), (py0 + ph).min(self.h));

        for y in y_start..y_end {
            let sy = (((y - py0) as u64 * src_h as u64) / ph as u64).min(src_h as u64 - 1) as usize;
            let row = (y * self.w) as usize * 4;
            for x in x_start..x_end {
                let sx = (((x - px0) as u64 * src_w as u64) / pw as u64).min(src_w as u64 - 1) as usize;
                let si = (sy * src_w as usize + sx) * 4;
                let a = rgba[si + 3] as f32 / 255.0;
                if a > 0.0 {
                    blend_pixel(
                        &mut self.buf[row + (x as usize) * 4..],
                        Rgb(rgba[si], rgba[si + 1], rgba[si + 2]),
                        a,
                    );
                }
            }
        }
    }
}

#[inline(always)]
fn blend_pixel(dst: &mut [u8], src: Rgb, alpha: f32) {
    if alpha >= 0.999 {
        dst[0] = src.2;
        dst[1] = src.1;
        dst[2] = src.0;
        dst[3] = 255;
        return;
    }
    let inv = 1.0 - alpha;
    dst[0] = (src.2 as f32 * alpha + dst[0] as f32 * inv) as u8;
    dst[1] = (src.1 as f32 * alpha + dst[1] as f32 * inv) as u8;
    dst[2] = (src.0 as f32 * alpha + dst[2] as f32 * inv) as u8;
    dst[3] = 255;
}
