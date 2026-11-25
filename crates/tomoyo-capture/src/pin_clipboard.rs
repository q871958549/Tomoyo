//! Universal "Pin Latest Copied Information" (`F3` / `--pin-clipboard`) clipboard ingestion:
//! supports registered `"PNG"`, `CF_DIB`/`CF_DIBV5`, `CF_HDROP` image/file copies,
//! pure CSS color swatch cards, and ClearType text/code sticky cards.

#![cfg(windows)]

use crate::export::{load_png_as_bgra, read_clipboard_image_bgra};
use crate::pin_card::render_text_or_color_pin_card;
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows::core::w;
use windows::Win32::Foundation::{HGLOBAL, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

pub struct PinPayload {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub source_text: Option<String>,
}

/// Reads the latest clipboard payload in priority order:
/// 1. Registered `"PNG"` or `CF_DIB` / `CF_DIBV5` bitmap image
/// 2. `CF_HDROP` copied image file (or non-image file path list rendered as a sticky card)
/// 3. `CF_UNICODETEXT` rendered as a Color Swatch card (`#HEX` / `rgb()`) or ClearType Text/Code card
pub fn read_latest_clipboard_for_pin(theme: &str) -> Option<PinPayload> {
    if let Some((w, h, bgra)) = read_clipboard_png_bgra().or_else(read_clipboard_image_bgra) {
        return Some(PinPayload { width: w, height: h, bgra, source_text: None });
    }
    if let Some(files) = read_clipboard_file_paths() {
        for p in &files {
            if is_supported_image_file(p)
                && let Some((w, h, bgra)) = load_any_image_bgra(p)
            {
                return Some(PinPayload { width: w, height: h, bgra, source_text: None });
            }
        }
        let joined = files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        if !joined.trim().is_empty() {
            return render_text_or_color_pin_card(&joined, theme, active_cursor_dpi_scale());
        }
    }
    if let Some(text) = read_clipboard_unicode_text() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return render_text_or_color_pin_card(&text, theme, active_cursor_dpi_scale());
        }
    }
    None
}

unsafe fn open_clipboard_with_retry() -> bool {
    for attempt in 0..6 {
        if unsafe { OpenClipboard(None).is_ok() } {
            return true;
        }
        if attempt < 5 {
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    false
}

pub(crate) fn active_cursor_dpi_scale() -> f32 {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 {
            return (dx as f32 / 96.0).clamp(1.0, 3.0);
        }
    }
    1.0
}

fn read_clipboard_png_bgra() -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let fmt = RegisterClipboardFormatW(w!("PNG"));
        if fmt == 0 || IsClipboardFormatAvailable(fmt).is_err() || !open_clipboard_with_retry() {
            return None;
        }
        let bytes = (|| -> Option<Vec<u8>> {
            let h = GetClipboardData(fmt).ok()?;
            let hg = HGLOBAL(h.0);
            let len = GlobalSize(hg);
            if !(24..=48 * 1024 * 1024).contains(&len) {
                return None;
            }
            let ptr = GlobalLock(hg) as *const u8;
            if ptr.is_null() {
                return None;
            }
            let out = std::slice::from_raw_parts(ptr, len).to_vec();
            let _ = GlobalUnlock(hg);
            Some(out)
        })();
        let _ = CloseClipboard();
        let raw = bytes?;
        decode_png_bytes_to_bgra(&raw)
    }
}

fn decode_png_bytes_to_bgra(raw: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(raw));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return None;
    }
    let slice = &buf[..info.buffer_size()];
    let mut bgra = Vec::with_capacity((w as usize) * (h as usize) * 4);
    match info.color_type {
        png::ColorType::Rgba => {
            for px in slice.as_chunks::<4>().0 {
                let a = px[3] as u32;
                let inv = 255 - a;
                let r = ((px[0] as u32 * a + 255 * inv) / 255) as u8;
                let g = ((px[1] as u32 * a + 255 * inv) / 255) as u8;
                let b = ((px[2] as u32 * a + 255 * inv) / 255) as u8;
                bgra.extend_from_slice(&[b, g, r, 255]);
            }
        }
        png::ColorType::Rgb => {
            for px in slice.as_chunks::<3>().0 {
                bgra.extend_from_slice(&[px[2], px[1], px[0], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in slice.as_chunks::<2>().0 {
                let a = px[1] as u32;
                let g = ((px[0] as u32 * a + 255 * (255 - a)) / 255) as u8;
                bgra.extend_from_slice(&[g, g, g, 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in slice {
                bgra.extend_from_slice(&[g, g, g, 255]);
            }
        }
        _ => return None,
    }
    Some((w, h, bgra))
}

fn read_clipboard_file_paths() -> Option<Vec<PathBuf>> {
    unsafe {
        if IsClipboardFormatAvailable(15).is_err() || !open_clipboard_with_retry() {
            return None;
        }
        let out = (|| -> Option<Vec<PathBuf>> {
            let h = GetClipboardData(15).ok()?; // CF_HDROP = 15
            let hdrop = HDROP(h.0);
            let count = DragQueryFileW(hdrop, u32::MAX, None).min(16);
            if count == 0 {
                return None;
            }
            let mut paths = Vec::with_capacity(count as usize);
            for idx in 0..count {
                let len = DragQueryFileW(hdrop, idx, None) as usize;
                if len == 0 || len > 1024 {
                    continue;
                }
                let mut buf = vec![0u16; len + 1];
                let got = DragQueryFileW(hdrop, idx, Some(&mut buf)) as usize;
                if got > 0 {
                    paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..got])));
                }
            }
            (!paths.is_empty()).then_some(paths)
        })();
        let _ = CloseClipboard();
        out
    }
}

fn read_clipboard_unicode_text() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(13).is_err() || !open_clipboard_with_retry() {
            return None;
        }
        let out = (|| -> Option<String> {
            let h = GetClipboardData(13).ok()?; // CF_UNICODETEXT = 13
            let hg = HGLOBAL(h.0);
            let bytes = GlobalSize(hg);
            if !(2..=2 * 1024 * 1024).contains(&bytes) {
                return None;
            }
            let ptr = GlobalLock(hg) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let max_u16 = (bytes / 2).min(16_384);
            let raw = std::slice::from_raw_parts(ptr, max_u16);
            let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            let s = String::from_utf16_lossy(&raw[..end]);
            let _ = GlobalUnlock(hg);
            Some(s)
        })();
        let _ = CloseClipboard();
        out
    }
}

fn is_supported_image_file(p: &Path) -> bool {
    let Some(ext) = p.extension().and_then(|e| e.to_str()) else { return false };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "ico" | "tif" | "tiff"
    )
}

fn load_any_image_bgra(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    load_png_as_bgra(path).ok().or_else(|| {
        let (_, _, w, h, mut rgba) =
            tomoyo_core::wic_thumb::decode_image_thumbnail_wic(path, 3840, 2160)?;
        for px in rgba.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        Some((w, h, rgba))
    })
}

/// Sends plain text directly from a pinned text card to Tomoyo's standalone Translator plugin.
pub fn handoff_text_to_translator(text: &str) {
    let _ = crate::export::spawn_translate_hybrid_viewer(text);
}

/// Minimum visible strip (in px) preserved when dragging a pinned window past any screen edge.
pub(crate) const MIN_PIN_GRAB_MARGIN_PX: i32 = 44;

pub(crate) fn clamp_pin_origin(x: i32, y: i32, win_w: i32, win_h: i32, vrect: RECT) -> (i32, i32) {
    let keep_x = MIN_PIN_GRAB_MARGIN_PX.min(win_w.max(1));
    let keep_y = MIN_PIN_GRAB_MARGIN_PX.min(win_h.max(1));
    let min_x = vrect.left - win_w + keep_x;
    let max_x = (vrect.right - keep_x).max(min_x);
    let min_y = vrect.top - win_h + keep_y;
    let max_y = (vrect.bottom - keep_y).max(min_y);
    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
}

pub(crate) fn virtual_desktop_rect() -> RECT {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if vw > 0 && vh > 0 {
            RECT { left: vx, top: vy, right: vx + vw, bottom: vy + vh }
        } else {
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN).max(640),
                bottom: GetSystemMetrics(SM_CYSCREEN).max(480),
            }
        }
    }
}

pub(crate) fn compute_pin_zoom_origin(
    rc: RECT,
    nw: i32,
    nh: i32,
    cursor: Option<POINT>,
    vrect: RECT,
) -> (i32, i32) {
    let ow = (rc.right - rc.left).max(1);
    let oh = (rc.bottom - rc.top).max(1);
    let (raw_x, raw_y) = match cursor {
        Some(pt) if pt.x >= rc.left && pt.x <= rc.right && pt.y >= rc.top && pt.y <= rc.bottom => {
            let rx = (pt.x - rc.left) as f32 / ow as f32;
            let ry = (pt.y - rc.top) as f32 / oh as f32;
            (pt.x - (nw as f32 * rx).round() as i32, pt.y - (nh as f32 * ry).round() as i32)
        }
        _ => (rc.left + (ow - nw) / 2, rc.top + (oh - nh) / 2),
    };
    clamp_pin_origin(raw_x, raw_y, nw, nh, vrect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_clamp_allows_dragging_beyond_all_four_screen_edges() {
        let vrect = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        assert_eq!(clamp_pin_origin(-700, -500, 800, 600, vrect), (-700, -500));
        assert_eq!(
            clamp_pin_origin(-900, -700, 800, 600, vrect),
            (-800 + MIN_PIN_GRAB_MARGIN_PX, -600 + MIN_PIN_GRAB_MARGIN_PX)
        );
        assert_eq!(
            clamp_pin_origin(2500, 1500, 800, 600, vrect),
            (1920 - MIN_PIN_GRAB_MARGIN_PX, 1080 - MIN_PIN_GRAB_MARGIN_PX)
        );
    }

    #[test]
    fn pin_zoom_anchors_under_cursor_and_stays_grabbable() {
        let vrect = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let rc = RECT { left: 100, top: 100, right: 500, bottom: 300 };
        // Zooming 2x while cursor sits at 25% of width/height keeps cursor pixel fixed.
        let (nx, ny) = compute_pin_zoom_origin(rc, 800, 400, Some(POINT { x: 200, y: 150 }), vrect);
        assert_eq!((nx, ny), (0, 50));
    }
}
