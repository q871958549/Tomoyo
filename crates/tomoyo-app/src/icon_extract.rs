//! High-DPI native Windows Shell icon extraction & premultiplied-alpha HQ resampling.
//!
//! Replaces legacy 32x32 `SHGFI_LARGEICON` + nearest-neighbor `DrawIconEx` with:
//! 1. `IShellItemImageFactory::GetImage` at 2x supersampled resolution (`64..=128 px`) for
//!    crisp Hi-DPI `.exe`, `.lnk`, `.ico`, folders, documents, and `shell:AppsFolder\*` UWP icons.
//! 2. `SHGetImageList(SHIL_EXTRALARGE)` (48x48) -> `SHGFI_LARGEICON` (32x32) `HICON` fallback
//!    with proper `hbmColor` + `hbmMask` alpha recovery.
//! 3. Tight bounding-box un-padding (fixes Windows Shell's legacy top-left corner padding quirk).
//! 4. Premultiplied Lanczos-2 / box-area downsampling to the exact physical pixel grid so icons
//!    blit 1:1 on our 32-bit BGRA surface with zero jaggies, zero dark halos, and zero `HICON` leaks.

use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::Controls::IImageList;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SHGetFileInfoW, SHGetImageList,
    SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE,
    SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

pub type DecodedRgba = (u32, u32, Vec<u8>);

struct OwnedHBitmap(HBITMAP);
impl Drop for OwnedHBitmap {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.0.into());
            }
        }
    }
}

struct OwnedHIcon(HICON);
impl Drop for OwnedHIcon {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DestroyIcon(self.0);
            }
        }
    }
}

/// Extracts a razor-sharp straight-alpha RGBA icon resampled to exact `target_px x target_px`.
pub fn extract_shell_icon_rgba(path: &str, target_px: u32) -> Option<DecodedRgba> {
    let dst = target_px.clamp(16, 128);
    let req = (dst * 2).clamp(64, 128) as i32;
    let raw = extract_via_image_factory(path, req)
        .or_else(|| extract_via_shell_image_list(path))
        .or_else(|| extract_via_shgetfileinfo(path))?;

    let (sw, sh, rgba) = unpad_corner_anchored(raw.0, raw.1, raw.2);
    if sw == dst && sh == dst {
        return Some((sw, sh, rgba));
    }
    Some(resample_rgba_premul(&rgba, sw, sh, dst, dst))
}

fn extract_via_image_factory(path: &str, req_px: i32) -> Option<DecodedRgba> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let hbmp = factory
            .GetImage(SIZE { cx: req_px, cy: req_px }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
            .ok()?;
        let _guard = OwnedHBitmap(hbmp);
        let (w, h, mut bgra) = read_bitmap_bgra(hbmp)?;
        let (has_alpha, has_rgb) = scan_alpha_and_rgb(&bgra);
        if !has_alpha {
            if !has_rgb {
                return None;
            }
            if path.starts_with("shell:") {
                for px in bgra.chunks_exact_mut(4) {
                    px[3] = 255;
                }
            } else {
                // Rare legacy 24-bit bitmap with zero alpha channel: defer to HICON mask path.
                return None;
            }
        }
        Some((w, h, bgra_to_straight_rgba(&bgra)))
    }
}

fn extract_via_shell_image_list(path: &str) -> Option<DecodedRgba> {
    if path.starts_with("shell:") {
        return None;
    }
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut shfi = SHFILEINFOW::default();
        let res = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX,
        );
        if res == 0 {
            return None;
        }
        let img_list: IImageList = SHGetImageList(SHIL_EXTRALARGE as i32).ok()?;
        let hicon = img_list.GetIcon(shfi.iIcon, 0).ok()?;
        let _guard = OwnedHIcon(hicon);
        hicon_to_rgba(hicon)
    }
}

fn extract_via_shgetfileinfo(path: &str) -> Option<DecodedRgba> {
    if path.starts_with("shell:") {
        return None;
    }
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut shfi = SHFILEINFOW::default();
        let res = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if res == 0 || shfi.hIcon.is_invalid() {
            return None;
        }
        let _guard = OwnedHIcon(shfi.hIcon);
        hicon_to_rgba(shfi.hIcon)
    }
}

unsafe fn hicon_to_rgba(hicon: HICON) -> Option<DecodedRgba> {
    let mut info = ICONINFO::default();
    if unsafe { GetIconInfo(hicon, &mut info) }.is_err() {
        return None;
    }
    let _color_guard = OwnedHBitmap(info.hbmColor);
    let _mask_guard = OwnedHBitmap(info.hbmMask);
    if info.hbmColor.is_invalid() {
        return None;
    }
    let (w, h, mut bgra) = unsafe { read_bitmap_bgra(info.hbmColor)? };
    let (has_alpha, _) = scan_alpha_and_rgb(&bgra);
    if !has_alpha && !info.hbmMask.is_invalid() {
        if let Some((mw, mh, mask)) = unsafe { read_bitmap_bgra(info.hbmMask) }
            && mw == w
            && mh >= h
        {
            for i in 0..(w as usize * h as usize) {
                let m = mask[i * 4]; // 0 = opaque pixel, 255 = transparent background
                bgra[i * 4 + 3] = if m == 0 { 255 } else { 0 };
            }
        } else {
            for px in bgra.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
    }
    Some((w, h, bgra_to_straight_rgba(&bgra)))
}

unsafe fn read_bitmap_bgra(hbmp: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut bm = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            hbmp.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut c_void),
        )
    };
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight == 0 {
        return None;
    }
    let w = bm.bmWidth as u32;
    let h = bm.bmHeight.unsigned_abs();
    if w > 512 || h > 512 {
        return None;
    }
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        return None;
    }
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32), // top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    let lines = unsafe {
        GetDIBits(
            hdc,
            hbmp,
            0,
            h,
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(hdc);
    }
    (lines > 0).then_some((w, h, buf))
}

fn scan_alpha_and_rgb(bgra: &[u8]) -> (bool, bool) {
    let mut has_alpha = false;
    let mut has_rgb = false;
    for px in bgra.chunks_exact(4) {
        if px[3] != 0 {
            has_alpha = true;
        }
        if px[0] != 0 || px[1] != 0 || px[2] != 0 {
            has_rgb = true;
        }
        if has_alpha && has_rgb {
            break;
        }
    }
    (has_alpha, has_rgb)
}

/// Converts BGRA (either straight or Windows premultiplied ARGB) into clean straight RGBA.
fn bgra_to_straight_rgba(bgra: &[u8]) -> Vec<u8> {
    // Detect premultiplied vs straight: in premultiplied ARGB, R/G/B never exceed A.
    let mut is_premul = true;
    for px in bgra.chunks_exact(4) {
        let a = px[3];
        if px[0] > a || px[1] > a || px[2] > a {
            is_premul = false;
            break;
        }
    }
    let mut out = vec![0u8; bgra.len()];
    for (src, dst) in bgra.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        let a = src[3];
        if a == 0 {
            continue;
        }
        if is_premul && a < 255 {
            let af = a as f32;
            dst[0] = ((src[2] as f32 * 255.0 / af).round()).clamp(0.0, 255.0) as u8;
            dst[1] = ((src[1] as f32 * 255.0 / af).round()).clamp(0.0, 255.0) as u8;
            dst[2] = ((src[0] as f32 * 255.0 / af).round()).clamp(0.0, 255.0) as u8;
            dst[3] = a;
        } else {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = a;
        }
    }
    out
}

/// Fixes Windows Shell quirk where a 32x32/48x48 legacy icon sits inside the top-left corner
/// of a much larger transparent canvas, returning a tight square crop if severely corner-padded.
fn unpad_corner_anchored(w: u32, h: u32, rgba: Vec<u8>) -> DecodedRgba {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, 0u32, 0u32);
    for y in 0..h {
        let row = (y * w) as usize * 4;
        for x in 0..w {
            if rgba[row + (x as usize) * 4 + 3] > 6 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if max_x < min_x || max_y < min_y {
        return (w, h, rgba);
    }
    let bw = max_x - min_x + 1;
    let bh = max_y - min_y + 1;
    let side = bw.max(bh);
    // Only crop if the icon is strongly anchored top-left with >35% dead transparent space on right & bottom.
    if min_x <= 4 && min_y <= 4 && side * 100 <= w.min(h) * 65 && side >= 16 {
        let crop_side = (side + 2).min(w).min(h);
        let mut out = vec![0u8; (crop_side * crop_side * 4) as usize];
        for y in 0..crop_side {
            let src_off = (y * w) as usize * 4;
            let dst_off = (y * crop_side) as usize * 4;
            let len = (crop_side as usize) * 4;
            out[dst_off..dst_off + len].copy_from_slice(&rgba[src_off..src_off + len]);
        }
        return (crop_side, crop_side, out);
    }
    (w, h, rgba)
}

/// High-quality premultiplied-alpha Lanczos-2 / box-area resampler (zero dark edge halos, zero jaggies).
pub fn resample_rgba_premul(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> DecodedRgba {
    let (swf, shf, dwf, dhf) = (sw as f32, sh as f32, dw as f32, dh as f32);
    let scale_x = swf / dwf;
    let scale_y = shf / dhf;
    let radius_x = (2.0 * scale_x.max(1.0)).ceil();
    let radius_y = (2.0 * scale_y.max(1.0)).ceil();
    let inv_sx = 1.0 / scale_x.max(1.0);
    let inv_sy = 1.0 / scale_y.max(1.0);

    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 4];
    for dy in 0..dh {
        let cy = (dy as f32 + 0.5) * scale_y - 0.5;
        let y0 = (cy - radius_y).floor().max(0.0) as i32;
        let y1 = (cy + radius_y).ceil().min(shf - 1.0) as i32;
        for dx in 0..dw {
            let cx = (dx as f32 + 0.5) * scale_x - 0.5;
            let x0 = (cx - radius_x).floor().max(0.0) as i32;
            let x1 = (cx + radius_x).ceil().min(swf - 1.0) as i32;
            let (mut pr, mut pg, mut pb, mut pa, mut wsum) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in y0..=y1 {
                let wy = lanczos2((sy as f32 - cy) * inv_sy);
                if wy == 0.0 {
                    continue;
                }
                let row = (sy as usize) * (sw as usize) * 4;
                for sx in x0..=x1 {
                    let wx = lanczos2((sx as f32 - cx) * inv_sx);
                    let w = wx * wy;
                    if w == 0.0 {
                        continue;
                    }
                    let idx = row + (sx as usize) * 4;
                    let a = src[idx + 3] as f32 / 255.0;
                    pr += src[idx] as f32 * a * w;
                    pg += src[idx + 1] as f32 * a * w;
                    pb += src[idx + 2] as f32 * a * w;
                    pa += a * w;
                    wsum += w;
                }
            }
            let di = ((dy * dw + dx) as usize) * 4;
            if wsum > 1e-6 {
                let a_out = (pa / wsum).clamp(0.0, 1.0);
                if a_out > 1e-4 {
                    let inv_a = 1.0 / (a_out * wsum);
                    out[di] = (pr * inv_a).round().clamp(0.0, 255.0) as u8;
                    out[di + 1] = (pg * inv_a).round().clamp(0.0, 255.0) as u8;
                    out[di + 2] = (pb * inv_a).round().clamp(0.0, 255.0) as u8;
                    out[di + 3] = (a_out * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    (dw, dh, out)
}

#[inline]
fn lanczos2(x: f32) -> f32 {
    let ax = x.abs();
    if ax < 1e-6 {
        return 1.0;
    }
    if ax >= 2.0 {
        return 0.0;
    }
    let pi_x = std::f32::consts::PI * ax;
    let pi_x2 = pi_x * 0.5;
    (pi_x.sin() / pi_x) * (pi_x2.sin() / pi_x2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_lanczos_preserves_clean_edges_without_dark_halo() {
        // White square centered inside transparent 64x64 canvas -> downsampled to 32x32
        let mut src = vec![0u8; 64 * 64 * 4];
        for y in 16..48 {
            for x in 16..48 {
                let i = (y * 64 + x) * 4;
                src[i] = 255;
                src[i + 1] = 255;
                src[i + 2] = 255;
                src[i + 3] = 255;
            }
        }
        let (dw, dh, dst) = resample_rgba_premul(&src, 64, 64, 32, 32);
        assert_eq!((dw, dh), (32, 32));
        // Interior pixel stays pure white opaque
        let mid = (16 * 32 + 16) * 4;
        assert_eq!(&dst[mid..mid + 4], &[255, 255, 255, 255]);
        // Semi-transparent edge pixels must stay pure white (RGB >= 240), never darkened toward black!
        for px in dst.chunks_exact(4) {
            if px[3] > 20 {
                assert!(px[0] >= 240 && px[1] >= 240 && px[2] >= 240, "dark halo detected: {px:?}");
            }
        }
    }

    #[test]
    fn unpads_top_left_corner_anchored_legacy_icon() {
        let mut src = vec![0u8; 128 * 128 * 4];
        for y in 0..32 {
            for x in 0..32 {
                let i = (y * 128 + x) * 4;
                src[i] = 40;
                src[i + 1] = 120;
                src[i + 2] = 240;
                src[i + 3] = 255;
            }
        }
        let (cw, ch, cropped) = unpad_corner_anchored(128, 128, src);
        assert_eq!(cw, 34);
        assert_eq!(ch, 34);
        assert_eq!(cropped.len(), 34 * 34 * 4);
    }
}
