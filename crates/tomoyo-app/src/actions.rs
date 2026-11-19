//! Native Windows action executor (launch files/apps, Explorer reveal, Terminal,
//! Clipboard read/write, window focus, system power commands, plugin RPC execute,
//! and ephemeral on-demand `tomoyo-webview-host` spawning).

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tomoyo_plugin_host::{JobGuard, PluginHost};
use tomoyo_protocol::{ActionKind, ExecuteRequest, InternalCommandKind, SystemCommandKind};

#[derive(Debug)]
pub struct ActionOutcome {
    pub hide_window: bool,
    pub paste_after_hide: bool,
    pub toast: Option<String>,
    pub fill_query: Option<String>,
    pub internal: Option<InternalCommandKind>,
}

impl Default for ActionOutcome {
    fn default() -> Self {
        Self {
            hide_window: true,
            paste_after_hide: false,
            toast: None,
            fill_query: None,
            internal: None,
        }
    }
}

pub fn execute_action(
    kind: &ActionKind,
    item_id: &str,
    plugin_host: &Arc<PluginHost>,
    is_dark_theme: bool,
) -> ActionOutcome {
    match kind {
        ActionKind::LaunchPath {
            path,
            args,
            working_dir,
        } => {
            let ok = shell_open(path, args.as_deref(), working_dir.as_deref());
            ActionOutcome {
                hide_window: ok,
                toast: if ok {
                    None
                } else {
                    Some(format!("Failed to open: {path}"))
                },
                ..Default::default()
            }
        }
        ActionKind::RevealInExplorer { path } => {
            let arg = format!("/select,\"{path}\"");
            let ok = shell_open("explorer.exe", Some(&arg), None);
            ActionOutcome {
                hide_window: ok,
                toast: if ok {
                    None
                } else {
                    Some(format!("Failed to reveal: {path}"))
                },
                ..Default::default()
            }
        }
        ActionKind::OpenTerminalAt { dir } => {
            let target_dir = if Path::new(dir).is_dir() {
                dir.as_str()
            } else {
                "."
            };
            let wt_arg = format!("-d \"{target_dir}\"");
            let ok = shell_open("wt.exe", Some(&wt_arg), Some(target_dir))
                || shell_open("powershell.exe", None, Some(target_dir));
            ActionOutcome {
                hide_window: ok,
                toast: if ok {
                    None
                } else {
                    Some("Failed to open terminal".into())
                },
                ..Default::default()
            }
        }
        ActionKind::OpenUrl { url } => {
            let ok = shell_open(url, None, None);
            ActionOutcome {
                hide_window: ok,
                toast: if ok {
                    None
                } else {
                    Some(format!("Failed to open URL: {url}"))
                },
                ..Default::default()
            }
        }
        ActionKind::CopyText {
            text,
            paste_immediately,
        } => {
            let ok = set_clipboard_text(text);
            ActionOutcome {
                hide_window: true,
                paste_after_hide: ok && *paste_immediately,
                toast: Some(if ok {
                    "已复制到剪贴板".into()
                } else {
                    "复制失败".into()
                }),
                ..Default::default()
            }
        }
        ActionKind::CopyImage {
            path,
            paste_immediately,
        } => {
            let ok = set_clipboard_image_from_png(path);
            ActionOutcome {
                hide_window: ok,
                paste_after_hide: ok && *paste_immediately,
                toast: Some(if ok {
                    "已复制图片到剪贴板".into()
                } else {
                    "复制图片失败（文件可能已被删除）".into()
                }),
                ..Default::default()
            }
        }
        ActionKind::FocusWindow { hwnd } => {
            focus_window(*hwnd);
            ActionOutcome {
                hide_window: true,
                ..Default::default()
            }
        }
        ActionKind::FillQuery { query } => ActionOutcome {
            hide_window: false,
            fill_query: Some(query.clone()),
            ..Default::default()
        },
        ActionKind::SystemCommand { command } => {
            run_system_command(*command);
            ActionOutcome {
                hide_window: true,
                ..Default::default()
            }
        }
        ActionKind::InternalCommand { command } => ActionOutcome {
            hide_window: false,
            internal: Some(command.clone()),
            ..Default::default()
        },
        ActionKind::PluginExecute {
            plugin_id,
            action_id,
            payload,
        } => {
            let req = ExecuteRequest {
                action_id: action_id.clone(),
                item_id: item_id.to_string(),
                payload: payload.clone(),
            };
            match plugin_host.execute_action(plugin_id, req) {
                Ok(resp) => {
                    if let Some(txt) = &resp.copy_text {
                        let _ = set_clipboard_text(txt);
                    }
                    if let Some(url) = &resp.open_url {
                        let _ = shell_open(url, None, None);
                    }
                    ActionOutcome {
                        hide_window: resp.hide_window && resp.fill_query.is_none(),
                        paste_after_hide: false,
                        toast: resp.toast,
                        fill_query: resp.fill_query,
                        internal: None,
                    }
                }
                Err(e) => ActionOutcome {
                    hide_window: false,
                    toast: Some(format!("Plugin error: {e}")),
                    ..Default::default()
                },
            }
        }
        ActionKind::OpenWebView {
            plugin_id,
            title,
            entry_path,
            utools_compat,
            initial_query,
            width,
            height,
            max_memory_mb,
        } => match spawn_ephemeral_webview(
            plugin_id,
            title,
            entry_path,
            *utools_compat,
            initial_query,
            *width,
            *height,
            *max_memory_mb,
            is_dark_theme,
        ) {
            Ok(()) => ActionOutcome {
                hide_window: true,
                ..Default::default()
            },
            Err(e) => ActionOutcome {
                hide_window: false,
                toast: Some(format!("WebView host error: {e}")),
                ..Default::default()
            },
        },
    }
}

pub fn spawn_settings_window(parent_hwnd: isize, theme_name: &str) -> Result<(), String> {
    let host_exe = locate_webview_host()?;
    let mut cmd = Command::new(&host_exe);
    cmd.args([
        "--settings",
        "--notify-hwnd",
        &parent_hwnd.to_string(),
        "--theme",
        theme_name,
    ]);
    if theme_name.eq_ignore_ascii_case("light") {
        cmd.arg("--light");
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch settings host {}: {e}", host_exe.display()))?;
    let job = JobGuard::assign_child(&child, 512).ok();
    let _ = std::thread::Builder::new()
        .name("tomoyo-settings-reaper".into())
        .spawn(move || {
            let _keep_job = job;
            let _ = child.wait();
        });
    Ok(())
}

/// Restores the previously focused window and simulates `Ctrl+V` via native `SendInput`
/// so Clipboard History `Enter` pastes straight into the user's active editor/chat!
#[cfg(windows)]
pub fn paste_into_previous_window_async(prev_hwnd: isize) {
    if prev_hwnd == 0 {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("tomoyo-autopaste".into())
        .spawn(move || {
            use std::ffi::c_void;
            use std::time::Duration;
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
                VIRTUAL_KEY, VK_CONTROL,
            };
            use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

            std::thread::sleep(Duration::from_millis(55));
            unsafe {
                let target = HWND(prev_hwnd as *mut c_void);
                let _ = SetForegroundWindow(target);
            }
            std::thread::sleep(Duration::from_millis(35));

            let make_key = |vk: VIRTUAL_KEY, up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                        ..Default::default()
                    },
                },
            };
            let inputs = [
                make_key(VK_CONTROL, false),
                make_key(VIRTUAL_KEY(b'V' as u16), false),
                make_key(VIRTUAL_KEY(b'V' as u16), true),
                make_key(VK_CONTROL, true),
            ];
            unsafe {
                let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        });
}

#[cfg(not(windows))]
pub fn paste_into_previous_window_async(_prev_hwnd: isize) {}

fn spawn_ephemeral_webview(
    plugin_id: &str,
    title: &str,
    entry_path: &str,
    utools_compat: bool,
    initial_query: &str,
    width: u32,
    height: u32,
    max_memory_mb: u32,
    is_dark: bool,
) -> Result<(), String> {
    let host_exe = locate_webview_host()?;
    let mut cmd = Command::new(&host_exe);
    cmd.args([
        "--plugin-id",
        plugin_id,
        "--title",
        title,
        "--entry",
        entry_path,
        "--query",
        initial_query,
        "--width",
        &width.to_string(),
        "--height",
        &height.to_string(),
    ]);
    if utools_compat {
        cmd.arg("--utools-compat");
    }
    if !is_dark {
        cmd.arg("--light");
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch {}: {e}", host_exe.display()))?;
    // Assign to Job Object so exiting Tomoyo cleans up ephemeral WebView hosts cleanly;
    // wait on a tiny reaper thread so the handle & JobGuard are freed as soon as the user closes it.
    let job = JobGuard::assign_child(&child, max_memory_mb.max(512)).ok();
    let _ = std::thread::Builder::new()
        .name("tomoyo-wv-reaper".into())
        .spawn(move || {
            let _keep_job = job;
            let _ = child.wait();
        });
    Ok(())
}

fn locate_webview_host() -> Result<std::path::PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sib = dir.join("tomoyo-webview-host.exe");
            if sib.is_file() {
                return Ok(sib);
            }
            if let Some(parent) = dir.parent() {
                let p = parent.join("tomoyo-webview-host.exe");
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }
    Err("tomoyo-webview-host.exe not found next to tomoyo.exe".into())
}

#[cfg(windows)]
pub fn shell_open(target: &str, args: Option<&str>, workdir: Option<&str>) -> bool {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let target_w: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let args_w: Option<Vec<u16>> = args.map(|a| a.encode_utf16().chain(std::iter::once(0)).collect());
    let dir_w: Option<Vec<u16>> = workdir.map(|d| d.encode_utf16().chain(std::iter::once(0)).collect());

    unsafe {
        let hinst = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(target_w.as_ptr()),
            args_w
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            dir_w
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            SW_SHOWNORMAL,
        );
        (hinst.0 as isize) > 32
    }
}

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
        if files.is_empty() {
            None
        } else {
            Some(files)
        }
    }
}

/// Captures a `CF_DIB` screenshot/image from the Windows clipboard, saves it as a content-addressed
/// PNG inside `%LOCALAPPDATA%\Tomoyo\clipboard-images\`, and returns `(path, width, height, digest16)`.
/// Holds ZERO pixel data in memory after returning!
#[cfg(windows)]
pub fn read_clipboard_image_png() -> Option<(String, u32, u32, String)> {
    use sha2::{Digest, Sha256};
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_BITFIELDS, BI_RGB};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::CF_DIB;

    unsafe {
        if IsClipboardFormatAvailable(CF_DIB.0 as u32).is_err() {
            return None;
        }
        if OpenClipboard(None).is_err() {
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
            let result = (|| -> Option<(u32, u32, Vec<u8>)> {
                let slice = std::slice::from_raw_parts(ptr, total_size);
                let mut hdr = BITMAPINFOHEADER::default();
                std::ptr::copy_nonoverlapping(
                    slice.as_ptr(),
                    &mut hdr as *mut _ as *mut u8,
                    hdr_size,
                );
                let bi_size = hdr.biSize as usize;
                if bi_size < hdr_size || bi_size > total_size {
                    return None;
                }
                let width = hdr.biWidth;
                let height_raw = hdr.biHeight;
                if width <= 0 || height_raw == 0 {
                    return None;
                }
                let w = width as u32;
                let h = height_raw.unsigned_abs();
                if w > 4096 || h > 4096 {
                    return None;
                }
                let bpp = hdr.biBitCount;
                if bpp != 24 && bpp != 32 {
                    return None;
                }
                if hdr.biCompression != BI_RGB.0 && hdr.biCompression != BI_BITFIELDS.0 {
                    return None;
                }

                let extra_masks = if hdr.biCompression == BI_BITFIELDS.0 && bi_size == hdr_size {
                    12usize
                } else {
                    0usize
                };
                let color_table_bytes = (hdr.biClrUsed as usize).saturating_mul(4);
                let pixel_offset = bi_size.saturating_add(extra_masks).saturating_add(color_table_bytes);
                let stride = (((w as usize) * (bpp as usize) + 31) / 32) * 4;
                let needed = pixel_offset.checked_add(stride.checked_mul(h as usize)?)?;
                if needed > total_size {
                    return None;
                }
                let pixels = &slice[pixel_offset..needed];
                let top_down = height_raw < 0;

                let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
                let mut any_nonzero_alpha = false;
                for y in 0..h as usize {
                    let src_row_idx = if top_down { y } else { (h as usize) - 1 - y };
                    let src_row = &pixels[src_row_idx * stride..(src_row_idx + 1) * stride];
                    let dst_row = &mut rgba[y * (w as usize) * 4..(y + 1) * (w as usize) * 4];
                    if bpp == 32 {
                        for x in 0..w as usize {
                            let s = x * 4;
                            let a = src_row[s + 3];
                            if a != 0 {
                                any_nonzero_alpha = true;
                            }
                            dst_row[s] = src_row[s + 2];
                            dst_row[s + 1] = src_row[s + 1];
                            dst_row[s + 2] = src_row[s];
                            dst_row[s + 3] = a;
                        }
                    } else {
                        for x in 0..w as usize {
                            let s = x * 3;
                            let d = x * 4;
                            dst_row[d] = src_row[s + 2];
                            dst_row[d + 1] = src_row[s + 1];
                            dst_row[d + 2] = src_row[s];
                            dst_row[d + 3] = 255;
                        }
                    }
                }
                // Many Windows apps (Snipping Tool / Paint) put 0 in the 32-bit DIB alpha channel!
                if bpp == 32 && !any_nonzero_alpha {
                    for px in rgba.chunks_exact_mut(4) {
                        px[3] = 255;
                    }
                }
                Some((w, h, rgba))
            })();
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
            let writer = std::io::BufWriter::new(file);
            let mut enc = png::Encoder::new(writer, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            let mut png_writer = enc.write_header().ok()?;
            png_writer.write_image_data(&rgba).ok()?;
        }
        Some((path.to_string_lossy().into_owned(), w, h, digest16))
    }
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

    let decoded = (|| -> Option<(u32, u32, Vec<u8>)> {
        let file = std::fs::File::open(path_str).ok()?;
        let mut dec = png::Decoder::new(std::io::BufReader::new(file));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = dec.read_info().ok()?;
        let info = reader.info();
        let w = info.width;
        let h = info.height;
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return None;
        }
        let mut buf = vec![0u8; reader.output_buffer_size()?];
        let frame = reader.next_frame(&mut buf).ok()?;
        let raw = &buf[..frame.buffer_size()];
        let mut bgra = vec![0u8; (w as usize) * (h as usize) * 4];
        for y in 0..h as usize {
            let dst_y = (h as usize) - 1 - y; // Bottom-up DIB
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
    })();

    let Some((w, h, bgra)) = decoded else {
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
    let total_bytes = hdr_size + bgra.len();

    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = match GlobalAlloc(GMEM_MOVEABLE, total_bytes) {
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

/// Terminates a Windows process cleanly by PID (`kill` / `ps` provider action).
#[cfg(windows)]
pub fn kill_process_by_pid(pid: u32) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    if pid == 0 || pid == 4 || pid == std::process::id() {
        return Err("拒绝结束受保护系统或自身进程".into());
    }
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| format!("无法打开进程 PID {pid}（可能需要管理员权限）: {e}"))?;
        let res = TerminateProcess(handle, 1)
            .map_err(|e| format!("结束进程失败: {e}"));
        let _ = CloseHandle(handle);
        res
    }
}

#[cfg(windows)]
fn focus_window(raw_hwnd: isize) {
    use std::ffi::c_void;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    unsafe {
        let hwnd = HWND(raw_hwnd as *mut c_void);
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(windows)]
fn run_system_command(cmd: SystemCommandKind) {
    use windows::Win32::System::Shutdown::LockWorkStation;
    use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOSOUND};

    match cmd {
        SystemCommandKind::LockWorkstation => unsafe {
            let _ = LockWorkStation();
        },
        SystemCommandKind::Sleep => {
            let _ = shell_open(
                "rundll32.exe",
                Some("powrprof.dll,SetSuspendState 0,1,0"),
                None,
            );
        }
        SystemCommandKind::EmptyRecycleBin => unsafe {
            let _ = SHEmptyRecycleBinW(None, None, SHERB_NOCONFIRMATION | SHERB_NOSOUND);
        },
        SystemCommandKind::OpenSettings => {
            let _ = shell_open("ms-settings:", None, None);
        }
        SystemCommandKind::OpenTaskManager => {
            let _ = shell_open("taskmgr.exe", None, None);
        }
        SystemCommandKind::OpenTerminal => {
            if !shell_open("wt.exe", None, None) {
                let _ = shell_open("powershell.exe", None, None);
            }
        }
    }
}
