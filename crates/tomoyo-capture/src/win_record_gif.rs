//! Zero-dependency pure-Rust GIF89a animated encoder for Windows screen recording.
//!
//! Uses a 255-color Median-Cut adaptive palette (with index 255 reserved as transparent delta
//! mask), 5-bit RGB lookup table for O(1) per-pixel quantization, dirty-rectangle bounding-box
//! cropping, static frame coalescing, and standard variable-width 9..12-bit GIF LZW compression.

#![cfg(windows)]

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

const TRANSPARENT_IDX: u8 = 255;

pub struct GifEncoder {
    path: PathBuf,
    width: u16,
    height: u16,
    out: BufWriter<File>,
    header_written: bool,
    lut: Vec<u8>,
    prev_idx: Vec<u8>,
    pending_frame: Option<PendingFrame>,
    bytes_written: u64,
}

struct PendingFrame {
    left: u16,
    top: u16,
    w: u16,
    h: u16,
    indices: Vec<u8>,
    transparent: bool,
    delay_cs: u16,
}

impl GifEncoder {
    pub fn create(path: &Path, width: u32, height: u32) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let w = u16::try_from(width.clamp(2, 4096)).map_err(|_| "GIF width too large")?;
        let h = u16::try_from(height.clamp(2, 4096)).map_err(|_| "GIF height too large")?;
        let file = File::create(path).map_err(|e| format!("创建 GIF 文件失败: {e}"))?;
        Ok(Self {
            path: path.to_path_buf(),
            width: w,
            height: h,
            out: BufWriter::with_capacity(64 * 1024, file),
            header_written: false,
            lut: Vec::new(),
            prev_idx: Vec::new(),
            pending_frame: None,
            bytes_written: 0,
        })
    }

    pub fn push_bgra_frame(&mut self, bgra: &[u8], delay_cs: u16) -> Result<(), String> {
        let w = self.width as usize;
        let h = self.height as usize;
        let delay = delay_cs.max(2);
        if bgra.len() < w * h * 4 {
            return Err("GIF frame buffer too small".into());
        }

        if !self.header_written {
            let (palette, lut) = build_median_cut_palette(bgra);
            self.write_header(&palette)?;
            self.lut = lut;
            self.header_written = true;
            let mut cur = vec![0u8; w * h];
            map_bgra_to_indices(bgra, &self.lut, &mut cur);
            self.prev_idx = cur.clone();
            self.pending_frame = Some(PendingFrame {
                left: 0,
                top: 0,
                w: self.width,
                h: self.height,
                indices: cur,
                transparent: false,
                delay_cs: delay,
            });
            return Ok(());
        }

        let mut cur = vec![0u8; w * h];
        map_bgra_to_indices(bgra, &self.lut, &mut cur);

        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        for y in 0..h {
            let row = y * w;
            for x in 0..w {
                if cur[row + x] != self.prev_idx[row + x] {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }

        // Identical frame: coalesce delay into pending frame instead of emitting redundant bytes.
        if min_x > max_x {
            if let Some(pf) = self.pending_frame.as_mut() {
                pf.delay_cs = pf.delay_cs.saturating_add(delay).min(6000);
            }
            return Ok(());
        }

        self.flush_pending()?;

        let dw = max_x - min_x + 1;
        let dh = max_y - min_y + 1;
        let mut sub = Vec::with_capacity(dw * dh);
        for y in min_y..=max_y {
            let row = y * w;
            for x in min_x..=max_x {
                let c = cur[row + x];
                sub.push(if c == self.prev_idx[row + x] { TRANSPARENT_IDX } else { c });
            }
        }
        self.prev_idx = cur;
        self.pending_frame = Some(PendingFrame {
            left: min_x as u16,
            top: min_y as u16,
            w: dw as u16,
            h: dh as u16,
            indices: sub,
            transparent: true,
            delay_cs: delay,
        });
        Ok(())
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    pub fn finish(mut self) -> Result<u64, String> {
        if !self.header_written {
            return Err("未捕获到有效 GIF 帧".into());
        }
        self.flush_pending()?;
        self.write_raw(&[0x3B])?; // GIF trailer
        self.out.flush().map_err(|e| format!("写入 GIF 失败: {e}"))?;
        drop(self.out);
        let sz = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(self.bytes_written);
        Ok(sz)
    }

    fn write_raw(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.out.write_all(bytes).map_err(|e| format!("写入 GIF 失败: {e}"))?;
        self.bytes_written += bytes.len() as u64;
        Ok(())
    }

    fn write_header(&mut self, palette: &[[u8; 3]; 256]) -> Result<(), String> {
        let mut hdr = Vec::with_capacity(13 + 768 + 19);
        hdr.extend_from_slice(b"GIF89a");
        hdr.extend_from_slice(&self.width.to_le_bytes());
        hdr.extend_from_slice(&self.height.to_le_bytes());
        hdr.push(0xF7); // Global color table flag + 8-bit color + 256 entries
        hdr.push(TRANSPARENT_IDX);
        hdr.push(0);
        for rgb in palette {
            hdr.extend_from_slice(rgb);
        }
        // NETSCAPE2.0 infinite loop extension
        hdr.extend_from_slice(&[
            0x21, 0xFF, 0x0B, b'N', b'E', b'T', b'S', b'C', b'A', b'P', b'E', b'2', b'.', b'0',
            0x03, 0x01, 0x00, 0x00, 0x00,
        ]);
        self.write_raw(&hdr)
    }

    fn flush_pending(&mut self) -> Result<(), String> {
        let Some(pf) = self.pending_frame.take() else { return Ok(()); };
        let packed = if pf.transparent { 0x05 } else { 0x04 }; // disposal=1 (do not dispose)
        let mut gce = [0x21, 0xF9, 0x04, packed, 0, 0, TRANSPARENT_IDX, 0x00];
        let d = pf.delay_cs.to_le_bytes();
        gce[4] = d[0];
        gce[5] = d[1];
        self.write_raw(&gce)?;

        let mut desc = [0u8; 10];
        desc[0] = 0x2C;
        desc[1..3].copy_from_slice(&pf.left.to_le_bytes());
        desc[3..5].copy_from_slice(&pf.top.to_le_bytes());
        desc[5..7].copy_from_slice(&pf.w.to_le_bytes());
        desc[7..9].copy_from_slice(&pf.h.to_le_bytes());
        desc[9] = 0x00;
        self.write_raw(&desc)?;

        let lzw = lzw_encode_8bit(&pf.indices);
        self.write_raw(&[8])?; // LZW minimum code size = 8
        for chunk in lzw.chunks(255) {
            self.write_raw(&[chunk.len() as u8])?;
            self.write_raw(chunk)?;
        }
        self.write_raw(&[0x00])
    }
}

fn map_bgra_to_indices(bgra: &[u8], lut: &[u8], out: &mut [u8]) {
    for (px, dst) in bgra.as_chunks::<4>().0.iter().zip(out.iter_mut()) {
        let k = ((px[2] as usize >> 3) << 10) | ((px[1] as usize >> 3) << 5) | (px[0] as usize >> 3);
        *dst = lut[k];
    }
}

fn build_median_cut_palette(bgra: &[u8]) -> ([[u8; 3]; 256], Vec<u8>) {
    let mut hist = vec![0u32; 32 * 32 * 32];
    let step = (bgra.len() / 4 / 160_000).max(1);
    for px in bgra.as_chunks::<4>().0.iter().step_by(step) {
        let k = ((px[2] as usize >> 3) << 10) | ((px[1] as usize >> 3) << 5) | (px[0] as usize >> 3);
        hist[k] = hist[k].saturating_add(1);
    }

    let mut entries: Vec<(u8, u8, u8, u32)> = Vec::with_capacity(4096);
    for (k, &cnt) in hist.iter().enumerate() {
        if cnt > 0 {
            entries.push((((k >> 10) & 31) as u8, ((k >> 5) & 31) as u8, (k & 31) as u8, cnt));
        }
    }
    if entries.is_empty() {
        entries.push((0, 0, 0, 1));
    }

    let mut boxes: Vec<(usize, usize)> = vec![(0, entries.len())];
    while boxes.len() < 255 {
        let mut best_i = None;
        let mut best_score = 0u64;
        for (i, &(lo, hi)) in boxes.iter().enumerate() {
            if hi - lo <= 1 { continue; }
            let (dr, dg, db, pop) = box_span(&entries[lo..hi]);
            let span = dr.max(dg).max(db) as u64;
            let score = span * (pop.max(1) as u64);
            if score > best_score {
                best_score = score;
                best_i = Some((i, dr, dg, db));
            }
        }
        let Some((idx, dr, dg, db)) = best_i else { break; };
        let (lo, hi) = boxes.swap_remove(idx);
        let slice = &mut entries[lo..hi];
        if dr >= dg && dr >= db {
            slice.sort_unstable_by_key(|e| e.0);
        } else if dg >= db {
            slice.sort_unstable_by_key(|e| e.1);
        } else {
            slice.sort_unstable_by_key(|e| e.2);
        }
        let total: u64 = slice.iter().map(|e| e.3 as u64).sum();
        let mut acc = 0u64;
        let mut mid = 1usize;
        for (j, e) in slice.iter().enumerate() {
            acc += e.3 as u64;
            if acc * 2 >= total {
                mid = (j + 1).clamp(1, slice.len() - 1);
                break;
            }
        }
        boxes.push((lo, lo + mid));
        boxes.push((lo + mid, hi));
    }

    let mut palette = [[0u8; 3]; 256];
    let mut lut = vec![0u8; 32 * 32 * 32];
    let used = boxes.len().min(255);
    for (ci, &(lo, hi)) in boxes.iter().take(used).enumerate() {
        let (mut sr, mut sg, mut sb, mut sw) = (0u64, 0u64, 0u64, 0u64);
        for &(r, g, b, w) in &entries[lo..hi] {
            let w64 = w as u64;
            sr += (r as u64 * 8 + 4) * w64;
            sg += (g as u64 * 8 + 4) * w64;
            sb += (b as u64 * 8 + 4) * w64;
            sw += w64;
        }
        let sw = sw.max(1);
        palette[ci] = [((sr / sw).min(255)) as u8, ((sg / sw).min(255)) as u8, ((sb / sw).min(255)) as u8];
        for &(r, g, b, _) in &entries[lo..hi] {
            let k = ((r as usize) << 10) | ((g as usize) << 5) | (b as usize);
            lut[k] = ci as u8;
        }
    }
    // Fill unvisited 5-bit RGB cells by nearest palette entry so later frames map accurately.
    for (k, cell) in lut.iter_mut().enumerate() {
        if hist[k] > 0 { continue; }
        let (r, g, b) = ((((k >> 10) & 31) * 8 + 4) as i32, (((k >> 5) & 31) * 8 + 4) as i32, ((k & 31) * 8 + 4) as i32);
        let mut best_c = 0u8;
        let mut best_d = i32::MAX;
        for (ci, p) in palette.iter().take(used).enumerate() {
            let d = (r - p[0] as i32).pow(2) + (g - p[1] as i32).pow(2) + (b - p[2] as i32).pow(2);
            if d < best_d { best_d = d; best_c = ci as u8; }
        }
        *cell = best_c;
    }
    (palette, lut)
}

fn box_span(slice: &[(u8, u8, u8, u32)]) -> (u8, u8, u8, u32) {
    let (mut r0, mut r1) = (31u8, 0u8);
    let (mut g0, mut g1) = (31u8, 0u8);
    let (mut b0, mut b1) = (31u8, 0u8);
    let mut pop = 0u32;
    for &(r, g, b, w) in slice {
        r0 = r0.min(r); r1 = r1.max(r);
        g0 = g0.min(g); g1 = g1.max(g);
        b0 = b0.min(b); b1 = b1.max(b);
        pop = pop.saturating_add(w);
    }
    (r1 - r0, g1 - g0, b1 - b0, pop)
}

fn lzw_encode_8bit(data: &[u8]) -> Vec<u8> {
    const CLEAR: u16 = 256;
    const EOI: u16 = 257;
    const HASH_SIZE: usize = 8192;
    let mut keys = [u32::MAX; HASH_SIZE];
    let mut vals = [0u16; HASH_SIZE];
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    let mut bit_buf = 0u32;
    let mut bit_cnt = 0u32;
    let mut emit = |code: u16, bits: u32, dst: &mut Vec<u8>| {
        bit_buf |= (code as u32) << bit_cnt;
        bit_cnt += bits;
        while bit_cnt >= 8 {
            dst.push((bit_buf & 0xFF) as u8);
            bit_buf >>= 8;
            bit_cnt -= 8;
        }
    };

    let mut code_bits = 9u32;
    let mut next_code = 258u16;
    emit(CLEAR, code_bits, &mut out);
    if data.is_empty() {
        emit(EOI, code_bits, &mut out);
        if bit_cnt > 0 { out.push(bit_buf as u8); }
        return out;
    }

    let mut prefix = data[0] as u16;
    for &byte in &data[1..] {
        let key = ((prefix as u32) << 8) | (byte as u32);
        let mut h = ((key.wrapping_mul(2654435761)) >> 19) as usize & (HASH_SIZE - 1);
        let mut found = None;
        loop {
            let k = keys[h];
            if k == key { found = Some(vals[h]); break; }
            if k == u32::MAX { break; }
            h = (h + 1) & (HASH_SIZE - 1);
        }
        if let Some(code) = found {
            prefix = code;
        } else {
            emit(prefix, code_bits, &mut out);
            if next_code < 4096 {
                keys[h] = key;
                vals[h] = next_code;
                next_code += 1;
                if next_code > (1u16 << code_bits) && code_bits < 12 {
                    code_bits += 1;
                }
            } else {
                emit(CLEAR, code_bits, &mut out);
                keys.fill(u32::MAX);
                next_code = 258;
                code_bits = 9;
            }
            prefix = byte as u16;
        }
    }
    emit(prefix, code_bits, &mut out);
    emit(EOI, code_bits, &mut out);
    if bit_cnt > 0 { out.push(bit_buf as u8); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_multiframe_gif89a_with_dirty_rect_and_dedup() {
        let path = std::env::temp_dir().join(format!("tomoyo_gif_test_{}.gif", std::process::id()));
        let mut enc = GifEncoder::create(&path, 32, 24).expect("create gif");
        let mut frame = vec![0u8; 32 * 24 * 4];
        for px in frame.as_chunks_mut::<4>().0 { px.copy_from_slice(&[240, 120, 40, 255]); }
        enc.push_bgra_frame(&frame, 5).expect("frame 0");
        enc.push_bgra_frame(&frame, 5).expect("duplicate frame coalesced");
        // Modify a small 4x4 region in the middle
        for y in 8..12 {
            for x in 10..14 {
                let off = (y * 32 + x) * 4;
                frame[off..off + 4].copy_from_slice(&[20, 220, 250, 255]);
            }
        }
        enc.push_bgra_frame(&frame, 5).expect("delta frame");
        let sz = enc.finish().expect("finish gif");
        let bytes = std::fs::read(&path).expect("read gif");
        let _ = std::fs::remove_file(&path);
        assert_eq!(sz as usize, bytes.len());
        assert!(bytes.starts_with(b"GIF89a"));
        assert_eq!(*bytes.last().unwrap(), 0x3B);
        assert!(bytes.len() > 800);
    }
}
