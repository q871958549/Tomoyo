//! Native Linux GTK3/GDK clipboard capture, image PNG storage, privacy guard & restore.

use base64::Engine;
use sha2::{Digest, Sha256};
use std::ffi::{c_char, c_int, CStr, CString};

unsafe extern "C" {
    fn tomoyo_linux_clipboard_set_text(text: *const c_char) -> c_int;
    fn tomoyo_linux_clipboard_get_text() -> *mut c_char;
    fn tomoyo_linux_clipboard_has_rich() -> c_int;
    fn tomoyo_linux_clipboard_is_confidential() -> c_int;
    fn tomoyo_linux_clipboard_get_uris() -> *mut c_char;
    fn tomoyo_linux_clipboard_get_wl_png(out_len: *mut usize) -> *mut u8;
    fn tomoyo_linux_clipboard_get_image_rgba(
        out_w: *mut u32,
        out_h: *mut u32,
        out_len: *mut usize,
    ) -> *mut u8;
    fn tomoyo_linux_clipboard_set_image_png(path: *const c_char) -> c_int;
    fn tomoyo_linux_clipboard_set_file_uri(path: *const c_char) -> c_int;
    fn tomoyo_wl_clip_active() -> c_int;
    fn tomoyo_linux_active_app_name() -> *mut c_char;
    fn tomoyo_linux_free_ptr(ptr: *mut std::ffi::c_void);
}

fn take_c_string(ptr: *mut c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    unsafe { tomoyo_linux_free_ptr(ptr.cast()); }
    Some(s)
}

pub fn set_clipboard_text(text: &str) -> bool {
    let Ok(cs) = CString::new(text) else { return false; };
    unsafe { tomoyo_linux_clipboard_set_text(cs.as_ptr()) != 0 }
}

pub fn read_clipboard_text() -> Option<String> {
    if clipboard_has_rich_attachment() {
        return None;
    }
    let s = take_c_string(unsafe { tomoyo_linux_clipboard_get_text() })
        .filter(|s| !s.trim().is_empty());
    if s.is_some() || unsafe { tomoyo_wl_clip_active() != 0 } {
        return s;
    }
    tomoyo_core::linux_sys::dbus_klipper_get().filter(|s| !s.trim().is_empty())
}

pub fn clipboard_has_rich_attachment() -> bool {
    unsafe { tomoyo_linux_clipboard_has_rich() != 0 }
}

pub fn clipboard_has_image() -> bool {
    clipboard_has_rich_attachment() && read_clipboard_files().is_none()
}

pub fn read_clipboard_files() -> Option<Vec<String>> {
    let raw = take_c_string(unsafe { tomoyo_linux_clipboard_get_uris() })?;
    let files: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.strip_prefix("file://").map(percent_decode).unwrap_or_else(|| l.to_string()))
        .take(64)
        .collect();
    (!files.is_empty()).then_some(files)
}

pub fn read_clipboard_image_png() -> Option<(String, u32, u32, String)> {
    let (w, h, rgba, raw_png) = if unsafe { tomoyo_wl_clip_active() != 0 } {
        let mut plen = 0usize;
        let ptr = unsafe { tomoyo_linux_clipboard_get_wl_png(&mut plen) };
        if ptr.is_null() || plen == 0 {
            return None;
        }
        let png_bytes = unsafe { std::slice::from_raw_parts(ptr, plen) }.to_vec();
        unsafe { tomoyo_linux_free_ptr(ptr.cast()); }
        let (w, h, rgba) = tomoyo_core::wic_thumb::decode_png_rgba_bytes(&png_bytes)?;
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return None;
        }
        (w, h, rgba, Some(png_bytes))
    } else {
        let (mut w, mut h, mut len) = (0u32, 0u32, 0usize);
        let ptr = unsafe { tomoyo_linux_clipboard_get_image_rgba(&mut w, &mut h, &mut len) };
        if ptr.is_null() || w == 0 || h == 0 || len == 0 {
            return None;
        }
        let rgba = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        unsafe { tomoyo_linux_free_ptr(ptr.cast()); }
        (w, h, rgba, None)
    };

    let mut hasher = Sha256::new();
    hasher.update(w.to_le_bytes());
    hasher.update(h.to_le_bytes());
    hasher.update(&rgba);
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let digest16 = hex[..16].to_string();

    let dir = tomoyo_core::data_dir().join("clipboard-images");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("img_{digest16}.png"));
    if !path.is_file() {
        if let Some(png_bytes) = raw_png {
            std::fs::write(&path, &png_bytes).ok()?;
        } else {
            let file = std::fs::File::create(&path).ok()?;
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            enc.write_header().ok()?.write_image_data(&rgba).ok()?;
        }
    }
    Some((path.to_string_lossy().into_owned(), w, h, digest16))
}

pub fn set_clipboard_image_from_png(path_str: &str) -> bool {
    let Ok(cs) = CString::new(path_str) else { return false; };
    unsafe { tomoyo_linux_clipboard_set_image_png(cs.as_ptr()) != 0 }
}

pub fn set_clipboard_file_uri(path_str: &str) -> bool {
    let Ok(cs) = CString::new(path_str) else { return false; };
    unsafe { tomoyo_linux_clipboard_set_file_uri(cs.as_ptr()) != 0 }
}

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

pub fn is_clipboard_confidential() -> bool {
    unsafe { tomoyo_linux_clipboard_is_confidential() != 0 }
}

pub fn clipboard_source_app_name() -> String {
    take_c_string(unsafe { tomoyo_linux_active_app_name() })
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((h << 4) | l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
