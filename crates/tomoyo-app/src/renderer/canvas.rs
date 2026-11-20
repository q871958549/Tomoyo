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
        let px_u32 = u32::from_le_bytes([c.2, c.1, c.0, 255]);
        // Fast aligned fill for clear background pass
        for chunk in self.buf.chunks_exact_mut(4) {
            chunk.copy_from_slice(&px_u32.to_le_bytes());
        }
    }

    pub fn fill_rect_dip(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb) {
        let ix0 = (x0 * self.scale).floor().max(0.0) as i32;
        let iy0 = (y0 * self.scale).floor().max(0.0) as i32;
        let ix1 = (x1 * self.scale).ceil().min(self.w as f32) as i32;
        let iy1 = (y1 * self.scale).ceil().min(self.h as f32) as i32;
        self.fill_rect_px(ix0, iy0, ix1, iy1, c);
    }

    fn fill_rect_px(&mut self, ix0: i32, iy0: i32, ix1: i32, iy1: i32, c: Rgb) {
        let x0 = ix0.max(0);
        let y0 = iy0.max(0);
        let x1 = ix1.min(self.w);
        let y1 = iy1.min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let px = [c.2, c.1, c.0, 255];
        for y in y0..y1 {
            let row = (y * self.w) as usize * 4;
            let start = row + (x0 as usize) * 4;
            let end = row + (x1 as usize) * 4;
            for cell in self.buf[start..end].chunks_exact_mut(4) {
                cell.copy_from_slice(&px);
            }
        }
    }

    pub fn fill_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r_dip: f32, c: Rgb) {
        let (px0, py0, px1, py1) = (
            x0 * self.scale,
            y0 * self.scale,
            x1 * self.scale,
            y1 * self.scale,
        );
        if px1 <= px0 || py1 <= py0 {
            return;
        }
        let r = (r_dip * self.scale)
            .min((px1 - px0) * 0.5)
            .min((py1 - py0) * 0.5);
        let (cx, cy) = ((px0 + px1) * 0.5, (py0 + py1) * 0.5);
        let (hx, hy) = ((px1 - px0) * 0.5 - r, (py1 - py0) * 0.5 - r);

        let ix0 = (px0.floor() as i32 - 1).max(0);
        let iy0 = (py0.floor() as i32 - 1).max(0);
        let ix1 = (px1.ceil() as i32 + 1).min(self.w);
        let iy1 = (py1.ceil() as i32 + 1).min(self.h);

        // Interior fast path: avoid `sqrt()` on all non-corner interior pixels!
        let core_x0 = (px0 + r + 1.0).ceil() as i32;
        let core_x1 = (px1 - r - 1.0).floor() as i32;
        let core_y0 = (py0 + 1.0).ceil() as i32;
        let core_y1 = (py1 - 1.0).floor() as i32;
        let band_y0 = (py0 + r + 1.0).ceil() as i32;
        let band_y1 = (py1 - r - 1.0).floor() as i32;

        if core_x0 < core_x1 && core_y0 < core_y1 {
            self.fill_rect_px(core_x0, core_y0, core_x1, core_y1, c);
        }
        if band_y0 < band_y1 {
            let left_x0 = (px0 + 1.0).ceil() as i32;
            let right_x1 = (px1 - 1.0).floor() as i32;
            if left_x0 < core_x0 {
                self.fill_rect_px(left_x0, band_y0, core_x0.min(right_x1), band_y1, c);
            }
            if core_x1 < right_x1 {
                self.fill_rect_px(core_x1.max(left_x0), band_y0, right_x1, band_y1, c);
            }
        }

        for y in iy0..iy1 {
            let in_mid_y = y >= band_y0 && y < band_y1;
            let dy = (y as f32 + 0.5 - cy).abs() - hy;
            let row = (y * self.w) as usize * 4;
            for x in ix0..ix1 {
                if in_mid_y && x >= (px0 + 1.0).ceil() as i32 && x < (px1 - 1.0).floor() as i32 {
                    continue;
                }
                if y >= core_y0 && y < core_y1 && x >= core_x0 && x < core_x1 {
                    continue;
                }
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
        let (px0, py0, px1, py1) = (
            x0 * self.scale,
            y0 * self.scale,
            x1 * self.scale,
            y1 * self.scale,
        );
        let r = (r_dip * self.scale)
            .min((px1 - px0) * 0.5)
            .min((py1 - py0) * 0.5);
        let half_stroke = 0.5 * self.scale;
        let (cx, cy) = ((px0 + px1) * 0.5, (py0 + py1) * 0.5);
        let (hx, hy) = ((px1 - px0) * 0.5 - r, (py1 - py0) * 0.5 - r);

        let ix0 = (px0.floor() as i32 - 2).max(0);
        let iy0 = (py0.floor() as i32 - 2).max(0);
        let ix1 = (px1.ceil() as i32 + 2).min(self.w);
        let iy1 = (py1.ceil() as i32 + 2).min(self.h);

        let skip_x0 = (px0 + 3.0 * self.scale).ceil() as i32;
        let skip_x1 = (px1 - 3.0 * self.scale).floor() as i32;
        let skip_y0 = (py0 + 3.0 * self.scale).ceil() as i32;
        let skip_y1 = (py1 - 3.0 * self.scale).floor() as i32;

        for y in iy0..iy1 {
            let in_skip_y = y >= skip_y0 && y < skip_y1;
            let dy = (y as f32 + 0.5 - cy).abs() - hy;
            let row = (y * self.w) as usize * 4;
            for x in ix0..ix1 {
                if in_skip_y && x >= skip_x0 && x < skip_x1 {
                    continue;
                }
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
