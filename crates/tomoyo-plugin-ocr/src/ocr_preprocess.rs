//! Pure-Rust adaptive upscaling, quiet-zone padding, background-distance high-contrast
//! normalization, thin UI border suppression, and multi-pass scoring for offline WinRT OCR.

/// Returns `(scaled_w, scaled_h, pad_px, needs_transform)` respecting `max_dim` after padding.
pub fn compute_ocr_layout(
    src_w: u32,
    src_h: u32,
    max_dim: u32,
    raw_mode: bool,
) -> (u32, u32, u32, bool) {
    if src_w == 0 || src_h == 0 || max_dim == 0 {
        return (src_w, src_h, 0, false);
    }
    let max_side = src_w.max(src_h);
    let min_side = src_w.min(src_h);
    if max_side > max_dim {
        let scale = max_dim as f64 / max_side as f64;
        return (
            ((src_w as f64 * scale).round() as u32).max(1),
            ((src_h as f64 * scale).round() as u32).max(1),
            0,
            true,
        );
    }
    if raw_mode {
        return (src_w, src_h, 0, false);
    }
    let pad = if min_side <= 360 { 48 } else { 24 };
    let avail = max_dim.saturating_sub(pad * 2).max(256);
    let desired = if min_side <= 96 || max_side <= 440 {
        4.0
    } else if min_side <= 220 || max_side <= 900 {
        3.0
    } else if max_side <= 1600 {
        2.0
    } else {
        1.0
    };
    let scale = (avail as f64 / max_side as f64).min(desired);
    if scale >= 1.15 {
        (
            ((src_w as f64 * scale).round() as u32).min(avail).max(1),
            ((src_h as f64 * scale).round() as u32).min(avail).max(1),
            pad,
            true,
        )
    } else {
        let fit_pad = if max_side + pad * 2 <= max_dim { pad } else { 0 };
        (src_w, src_h, fit_pad, false)
    }
}

/// Estimates dominant canvas background RGB via 4-bit quantized histogram (works on light & dark UI).
pub fn estimate_bg_rgb(bgra: &[u8], w: u32, h: u32) -> (u8, u8, u8) {
    if bgra.len() < 4 || w == 0 || h == 0 {
        return (255, 255, 255);
    }
    let mut counts = vec![0u32; 4096];
    let mut sums = vec![(0u64, 0u64, 0u64); 4096];
    let step_x = (w / 96).max(1) as usize;
    let step_y = (h / 96).max(1) as usize;
    let uw = w as usize;
    let uh = h as usize;
    for y in (0..uh).step_by(step_y) {
        for x in (0..uw).step_by(step_x) {
            let o = (y * uw + x) * 4;
            let (b, g, r) = (bgra[o], bgra[o + 1], bgra[o + 2]);
            let key = (((r as usize) >> 4) << 8) | (((g as usize) >> 4) << 4) | ((b as usize) >> 4);
            counts[key] += 1;
            sums[key].0 += r as u64;
            sums[key].1 += g as u64;
            sums[key].2 += b as u64;
        }
    }
    let best = counts
        .iter()
        .enumerate()
        .max_by_key(|&(_, &c)| c)
        .map(|(idx, _)| idx)
        .unwrap_or(4095);
    let c = counts[best].max(1) as u64;
    ((sums[best].0 / c) as u8, (sums[best].1 / c) as u8, (sums[best].2 / c) as u8)
}

/// Pads a BGRA buffer uniformly on all 4 sides with `bg_rgb` so edge-hugging lines are never clipped.
pub fn pad_bgra(src: &[u8], w: u32, h: u32, pad: u32, bg: (u8, u8, u8)) -> (u32, u32, Vec<u8>) {
    if pad == 0 {
        return (w, h, src.to_vec());
    }
    let nw = w + pad * 2;
    let nh = h + pad * 2;
    let mut out = vec![0u8; (nw as usize) * (nh as usize) * 4];
    for px in out.as_chunks_mut::<4>().0 {
        px[0] = bg.2;
        px[1] = bg.1;
        px[2] = bg.0;
        px[3] = 255;
    }
    let row_bytes = (w as usize) * 4;
    let dst_stride = (nw as usize) * 4;
    let pad_bytes = (pad as usize) * 4;
    for y in 0..(h as usize) {
        let s = y * row_bytes;
        let d = (y + pad as usize) * dst_stride + pad_bytes;
        out[d..d + row_bytes].copy_from_slice(&src[s..s + row_bytes]);
    }
    (nw, nh, out)
}

/// Converts colored or dark-mode UI text into crisp black-on-white via background color distance,
/// removes thin UI button/table border lines, applies mild unsharp masking, and pads with white.
pub fn build_high_contrast_bgra(
    src: &[u8],
    w: u32,
    h: u32,
    pad: u32,
    bg: (u8, u8, u8),
) -> (u32, u32, Vec<u8>) {
    let n = (w as usize) * (h as usize);
    let mut ink = vec![0u8; n];
    let mut hist = [0u32; 256];

    for (i, px) in src.as_chunks::<4>().0.iter().take(n).enumerate() {
        let dr = px[2].abs_diff(bg.0) as u32;
        let dg = px[1].abs_diff(bg.1) as u32;
        let db = px[0].abs_diff(bg.2) as u32;
        let d_max = dr.max(dg).max(db);
        let d_luma = (dr * 3 + dg * 6 + db) / 10;
        let v = ((d_max * 3 + d_luma) / 4).min(255) as u8;
        ink[i] = v;
        hist[v as usize] += 1;
    }

    // Find 98.5th percentile ink intensity so colored/faint text stretches cleanly to jet black.
    let target = ((n as f64) * 0.985) as u32;
    let mut acc = 0u32;
    let mut hi = 180u32;
    for (v, &cnt) in hist.iter().enumerate() {
        acc += cnt;
        if acc >= target {
            hi = (v as u32).max(48);
            break;
        }
    }
    let lo = 12u32;
    let span = hi.saturating_sub(lo).max(1);
    let mut gray = vec![255u8; n];
    for (g, &iv) in gray.iter_mut().zip(ink.iter()) {
        let s = iv.saturating_sub(lo as u8) as u32;
        let stretched = ((s * 255) / span).min(255);
        // Mild gamma boost darkens thin anti-aliased CJK strokes without bloating bold text.
        let norm = stretched as f32 / 255.0;
        let boosted = (norm.powf(0.82) * 255.0).clamp(0.0, 255.0) as u8;
        *g = 255 - boosted;
    }

    suppress_thin_ui_borders(&mut gray, w as usize, h as usize);
    let sharp = unsharp_gray(&gray, w as usize, h as usize);

    let nw = w + pad * 2;
    let nh = h + pad * 2;
    let mut out = vec![255u8; (nw as usize) * (nh as usize) * 4];
    let uw = w as usize;
    let uh = h as usize;
    let unw = nw as usize;
    let upad = pad as usize;
    for y in 0..uh {
        for x in 0..uw {
            let v = sharp[y * uw + x];
            let o = ((y + upad) * unw + (x + upad)) * 4;
            out[o] = v;
            out[o + 1] = v;
            out[o + 2] = v;
            out[o + 3] = 255;
        }
    }
    (nw, nh, out)
}

/// Erases thin continuous horizontal/vertical UI box border lines (e.g. button outlines)
/// spanning > 70% of the crop so adjacent small lines of text aren't rejected by WinRT layout.
fn suppress_thin_ui_borders(gray: &mut [u8], w: usize, h: usize) {
    if w < 32 || h < 20 {
        return;
    }
    let max_bar = ((h.min(w)) / 14).clamp(4, 12);
    let mut dark_rows = vec![false; h];
    for (y, flag) in dark_rows.iter_mut().enumerate() {
        let row = &gray[y * w..(y + 1) * w];
        let dark = row.iter().filter(|&&v| v < 195).count();
        *flag = dark * 100 >= w * 68;
    }
    let mut y = 0;
    while y < h {
        if !dark_rows[y] {
            y += 1;
            continue;
        }
        let start = y;
        while y < h && dark_rows[y] {
            y += 1;
        }
        if y - start <= max_bar {
            for r in start..y {
                gray[r * w..(r + 1) * w].fill(255);
            }
        }
    }

    let mut dark_cols = vec![false; w];
    for (x, flag) in dark_cols.iter_mut().enumerate() {
        let mut dark = 0usize;
        for row in 0..h {
            if gray[row * w + x] < 195 {
                dark += 1;
            }
        }
        *flag = dark * 100 >= h * 65;
    }
    let mut x = 0;
    while x < w {
        if !dark_cols[x] {
            x += 1;
            continue;
        }
        let start = x;
        while x < w && dark_cols[x] {
            x += 1;
        }
        if x - start <= max_bar {
            for row in 0..h {
                for c in start..x {
                    gray[row * w + c] = 255;
                }
            }
        }
    }
}

fn unsharp_gray(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    if w < 3 || h < 3 {
        return src.to_vec();
    }
    let mut out = src.to_vec();
    for y in 1..(h - 1) {
        for x in 1..(w - 1) {
            let idx = y * w + x;
            let c = src[idx] as i32;
            let blur = (src[idx - w - 1] as i32
                + src[idx - w] as i32 * 2
                + src[idx - w + 1] as i32
                + src[idx - 1] as i32 * 2
                + c * 4
                + src[idx + 1] as i32 * 2
                + src[idx + w - 1] as i32
                + src[idx + w] as i32 * 2
                + src[idx + w + 1] as i32)
                / 16;
            let sharpened = c + ((c - blur) * 55) / 100;
            out[idx] = sharpened.clamp(0, 255) as u8;
        }
    }
    out
}

/// Scores an OCR pass so multi-pass selection prefers complete multi-line clean text over garble.
pub fn score_ocr_lines(lines: &[String]) -> i64 {
    let mut score = 0i64;
    for line in lines {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        score += 120;
        for ch in t.chars() {
            if ch.is_ascii_alphanumeric() {
                score += 10;
            } else if matches!(ch, '\u{4E00}'..='\u{9FFF}') {
                score += 12;
            } else if matches!(ch, '_' | '.' | '-' | '(' | ')' | ':' | '/' | '·' | '，' | '。') {
                score += 6;
            } else if matches!(ch, '|' | '~' | '^' | '`' | '\\') {
                score -= 6;
            } else if !ch.is_whitespace() {
                score += 2;
            }
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_tiny_ui_crops_up_to_4x_with_quiet_padding() {
        let (w, h, pad, scaled) = compute_ocr_layout(305, 57, 2600, false);
        assert!(scaled);
        assert_eq!((w, h, pad), (1220, 228, 48));
        assert!(w + pad * 2 <= 2600 && h + pad * 2 <= 2600);
    }

    #[test]
    fn high_contrast_turns_blue_text_dark_and_erases_thin_box_border() {
        let (w, h) = (100u32, 40u32);
        let bg = (247u8, 248u8, 250u8);
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        for px in bgra.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&[bg.2, bg.1, bg.0, 255]);
        }
        // Top 1px blue border across entire width
        for x in 0..w as usize {
            let o = (2 * w as usize + x) * 4;
            bgra[o..o + 4].copy_from_slice(&[235, 99, 37, 255]);
        }
        // Short blue glyph stroke inside
        for x in 40..50 {
            let o = (20 * w as usize + x) * 4;
            bgra[o..o + 4].copy_from_slice(&[235, 99, 37, 255]);
        }
        let est = estimate_bg_rgb(&bgra, w, h);
        assert!(est.0 > 240 && est.1 > 240 && est.2 > 240);
        let (nw, nh, out) = build_high_contrast_bgra(&bgra, w, h, 10, est);
        assert_eq!((nw, nh), (120, 60));
        // Border row at y=2+10 should be whitened
        assert_eq!(out[(12 * nw as usize + 60) * 4], 255);
        // Glyph stroke at y=20+10, x=45+10 should be dark (< 60)
        assert!(out[(30 * nw as usize + 55) * 4] < 60);
    }
}
