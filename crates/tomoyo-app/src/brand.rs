//! Procedural Modern White artistic Tomoyo brand icon rasterizer.
//!
//! Renders the porcelain-white rounded squircle, sculptural calligraphic obsidian "T",
//! and iridescent silver-ice pearl dot with 2x2 supersampled anti-aliasing at any DPI.
//! Used coherently across the Win32 system tray icon, window icon, and launcher header!

/// Paints the Modern White porcelain Tomoyo emblem onto a 32-bit BGRA pixel buffer
/// at `(dst_x, dst_y)` with size `size_px x size_px`.
pub fn paint_modern_white_emblem(
    buf: &mut [u8],
    buf_w: i32,
    buf_h: i32,
    dst_x: i32,
    dst_y: i32,
    size_px: i32,
) {
    if size_px <= 0 {
        return;
    }
    let inv = 1.0 / size_px as f32;

    for ly in 0..size_px {
        let py = dst_y + ly;
        if py < 0 || py >= buf_h {
            continue;
        }
        let row = (py * buf_w) as usize * 4;
        for lx in 0..size_px {
            let px = dst_x + lx;
            if px < 0 || px >= buf_w {
                continue;
            }

            // 2x2 supersampling for razor-sharp calligraphic edges at small icon sizes (16-48px)
            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut acc_a = 0.0f32;

            for sy in 0..2 {
                for sx in 0..2 {
                    let u = (lx as f32 + (sx as f32 + 0.5) * 0.5) * inv;
                    let v = (ly as f32 + (sy as f32 + 0.5) * 0.5) * inv;
                    let (r, g, b, a) = sample_modern_white_icon(u, v);
                    acc_r += r * a;
                    acc_g += g * a;
                    acc_b += b * a;
                    acc_a += a;
                }
            }

            let alpha = (acc_a * 0.25).clamp(0.0, 1.0);
            if alpha <= 0.002 {
                continue;
            }
            let sr = (acc_r / acc_a.max(1e-5)).clamp(0.0, 255.0);
            let sg = (acc_g / acc_a.max(1e-5)).clamp(0.0, 255.0);
            let sb = (acc_b / acc_a.max(1e-5)).clamp(0.0, 255.0);

            let off = row + (px as usize) * 4;
            if alpha >= 0.996 {
                buf[off] = sb as u8;
                buf[off + 1] = sg as u8;
                buf[off + 2] = sr as u8;
                buf[off + 3] = 255;
            } else {
                let inv_a = 1.0 - alpha;
                buf[off] = (sb * alpha + buf[off] as f32 * inv_a) as u8;
                buf[off + 1] = (sg * alpha + buf[off + 1] as f32 * inv_a) as u8;
                buf[off + 2] = (sr * alpha + buf[off + 2] as f32 * inv_a) as u8;
                buf[off + 3] = 255;
            }
        }
    }
}

/// Generates a standalone transparent 32-bit BGRA icon buffer (`size x size`) for `CreateIconIndirect`.
pub fn generate_icon_bgra(size: i32) -> Vec<u8> {
    let mut bgra = vec![0u8; (size * size * 4) as usize];
    let inv = 1.0 / size as f32;

    for y in 0..size {
        for x in 0..size {
            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut acc_a = 0.0f32;

            for sy in 0..2 {
                for sx in 0..2 {
                    let u = (x as f32 + (sx as f32 + 0.5) * 0.5) * inv;
                    let v = (y as f32 + (sy as f32 + 0.5) * 0.5) * inv;
                    let (r, g, b, a) = sample_modern_white_icon(u, v);
                    acc_r += r * a;
                    acc_g += g * a;
                    acc_b += b * a;
                    acc_a += a;
                }
            }

            let alpha = (acc_a * 0.25).clamp(0.0, 1.0);
            let idx = ((y * size + x) * 4) as usize;
            if alpha > 0.01 {
                let r = (acc_r / acc_a.max(1e-5)).clamp(0.0, 255.0);
                let g = (acc_g / acc_a.max(1e-5)).clamp(0.0, 255.0);
                let b = (acc_b / acc_a.max(1e-5)).clamp(0.0, 255.0);
                bgra[idx] = b as u8;
                bgra[idx + 1] = g as u8;
                bgra[idx + 2] = r as u8;
                bgra[idx + 3] = (alpha * 255.0) as u8;
            }
        }
    }
    bgra
}

/// Evaluates normalized `(u, v) in [0, 1]^2` for the Modern White porcelain squircle +
/// calligraphic deep-obsidian "T" + silver-ice pearl dot.
#[inline]
fn sample_modern_white_icon(u: f32, v: f32) -> (f32, f32, f32, f32) {
    // 1. Porcelain squircle tile SDF
    let cx = (u - 0.5).abs() - 0.24;
    let cy = (v - 0.5).abs() - 0.24;
    let tile_dist = (cx.max(0.0).powi(2) + cy.max(0.0).powi(2)).sqrt() + cx.max(cy).min(0.0) - 0.23;
    let tile_alpha = ((0.015 - tile_dist) / 0.03).clamp(0.0, 1.0);
    if tile_alpha <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }

    // Modern porcelain-white vertical gradient (#FFFFFF -> #ECEFF5) + crisp ceramic rim bevel
    let grad = (v * 0.85 + u * 0.15).clamp(0.0, 1.0);
    let mut r = 254.0 - grad * 16.0;
    let mut g = 254.0 - grad * 14.0;
    let mut b = 255.0 - grad * 9.0;

    // Subtle frosted rim highlight on outer bevel
    if tile_dist > -0.038 {
        let rim = ((tile_dist + 0.038) / 0.038).clamp(0.0, 1.0);
        if v < 0.5 {
            // Top specular white sheen
            r = r + (255.0 - r) * rim * 0.7;
            g = g + (255.0 - g) * rim * 0.7;
            b = b + (255.0 - b) * rim * 0.7;
        } else {
            // Soft ceramic shadow rim at bottom
            r -= rim * 26.0;
            g -= rim * 24.0;
            b -= rim * 20.0;
        }
    }

    // 2. Sculptural Calligraphic Obsidian "T" (2 sweeping quadratic Bezier ribbon strokes!)
    // Crossbar curve: gentle wave rising rightwards from (0.22, 0.38) through (0.50, 0.30) to (0.79, 0.24)
    let bar_d = sd_tapered_quad_bezier(
        (u, v),
        (0.21, 0.39),
        (0.49, 0.32),
        (0.79, 0.25),
        0.016,
        0.044,
        0.012,
    );

    // Fluid gracefully curved vertical stem from (0.55, 0.35) through (0.43, 0.56) to (0.53, 0.79)
    let stem_d = sd_tapered_quad_bezier(
        (u, v),
        (0.55, 0.34),
        (0.43, 0.57),
        (0.53, 0.79),
        0.022,
        0.052,
        0.034,
    );

    let ink_d = bar_d.min(stem_d);
    let ink_cov = ((0.008 - ink_d) / 0.022).clamp(0.0, 1.0);
    if ink_cov > 0.0 {
        // Sculpted deep graphite-obsidian gradient with subtle specular highlight
        let shade = ((u * 0.4 + v * 0.6) * 18.0).clamp(0.0, 18.0);
        let ir = 34.0 - shade;
        let ig = 38.0 - shade;
        let ib = 50.0 - shade;
        r = r * (1.0 - ink_cov) + ir * ink_cov;
        g = g * (1.0 - ink_cov) + ig * ink_cov;
        b = b * (1.0 - ink_cov) + ib * ink_cov;
    }

    // 3. Minimal Iridescent Silver-Ice Accent Dot at (0.65, 0.46)
    let dx = u - 0.65;
    let dy = v - 0.46;
    let dot_r = 0.042;
    let dot_d = (dx * dx + dy * dy).sqrt() - dot_r;
    let dot_cov = ((0.006 - dot_d) / 0.018).clamp(0.0, 1.0);
    if dot_cov > 0.0 {
        // Pearl radial highlight offset toward top-left
        let hl = (((dx + 0.014).powi(2) + (dy + 0.014).powi(2)).sqrt() / (dot_r * 1.4)).clamp(0.0, 1.0);
        let dr = 248.0 - hl * 88.0;
        let dg = 251.0 - hl * 76.0;
        let db = 255.0 - hl * 56.0;
        r = r * (1.0 - dot_cov) + dr * dot_cov;
        g = g * (1.0 - dot_cov) + dg * dot_cov;
        b = b * (1.0 - dot_cov) + db * dot_cov;
    }

    (r, g, b, tile_alpha)
}

/// Evaluates signed distance to a calligraphic tapered quadratic Bezier stroke.
#[inline]
fn sd_tapered_quad_bezier(
    p: (f32, f32),
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    r0: f32,
    r_mid: f32,
    r1: f32,
) -> f32 {
    // 10-segment polyline approximation is exact to <0.2px at icon scales and ultra-fast!
    const STEPS: usize = 10;
    let mut min_d = f32::MAX;
    let mut prev = a;
    let mut prev_r = r0;

    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let omt = 1.0 - t;
        let cur = (
            omt * omt * a.0 + 2.0 * omt * t * b.0 + t * t * c.0,
            omt * omt * a.1 + 2.0 * omt * t * b.1 + t * t * c.1,
        );
        // Smooth calligraphic width envelope (tapered ends, fuller middle belly)
        let cur_r = omt * omt * r0 + 2.0 * omt * t * r_mid + t * t * r1;

        let seg_d = sd_tapered_segment(p, prev, cur, prev_r, cur_r);
        if seg_d < min_d {
            min_d = seg_d;
        }
        prev = cur;
        prev_r = cur_r;
    }
    min_d
}

#[inline]
fn sd_tapered_segment(
    p: (f32, f32),
    a: (f32, f32),
    b: (f32, f32),
    ra: f32,
    rb: f32,
) -> f32 {
    let pax = p.0 - a.0;
    let pay = p.1 - a.1;
    let bax = b.0 - a.0;
    let bay = b.1 - a.1;
    let denom = (bax * bax + bay * bay).max(1e-7);
    let h = ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0);
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    let r = ra + (rb - ra) * h;
    (dx * dx + dy * dy).sqrt() - r
}
