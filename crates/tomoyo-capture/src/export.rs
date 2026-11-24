//! Image clipboard (`CF_DIB`), PNG persistence, and direct OCR / Screenshot-Translate handoff.

use std::path::{Path, PathBuf};
use tomoyo_core::{data_dir, logs_dir, TomoyoConfig};

pub fn default_save_dir(custom_dir: &str) -> PathBuf {
    let dir = if !custom_dir.trim().is_empty() && Path::new(custom_dir.trim()).is_dir() {
        PathBuf::from(custom_dir.trim())
    } else if cfg!(target_os = "linux") {
        std::env::var_os("XDG_PICTURES_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Pictures")))
            .map(|p| p.join("Tomoyo"))
            .unwrap_or_else(|| data_dir().join("screenshots"))
    } else if let Ok(profile) = std::env::var("USERPROFILE") {
        PathBuf::from(profile).join("Pictures").join("Tomoyo")
    } else {
        data_dir().join("screenshots")
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn default_save_path(custom_dir: &str) -> PathBuf {
    let dir = default_save_dir(custom_dir);
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    dir.join(format!("Tomoyo_{ts}.png"))
}

pub fn save_bgra_as_png(path: &Path, width: u32, height: u32, bgra: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for px in bgra.as_chunks::<4>().0 {
        rgba.push(px[2]);
        rgba.push(px[1]);
        rgba.push(px[0]);
        rgba.push(255);
    }
    let file = std::fs::File::create(path).map_err(|e| format!("create png: {e}"))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(&rgba)
        .map_err(|e| format!("png data: {e}"))
}

pub fn load_png_as_bgra(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open png: {e}"))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| format!("decode png info: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("decode png frame: {e}"))?;
    let (w, h) = (info.width, info.height);
    let mut bgra = Vec::with_capacity((w as usize) * (h as usize) * 4);
    match info.color_type {
        png::ColorType::Rgba => {
            for px in buf[..info.buffer_size()].as_chunks::<4>().0 {
                bgra.extend_from_slice(&[px[2], px[1], px[0], 255]);
            }
        }
        png::ColorType::Rgb => {
            for px in buf[..info.buffer_size()].as_chunks::<3>().0 {
                bgra.extend_from_slice(&[px[2], px[1], px[0], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in buf[..info.buffer_size()].as_chunks::<2>().0 {
                bgra.extend_from_slice(&[px[0], px[0], px[0], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in &buf[..info.buffer_size()] {
                bgra.extend_from_slice(&[g, g, g, 255]);
            }
        }
        _ => return Err("Unsupported PNG color layout".into()),
    }
    Ok((w, h, bgra))
}

#[cfg(windows)]
pub fn copy_bgra_to_clipboard(width: u32, height: u32, bgra: &[u8]) -> Result<(), String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    let hdr_size = std::mem::size_of::<BITMAPINFOHEADER>();
    let row_bytes = (width as usize) * 4;
    let total = hdr_size + row_bytes * (height as usize);
    let bmi = BITMAPINFOHEADER {
        biSize: hdr_size as u32,
        biWidth: width as i32,
        biHeight: height as i32, // bottom-up standard CF_DIB for maximum app compatibility
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: (row_bytes * height as usize) as u32,
        ..Default::default()
    };

    unsafe {
        if OpenClipboard(None).is_err() {
            return Err("OpenClipboard failed".into());
        }
        let _ = EmptyClipboard();
        let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, total) else {
            let _ = CloseClipboard();
            return Err("GlobalAlloc CF_DIB failed".into());
        };
        let ptr = GlobalLock(hmem) as *mut u8;
        if ptr.is_null() {
            let _ = CloseClipboard();
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(&bmi as *const _ as *const u8, ptr, hdr_size);
        let dst_pixels = std::slice::from_raw_parts_mut(ptr.add(hdr_size), row_bytes * height as usize);
        for y in 0..(height as usize) {
            let src_y = (height as usize - 1) - y;
            let src = &bgra[src_y * row_bytes..(src_y + 1) * row_bytes];
            dst_pixels[y * row_bytes..(y + 1) * row_bytes].copy_from_slice(src);
        }
        let _ = GlobalUnlock(hmem);
        let ok = SetClipboardData(8, Some(HANDLE(hmem.0))).is_ok(); // CF_DIB = 8
        let _ = CloseClipboard();
        if ok { Ok(()) } else { Err("SetClipboardData(CF_DIB) failed".into()) }
    }
}

#[cfg(windows)]
pub fn copy_text_to_clipboard(text: &str) {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * 2;
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
            let ptr = GlobalLock(hmem) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(hmem);
                let _ = SetClipboardData(13, Some(HANDLE(hmem.0))); // CF_UNICODETEXT = 13
            }
        }
        let _ = CloseClipboard();
    }
}

#[cfg(windows)]
pub fn read_clipboard_image_bgra() -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::Graphics::Gdi::BITMAPINFOHEADER;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    unsafe {
        if IsClipboardFormatAvailable(8).is_err() {
            return None;
        }
        let mut opened = false;
        for attempt in 0..6 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            if attempt < 5 {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
        if !opened {
            return None;
        }
        let res = (|| -> Option<(u32, u32, Vec<u8>)> {
            let h = GetClipboardData(8).ok()?;
            let hg = HGLOBAL(h.0);
            let total = GlobalSize(hg);
            let min_hdr = std::mem::size_of::<BITMAPINFOHEADER>();
            if total < min_hdr {
                return None;
            }
            let ptr = GlobalLock(hg) as *const u8;
            if ptr.is_null() {
                return None;
            }
            let hdr = &*(ptr as *const BITMAPINFOHEADER);
            let w = hdr.biWidth.unsigned_abs();
            let h_px = hdr.biHeight.unsigned_abs();
            let bpp = hdr.biBitCount as usize;
            let top_down = hdr.biHeight < 0;
            let out = if (bpp == 24 || bpp == 32) && w > 0 && h_px > 0 && w <= 8192 && h_px <= 8192 {
                let mut offset = hdr.biSize as usize;
                if hdr.biCompression == 3 && hdr.biSize == 40 {
                    offset += 12;
                }
                let src_stride = (w as usize * bpp).div_ceil(32) * 4;
                if offset + src_stride * (h_px as usize) <= total {
                    let pixels = std::slice::from_raw_parts(ptr.add(offset), src_stride * (h_px as usize));
                    let mut bgra = vec![0u8; (w as usize) * (h_px as usize) * 4];
                    for y in 0..(h_px as usize) {
                        let sy = if top_down { y } else { (h_px as usize - 1) - y };
                        let row = &pixels[sy * src_stride..(sy + 1) * src_stride];
                        for x in 0..(w as usize) {
                            let s = x * (bpp / 8);
                            let d = (y * (w as usize) + x) * 4;
                            bgra[d] = row[s];
                            bgra[d + 1] = row[s + 1];
                            bgra[d + 2] = row[s + 2];
                            bgra[d + 3] = 255;
                        }
                    }
                    Some((w, h_px, bgra))
                } else {
                    None
                }
            } else {
                None
            };
            let _ = GlobalUnlock(hg);
            out
        })();
        let _ = CloseClipboard();
        res
    }
}

fn find_sibling_exe(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    let debug = dir.join("target").join("release").join(name);
    if debug.is_file() {
        return Some(debug);
    }
    None
}

#[cfg(target_os = "linux")]
pub use crate::linux_clip::{
    copy_bgra_to_clipboard, copy_text_to_clipboard, read_clipboard_image_bgra,
};

#[cfg(any(windows, target_os = "linux"))]
pub fn locate_plugin_ui(folder_name: &str) -> Option<PathBuf> {
    let mut dirs = vec![data_dir().join("plugins")];
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.join("plugins"));
        if let Some(p) = dir.parent().and_then(|d| d.parent()) {
            dirs.push(p.join("plugins"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("plugins"));
    }
    for root in dirs {
        let ui = root.join(folder_name).join("ui").join("index.html");
        if ui.is_file() {
            return Some(ui);
        }
    }
    None
}

#[cfg(any(windows, target_os = "linux"))]
pub fn spawn_translate_hybrid_viewer(text: &str) -> Result<(), String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let cfg = TomoyoConfig::load_or_create();
    let req_dir = logs_dir();
    let _ = std::fs::create_dir_all(&req_dir);
    let payload = serde_json::json!({ "text": trimmed, "target_lang": cfg.translate.target_lang });
    let _ = std::fs::write(req_dir.join("last_translate_req.json"), payload.to_string());
    let ui_entry = locate_plugin_ui("official-translate")
        .ok_or("未安装官方翻译插件（请在设置中心→插件市场安装「多源对照翻译」）")?;
    let host_name = if cfg!(windows) { "tomoyo-webview-host.exe" } else { "tomoyo-webview-host" };
    let host_exe = find_sibling_exe(host_name).ok_or_else(|| format!("未找到 {host_name}"))?;
    let mut cmd = std::process::Command::new(host_exe);
    cmd.args([
        "--plugin-id", "dev.tomoyo.translate",
        "--entry", &ui_entry.to_string_lossy(),
        "--title", "Tomoyo 多源翻译",
        "--theme", &cfg.theme,
        "--width", "820",
        "--height", "620",
    ]);
    if trimmed.chars().count() <= 2000 {
        cmd.args(["--query", trimmed]);
    }
    cmd.spawn().map_err(|e| format!("拉起翻译窗口失败: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn handoff_text_to_translator(text: &str) {
    let _ = spawn_translate_hybrid_viewer(text);
}

#[cfg(any(windows, target_os = "linux"))]
fn run_offline_ocr_json(width: u32, height: u32, bgra: &[u8]) -> Result<(PathBuf, serde_json::Value), String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let img_dir = data_dir().join("clipboard-images");
    let _ = std::fs::create_dir_all(&img_dir);
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S_%3f");
    let img_path = img_dir.join(format!("capture_{ts}.png"));
    save_bgra_as_png(&img_path, width, height, bgra)?;

    let ocr_name = if cfg!(windows) { "tomoyo-plugin-ocr.exe" } else { "tomoyo-plugin-ocr" };
    let ocr_exe = find_sibling_exe(ocr_name)
        .ok_or_else(|| format!("未找到 {ocr_name}"))?;
    let mut cmd = std::process::Command::new(&ocr_exe);
    cmd.args(["--recognize", &img_path.to_string_lossy(), "--json"]);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("运行 OCR 失败: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed = serde_json::from_str::<serde_json::Value>(stdout.trim()).unwrap_or_else(|_| {
        let fallback = stdout
            .lines()
            .skip_while(|l| l.trim_start().starts_with("[OCR "))
            .collect::<Vec<_>>()
            .join("\r\n");
        serde_json::json!({
            "image_path": img_path.to_string_lossy(),
            "text": fallback.trim(),
            "raw_text": fallback.trim(),
            "lang_tag": "WinRT",
            "src_dims": [width, height],
            "proc_dims": [width, height],
            "elapsed_ms": 0,
        })
    });
    let _ = std::fs::write(logs_dir().join("last_ocr.json"), parsed.to_string());
    Ok((img_path, parsed))
}

/// Runs offline OCR and directly copies clean extracted text to clipboard (for `Shift+C` direct extract).
/// Locates the standalone `builtin-ocr/ui/index.html` Hybrid plugin UI entrypoint across plugin roots.
#[cfg(any(windows, target_os = "linux"))]
fn locate_ocr_plugin_ui() -> Option<std::path::PathBuf> {
    let rel = std::path::Path::new("plugins").join("builtin-ocr").join("ui").join("index.html");
    let mut cands = vec![tomoyo_core::data_dir().join(&rel)];
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        cands.push(dir.join(&rel));
        if let Some(prefix) = dir.parent() {
            cands.push(prefix.join("share").join("tomoyo").join(&rel));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        cands.push(cwd.join(&rel));
    }
    #[cfg(target_os = "linux")]
    cands.push(std::path::PathBuf::from("/usr/share/tomoyo").join(&rel));
    cands.into_iter().find(|p| p.is_file())
}

#[cfg(any(windows, target_os = "linux"))]
fn spawn_ocr_hybrid_viewer(host_exe: &Path, theme: &str, img_path: &Path) -> Result<(), String> {
    let ui_entry = locate_ocr_plugin_ui()
        .ok_or_else(|| "未检测到离线 OCR 插件前端（请在设置中心「插件市场」安装离线 OCR 插件）".to_string())?;
    std::process::Command::new(host_exe)
        .args([
            "--plugin-id", "dev.tomoyo.ocr",
            "--entry", &ui_entry.to_string_lossy(),
            "--title", "Tomoyo OCR 审校",
            "--theme", theme,
            "--query", &img_path.to_string_lossy(),
            "--width", "980",
            "--height", "660",
        ])
        .spawn()
        .map_err(|e| format!("拉起 OCR 审校窗口失败: {e}"))?;
    Ok(())
}

/// Falls back to opening OCR Inspector if no readable text was detected.
#[cfg(any(windows, target_os = "linux"))]
pub fn handoff_ocr_copy_direct(width: u32, height: u32, bgra: &[u8]) -> Result<(), String> {
    let (img_path, parsed) = run_offline_ocr_json(width, height, bgra)?;
    let text = parsed.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
    if !text.is_empty() {
        copy_text_to_clipboard(text);
        return Ok(());
    }
    let cfg = TomoyoConfig::load_or_create();
    let host_name = if cfg!(windows) { "tomoyo-webview-host.exe" } else { "tomoyo-webview-host" };
    let host_exe = find_sibling_exe(host_name)
        .ok_or_else(|| format!("未找到 {host_name}"))?;
    spawn_ocr_hybrid_viewer(&host_exe, &cfg.theme, &img_path)
}

/// Saves screenshot PNG, invokes `tomoyo-plugin-ocr.exe --json` offline, and opens either
/// the standalone Hybrid OCR viewer or Translator.
#[cfg(any(windows, target_os = "linux"))]
pub fn handoff_to_ocr_or_translate(width: u32, height: u32, bgra: &[u8], translate: bool) -> Result<(), String> {
    let (img_path, parsed) = run_offline_ocr_json(width, height, bgra)?;
    let recognized = parsed.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();

    let cfg = TomoyoConfig::load_or_create();
    let host_name = if cfg!(windows) { "tomoyo-webview-host.exe" } else { "tomoyo-webview-host" };
    let host_exe = find_sibling_exe(host_name)
        .ok_or_else(|| format!("未找到 {host_name}"))?;

    if translate {
        spawn_translate_hybrid_viewer(&recognized)?;
    } else {
        spawn_ocr_hybrid_viewer(&host_exe, &cfg.theme, &img_path)?;
    }
    Ok(())
}
