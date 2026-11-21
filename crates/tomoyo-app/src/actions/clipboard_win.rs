//! Win32 native clipboard read/write for Unicode Text, `CF_DIB` PNG Images, and `CF_HDROP` Files.

use base64::Engine;

#[cfg(windows)]
pub fn set_clipboard_text(text: &str) -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = match GlobalAlloc(GMEM_MOVEABLE, byte_len) {
            Ok(hmem) => {
                let ptr = GlobalLock(hmem) as *mut u16;
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                    let _ = GlobalUnlock(hmem);
                    SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0))).is_ok()
                } else {
                    false
                }
            }
            Err(_) => false,
        };
        let _ = CloseClipboard();
        ok
    }
}

#[cfg(windows)]
pub fn read_clipboard_text() -> Option<String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() {
            return None;
        }
        if OpenClipboard(None).is_err() {
            return None;
        }
        let mut out = None;
        if let Ok(handle) = GetClipboardData(CF_UNICODETEXT.0 as u32) {
            let hmem = HGLOBAL(handle.0);
            let max_bytes = GlobalSize(hmem).min(64 * 1024);
            let ptr = GlobalLock(hmem) as *const u16;
            if !ptr.is_null() && max_bytes >= 2 {
                let max_u16 = max_bytes / 2;
                let slice = std::slice::from_raw_parts(ptr, max_u16);
                let len = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
                out = Some(String::from_utf16_lossy(&slice[..len]));
                let _ = GlobalUnlock(hmem);
            }
        }
        let _ = CloseClipboard();
        out
    }
}

/// Checks whether the Windows clipboard currently holds an image (`CF_DIB`) or file list (`CF_HDROP`).
#[cfg(windows)]
pub fn clipboard_has_rich_attachment() -> bool {
    use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;
    use windows::Win32::System::Ole::{CF_DIB, CF_HDROP};
    unsafe {
        IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok()
            || IsClipboardFormatAvailable(CF_DIB.0 as u32).is_ok()
    }
}

/// Reads copied file paths (`CF_HDROP`) from the Windows clipboard (capped at 32 paths).
#[cfg(windows)]
pub fn read_clipboard_files() -> Option<Vec<String>> {
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_err() {
            return None;
        }
        if OpenClipboard(None).is_err() {
            return None;
        }
        let mut files = Vec::new();
        if let Ok(handle) = GetClipboardData(CF_HDROP.0 as u32) {
            let hdrop = HDROP(handle.0);
            let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None).min(32);
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, None);
                if len > 0 && len < 4096 {
                    let mut buf = vec![0u16; (len + 1) as usize];
                    let copied = DragQueryFileW(hdrop, i, Some(&mut buf));
                    if copied > 0 {
                        files.push(String::from_utf16_lossy(&buf[..copied as usize]));
                    }
                }
            }
        }
        let _ = CloseClipboard();
        if files.is_empty() { None } else { Some(files) }
    }
}

/// Captures a `CF_DIB` screenshot/image to a content-addressed PNG on disk (0 pixel bytes kept in RAM).
#[cfg(windows)]
pub fn read_clipboard_image_png() -> Option<(String, u32, u32, String)> {
    use sha2::{Digest, Sha256};
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::Graphics::Gdi::BITMAPINFOHEADER;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::CF_DIB;

    unsafe {
        if IsClipboardFormatAvailable(CF_DIB.0 as u32).is_err() || OpenClipboard(None).is_err() {
            return None;
        }
        let parsed = (|| -> Option<(u32, u32, Vec<u8>)> {
            let handle = GetClipboardData(CF_DIB.0 as u32).ok()?;
            let hmem = HGLOBAL(handle.0);
            let total_size = GlobalSize(hmem);
            let hdr_size = std::mem::size_of::<BITMAPINFOHEADER>();
            if total_size < hdr_size || total_size > 64 * 1024 * 1024 {
                return None;
            }
            let ptr = GlobalLock(hmem) as *const u8;
            if ptr.is_null() {
                return None;
            }
            let result = decode_dib_slice(std::slice::from_raw_parts(ptr, total_size), hdr_size);
            let _ = GlobalUnlock(hmem);
            result
        })();
        let _ = CloseClipboard();

        let (w, h, rgba) = parsed?;
        let mut hasher = Sha256::new();
        hasher.update(w.to_le_bytes());
        hasher.update(h.to_le_bytes());
        hasher.update(&rgba);
        let full_hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let digest16 = full_hex[..16].to_string();

        let dir = tomoyo_core::data_dir().join("clipboard-images");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("img_{digest16}.png"));
        if !path.is_file() {
            let file = std::fs::File::create(&path).ok()?;
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            enc.write_header().ok()?.write_image_data(&rgba).ok()?;
        }
        Some((path.to_string_lossy().into_owned(), w, h, digest16))
    }
}

#[cfg(windows)]
fn decode_dib_slice(slice: &[u8], hdr_size: usize) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_BITFIELDS, BI_RGB};

    let mut hdr = BITMAPINFOHEADER::default();
    unsafe {
        std::ptr::copy_nonoverlapping(slice.as_ptr(), &mut hdr as *mut _ as *mut u8, hdr_size);
    }
    let bi_size = hdr.biSize as usize;
    if bi_size < hdr_size || bi_size > slice.len() || hdr.biWidth <= 0 || hdr.biHeight == 0 {
        return None;
    }
    let w = hdr.biWidth as u32;
    let h = hdr.biHeight.unsigned_abs();
    let bpp = hdr.biBitCount;
    if w > 4096 || h > 4096 || (bpp != 24 && bpp != 32) {
        return None;
    }
    if hdr.biCompression != BI_RGB.0 && hdr.biCompression != BI_BITFIELDS.0 {
        return None;
    }

    let extra_masks = if hdr.biCompression == BI_BITFIELDS.0 && bi_size == hdr_size { 12 } else { 0 };
    let pixel_offset = bi_size
        .saturating_add(extra_masks)
        .saturating_add((hdr.biClrUsed as usize).saturating_mul(4));
    let stride = ((w as usize) * (bpp as usize)).div_ceil(32) * 4;
    let needed = pixel_offset.checked_add(stride.checked_mul(h as usize)?)?;
    if needed > slice.len() {
        return None;
    }

    let pixels = &slice[pixel_offset..needed];
    let top_down = hdr.biHeight < 0;
    let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
    let mut any_alpha = false;
    for y in 0..h as usize {
        let src_y = if top_down { y } else { (h as usize) - 1 - y };
        let src = &pixels[src_y * stride..(src_y + 1) * stride];
        let dst = &mut rgba[y * (w as usize) * 4..(y + 1) * (w as usize) * 4];
        if bpp == 32 {
            for x in 0..w as usize {
                let s = x * 4;
                if src[s + 3] != 0 {
                    any_alpha = true;
                }
                dst[s] = src[s + 2];
                dst[s + 1] = src[s + 1];
                dst[s + 2] = src[s];
                dst[s + 3] = src[s + 3];
            }
        } else {
            for x in 0..w as usize {
                let (s, d) = (x * 3, x * 4);
                dst[d] = src[s + 2];
                dst[d + 1] = src[s + 1];
                dst[d + 2] = src[s];
                dst[d + 3] = 255;
            }
        }
    }
    if bpp == 32 && !any_alpha {
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }
    Some((w, h, rgba))
}

/// Writes a PNG file back onto the Windows clipboard as a standard 32-bit `CF_DIB` bitmap.
#[cfg(windows)]
pub fn set_clipboard_image_from_png(path_str: &str) -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_DIB;

    let Some((w, h, bgra)) = decode_png_to_bottom_up_bgra(path_str) else {
        return false;
    };
    let hdr = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w as i32,
        biHeight: h as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: bgra.len() as u32,
        ..Default::default()
    };
    let hdr_size = std::mem::size_of::<BITMAPINFOHEADER>();
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = match GlobalAlloc(GMEM_MOVEABLE, hdr_size + bgra.len()) {
            Ok(hmem) => {
                let ptr = GlobalLock(hmem) as *mut u8;
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(&hdr as *const _ as *const u8, ptr, hdr_size);
                    std::ptr::copy_nonoverlapping(bgra.as_ptr(), ptr.add(hdr_size), bgra.len());
                    let _ = GlobalUnlock(hmem);
                    SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(hmem.0))).is_ok()
                } else {
                    false
                }
            }
            Err(_) => false,
        };
        let _ = CloseClipboard();
        ok
    }
}

fn decode_png_to_bottom_up_bgra(path_str: &str) -> Option<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path_str).ok()?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let raw = &buf[..frame.buffer_size()];
    let mut bgra = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h as usize {
        let dst_y = (h as usize) - 1 - y;
        for x in 0..w as usize {
            let (r, g, b, a) = match frame.color_type {
                png::ColorType::Rgba => {
                    let i = (y * w as usize + x) * 4;
                    (raw[i], raw[i + 1], raw[i + 2], raw[i + 3])
                }
                png::ColorType::Rgb => {
                    let i = (y * w as usize + x) * 3;
                    (raw[i], raw[i + 1], raw[i + 2], 255)
                }
                png::ColorType::GrayscaleAlpha => {
                    let i = (y * w as usize + x) * 2;
                    (raw[i], raw[i], raw[i], raw[i + 1])
                }
                png::ColorType::Grayscale => {
                    let v = raw[y * w as usize + x];
                    (v, v, v, 255)
                }
                _ => (0, 0, 0, 255),
            };
            let d = (dst_y * w as usize + x) * 4;
            bgra[d] = b;
            bgra[d + 1] = g;
            bgra[d + 2] = r;
            bgra[d + 3] = a;
        }
    }
    Some((w, h, bgra))
}

/// Lazily reads & Base64-encodes a PNG image (max 4 MB) into the clipboard only on user execution.
pub fn copy_image_as_base64_uri(path_str: &str) -> Result<usize, String> {
    let meta = std::fs::metadata(path_str).map_err(|e| format!("无法读取图片: {e}"))?;
    if meta.len() > 4 * 1024 * 1024 {
        return Err("图片超过 4 MB 上限，拒绝编码为 Base64".into());
    }
    let bytes = std::fs::read(path_str).map_err(|e| format!("读取失败: {e}"))?;
    let uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    let len = uri.len();
    if set_clipboard_text(&uri) {
        Ok(len)
    } else {
        Err("写入剪贴板失败".into())
    }
}

/// Checks Windows confidential clipboard markers (`ExcludeClipboardContentFromMonitorProcessing`
/// and `CanIncludeInClipboardHistory == 0`) used by password managers.
#[cfg(windows)]
pub fn is_clipboard_confidential() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    unsafe {
        let excl = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        if excl != 0 && IsClipboardFormatAvailable(excl).is_ok() {
            return true;
        }
        let hist = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if hist != 0 && IsClipboardFormatAvailable(hist).is_ok() && OpenClipboard(None).is_ok() {
            let mut disallowed = false;
            if let Ok(h) = GetClipboardData(hist) {
                let hm = HGLOBAL(h.0);
                if GlobalSize(hm) >= 4 {
                    let ptr = GlobalLock(hm) as *const u32;
                    if !ptr.is_null() {
                        disallowed = *ptr == 0;
                        let _ = GlobalUnlock(hm);
                    }
                }
            }
            let _ = CloseClipboard();
            return disallowed;
        }
        false
    }
}

/// Returns the lowercase `.exe` filename of the clipboard owner or foreground window.
#[cfg(windows)]
pub fn clipboard_source_app_name() -> String {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::DataExchange::GetClipboardOwner;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let mut hwnd = GetClipboardOwner().unwrap_or_default();
        if hwnd.is_invalid() {
            hwnd = GetForegroundWindow();
        }
        if hwnd.is_invalid() {
            return String::new();
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(handle);
        if !ok || len == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&full)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default()
    }
}
