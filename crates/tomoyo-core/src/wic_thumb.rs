//! Zero-dependency Windows Imaging Component (WIC) bounded RGBA thumbnail decoder.
//! Supports PNG, JPEG, WebP, BMP, GIF, ICO, and TIFF on Windows without extra image crates.

use std::path::Path;

/// Returns `(src_w, src_h, dst_w, dst_h, rgba_bytes)` scaled to fit within `max_w x max_h`.
#[cfg(windows)]
pub fn decode_image_thumbnail_wic(
    path: &Path,
    max_w: u32,
    max_h: u32,
) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    if let Some(png_res) = decode_png_thumbnail(path, max_w, max_h) {
        return Some(png_res);
    }
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory,
        WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
        WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::Foundation::GENERIC_READ;

    let meta = std::fs::metadata(path).ok()?;
    if meta.len() == 0 || meta.len() > 32 * 1024 * 1024 {
        return None;
    }

    struct ComScope(bool);
    impl Drop for ComScope {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _com = ComScope(hr.is_ok());

        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let path_w = HSTRING::from(path.to_string_lossy().as_ref());
        let decoder = factory
            .CreateDecoderFromFilename(
                &path_w,
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;

        let mut src_w = 0u32;
        let mut src_h = 0u32;
        frame.GetSize(&mut src_w, &mut src_h).ok()?;
        if src_w == 0 || src_h == 0 || src_w > 12_000 || src_h > 12_000 {
            return None;
        }

        let scale = ((max_w as f32 / src_w as f32).min(max_h as f32 / src_h as f32)).min(1.0);
        let dst_w = ((src_w as f32 * scale).round() as u32).max(1);
        let dst_h = ((src_h as f32 * scale).round() as u32).max(1);

        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&frame, dst_w, dst_h, WICBitmapInterpolationModeFant)
            .ok()?;

        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;

        let stride = dst_w.checked_mul(4)?;
        let buf_len = (stride as usize).checked_mul(dst_h as usize)?;
        let mut rgba = vec![0u8; buf_len];
        converter.CopyPixels(std::ptr::null(), stride, &mut rgba).ok()?;
        Some((src_w, src_h, dst_w, dst_h, rgba))
    }
}

#[cfg(target_os = "linux")]
pub fn decode_image_thumbnail_wic(
    path: &Path,
    max_w: u32,
    max_h: u32,
) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    if let Some(png_res) = decode_png_thumbnail(path, max_w, max_h) {
        return Some(png_res);
    }
    crate::linux_sys::decode_image_rgba(path, max_w, max_h)
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn decode_image_thumbnail_wic(
    _path: &Path,
    _max_w: u32,
    _max_h: u32,
) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    None
}

const MAX_PNG_FILE_BYTES: u64 = 32 * 1024 * 1024;
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

fn has_png_magic(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut hdr = [0u8; 8];
    f.read_exact(&mut hdr).is_ok() && &hdr == PNG_MAGIC
}

fn to_rgba_pixels(raw: &[u8], color: png::ColorType, px_count: usize) -> Option<Vec<u8>> {
    match color {
        png::ColorType::Rgba => (raw.len() >= px_count * 4).then(|| raw[..px_count * 4].to_vec()),
        png::ColorType::Rgb => (raw.len() >= px_count * 3).then(|| {
            let mut out = Vec::with_capacity(px_count * 4);
            for &[r, g, b] in raw[..px_count * 3].as_chunks::<3>().0 {
                out.extend_from_slice(&[r, g, b, 255]);
            }
            out
        }),
        png::ColorType::GrayscaleAlpha => (raw.len() >= px_count * 2).then(|| {
            let mut out = Vec::with_capacity(px_count * 4);
            for &[v, a] in raw[..px_count * 2].as_chunks::<2>().0 {
                out.extend_from_slice(&[v, v, v, a]);
            }
            out
        }),
        png::ColorType::Grayscale => (raw.len() >= px_count).then(|| {
            let mut out = Vec::with_capacity(px_count * 4);
            for &v in &raw[..px_count] {
                out.extend_from_slice(&[v, v, v, 255]);
            }
            out
        }),
        _ => None,
    }
}

fn read_png_rgba_file(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() == 0 || meta.len() > MAX_PNG_FILE_BYTES || !has_png_magic(path) {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    decode_png_rgba_bytes(&bytes)
}

/// Pure-Rust in-process PNG decoder returning `(width, height, rgba_bytes)` without `gdk-pixbuf` / `glycin`.
pub fn decode_png_rgba_bytes(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if bytes.len() < 8 || bytes.len() as u64 > MAX_PNG_FILE_BYTES || &bytes[..8] != PNG_MAGIC {
        return None;
    }
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let (w, h) = (reader.info().width, reader.info().height);
    let px_count = (w as usize).checked_mul(h as usize)?;
    if w == 0 || h == 0 || w > 12_000 || h > 12_000 || px_count > 64_000_000 {
        return None;
    }
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let rgba = to_rgba_pixels(&buf[..frame.buffer_size()], frame.color_type, px_count)?;
    Some((w, h, rgba))
}

/// Bilinearly resamples an RGBA8 buffer from `(sw, sh)` to `(dw, dh)`.
pub fn scale_rgba_bilinear(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    if sw == dw && sh == dh && src.len() >= (sw as usize) * (sh as usize) * 4 {
        return src[..(sw as usize) * (sh as usize) * 4].to_vec();
    }
    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 4];
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return out;
    }
    for y in 0..dh {
        let fy = if dh > 1 { (y as f32) * ((sh - 1) as f32) / ((dh - 1) as f32) } else { 0.0 };
        let y0 = (fy.floor() as u32).min(sh - 1);
        let y1 = (y0 + 1).min(sh - 1);
        let wy = fy - (y0 as f32);
        for x in 0..dw {
            let fx = if dw > 1 { (x as f32) * ((sw - 1) as f32) / ((dw - 1) as f32) } else { 0.0 };
            let x0 = (fx.floor() as u32).min(sw - 1);
            let x1 = (x0 + 1).min(sw - 1);
            let wx = fx - (x0 as f32);
            let i00 = ((y0 * sw + x0) * 4) as usize;
            let i10 = ((y0 * sw + x1) * 4) as usize;
            let i01 = ((y1 * sw + x0) * 4) as usize;
            let i11 = ((y1 * sw + x1) * 4) as usize;
            let di = ((y * dw + x) * 4) as usize;
            for c in 0..4 {
                let top = src[i00 + c] as f32 * (1.0 - wx) + src[i10 + c] as f32 * wx;
                let bot = src[i01 + c] as f32 * (1.0 - wx) + src[i11 + c] as f32 * wx;
                out[di + c] = (top * (1.0 - wy) + bot * wy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

fn flush_acc_row(acc: &mut [[u32; 5]], out_row: &mut [u8]) {
    for (cell, px) in acc.iter_mut().zip(out_row.chunks_exact_mut(4)) {
        let cnt = cell[4];
        if cnt > 0 {
            let half = cnt / 2;
            for c in 0..4 {
                px[c] = ((cell[c] + half) / cnt).min(255) as u8;
            }
        }
        *cell = [0; 5];
    }
}

fn accumulate_row(row: &[u8], color: png::ColorType, sw: u32, dw: u32, acc: &mut [[u32; 5]]) -> bool {
    let step = match color {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => return false,
    };
    if row.len() < (sw as usize).saturating_mul(step) {
        return false;
    }
    for sx in 0..sw {
        let dx = (((sx as u64) * (dw as u64)) / (sw as u64)) as usize;
        let off = (sx as usize) * step;
        let (r, g, b, a) = match color {
            png::ColorType::Rgba => (row[off], row[off + 1], row[off + 2], row[off + 3]),
            png::ColorType::Rgb => (row[off], row[off + 1], row[off + 2], 255),
            png::ColorType::GrayscaleAlpha => (row[off], row[off], row[off], row[off + 1]),
            png::ColorType::Grayscale => (row[off], row[off], row[off], 255),
            _ => unreachable!(),
        };
        let cell = &mut acc[dx];
        cell[0] += r as u32;
        cell[1] += g as u32;
        cell[2] += b as u32;
        cell[3] += a as u32;
        cell[4] += 1;
    }
    true
}

/// Pure-Rust streaming PNG thumbnail decoder bounded to `max_w x max_h` with O(width) scanline RAM.
pub fn decode_png_thumbnail(
    path: &Path,
    max_w: u32,
    max_h: u32,
) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() == 0 || meta.len() > MAX_PNG_FILE_BYTES || !has_png_magic(path) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let (sw, sh) = (reader.info().width, reader.info().height);
    let color = reader.info().color_type;
    let interlaced = reader.info().interlaced;
    let px_count = (sw as usize).checked_mul(sh as usize)?;
    if sw == 0 || sh == 0 || sw > 12_000 || sh > 12_000 || px_count > 64_000_000 {
        return None;
    }
    let scale = ((max_w as f32 / sw as f32).min(max_h as f32 / sh as f32)).min(1.0);
    let dw = ((sw as f32 * scale).round() as u32).max(1).min(sw);
    let dh = ((sh as f32 * scale).round() as u32).max(1).min(sh);
    if interlaced {
        drop(reader);
        let (_, _, rgba) = read_png_rgba_file(path)?;
        return Some((sw, sh, dw, dh, scale_rgba_bilinear(&rgba, sw, sh, dw, dh)));
    }
    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 4];
    let mut row_acc = vec![[0u32; 5]; dw as usize];
    let mut cur_dy = 0usize;
    let row_stride = (dw as usize) * 4;
    for sy in 0..sh {
        let row = reader.next_row().ok()??;
        let dy = (((sy as u64) * (dh as u64)) / (sh as u64)) as usize;
        if dy != cur_dy {
            flush_acc_row(&mut row_acc, &mut out[cur_dy * row_stride..(cur_dy + 1) * row_stride]);
            cur_dy = dy;
        }
        if !accumulate_row(row.data(), color, sw, dw, &mut row_acc) {
            return None;
        }
    }
    flush_acc_row(&mut row_acc, &mut out[cur_dy * row_stride..(cur_dy + 1) * row_stride]);
    Some((sw, sh, dw, dh, out))
}

/// Decodes a PNG file into a `sz x sz` premultiplied BGRA buffer suitable for Cairo blitting.
pub fn decode_png_file_bgra_square(path: &Path, sz: u32) -> Option<Vec<u8>> {
    if sz == 0 || sz > 512 {
        return None;
    }
    let (sw, sh, rgba) = read_png_rgba_file(path)?;
    let scaled = scale_rgba_bilinear(&rgba, sw, sh, sz, sz);
    let mut bgra = vec![0u8; (sz as usize) * (sz as usize) * 4];
    for (&[r, g, b, alpha], d) in scaled
        .as_chunks::<4>()
        .0
        .iter()
        .zip(bgra.as_chunks_mut::<4>().0.iter_mut())
    {
        let a = alpha as u16;
        d[0] = ((b as u16 * a + 127) / 255) as u8;
        d[1] = ((g as u16 * a + 127) / 255) as u8;
        d[2] = ((r as u16 * a + 127) / 255) as u8;
        d[3] = alpha;
    }
    Some(bgra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_and_scales_png_in_pure_rust() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 4, 4);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().unwrap();
            let px = [255u8, 0, 0, 128].repeat(16);
            wr.write_image_data(&px).unwrap();
        }
        let (w, h, rgba) = decode_png_rgba_bytes(&png_bytes).unwrap();
        assert_eq!((w, h), (4, 4));
        assert_eq!(rgba.len(), 64);
        let scaled = scale_rgba_bilinear(&rgba, w, h, 2, 2);
        assert_eq!(scaled.len(), 16);
        assert_eq!(&scaled[..4], &[255, 0, 0, 128]);
    }

    #[test]
    fn streaming_png_thumbnail_downsamples_with_bounded_memory() {
        let tmp = std::env::temp_dir().join("tomoyo_stream_thumb_test.png");
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 16, 12);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().unwrap();
            let px = [40u8, 120, 200, 255].repeat(16 * 12);
            wr.write_image_data(&px).unwrap();
        }
        std::fs::write(&tmp, &png_bytes).unwrap();
        let res = decode_png_thumbnail(&tmp, 4, 3);
        let _ = std::fs::remove_file(&tmp);
        let (sw, sh, dw, dh, out) = res.unwrap();
        assert_eq!((sw, sh, dw, dh), (16, 12, 4, 3));
        assert_eq!(out.len(), 48);
        assert_eq!(&out[..4], &[40, 120, 200, 255]);
    }
}
