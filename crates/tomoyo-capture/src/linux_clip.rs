//! Wayland-persistent Linux clipboard handoff via running `tomoyo` IPC socket or GTK/`wl-copy`.

use crate::export::save_bgra_as_png;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn tomoyo_cap_clip_set_bgra(bgra: *const u8, w: i32, h: i32) -> i32;
    fn tomoyo_cap_clip_set_text(txt: *const std::ffi::c_char);
    fn tomoyo_cap_clip_get_image_bgra(ow: *mut u32, oh: *mut u32, olen: *mut usize) -> *mut u8;
    fn tomoyo_cap_free(p: *mut std::ffi::c_void);
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

pub fn copy_bgra_to_clipboard(width: u32, height: u32, bgra: &[u8]) -> Result<(), String> {
    let rdir = runtime_dir();
    let sock = rdir.join("tomoyo-ipc.sock");
    if sock.exists() {
        let tmp_png = rdir.join(format!("tomoyo-cap-{}.png", std::process::id()));
        if save_bgra_as_png(&tmp_png, width, height, bgra).is_ok()
            && let Ok(mut stream) = UnixStream::connect(&sock)
        {
            let msg = format!("clip_img:{}\n", tmp_png.display());
            if stream.write_all(msg.as_bytes()).is_ok() {
                let deadline = Instant::now() + Duration::from_millis(250);
                while tmp_png.exists() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(8));
                }
                if !tmp_png.exists() {
                    return Ok(());
                }
            }
            let _ = std::fs::remove_file(&tmp_png);
        }
    }

    if let Ok(mut child) = Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let mut png_bytes = Vec::new();
            let mut enc = png::Encoder::new(&mut png_bytes, width, height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            if let Ok(mut w) = enc.write_header() {
                let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
                for px in bgra.as_chunks::<4>().0 {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                }
                let _ = w.write_image_data(&rgba);
            }
            let _ = stdin.write_all(&png_bytes);
        }
        if child.wait().is_ok_and(|s| s.success()) {
            return Ok(());
        }
    }

    let ok = unsafe { tomoyo_cap_clip_set_bgra(bgra.as_ptr(), width as i32, height as i32) };
    if ok != 0 {
        Ok(())
    } else {
        Err("写入 Linux 剪贴板图片失败".into())
    }
}

pub fn copy_text_to_clipboard(text: &str) {
    let sock = runtime_dir().join("tomoyo-ipc.sock");
    if sock.exists()
        && let Ok(mut stream) = UnixStream::connect(&sock)
        && let Ok(js) = serde_json::to_string(text)
        && stream.write_all(format!("clip_txt:{js}\n").as_bytes()).is_ok()
    {
        return;
    }
    if let Ok(mut child) = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        && let Some(mut stdin) = child.stdin.take()
    {
        let _ = stdin.write_all(text.as_bytes());
        drop(stdin);
        if child.wait().is_ok_and(|s| s.success()) {
            return;
        }
    }
    if let Ok(cs) = std::ffi::CString::new(text) {
        unsafe { tomoyo_cap_clip_set_text(cs.as_ptr()); }
    }
}

pub fn read_clipboard_image_bgra() -> Option<(u32, u32, Vec<u8>)> {
    let (mut w, mut h, mut len) = (0u32, 0u32, 0usize);
    let ptr = unsafe { tomoyo_cap_clip_get_image_bgra(&mut w, &mut h, &mut len) };
    if ptr.is_null() || w == 0 || h == 0 || len == 0 {
        return None;
    }
    let bgra = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    unsafe { tomoyo_cap_free(ptr.cast()); }
    Some((w, h, bgra))
}

pub fn copy_file_uri_to_clipboard(path: &std::path::Path) -> bool {
    let path_str = path.to_string_lossy();
    let sock = runtime_dir().join("tomoyo-ipc.sock");
    if sock.exists()
        && let Ok(mut stream) = UnixStream::connect(&sock)
        && stream.write_all(format!("clip_file:{path_str}\n").as_bytes()).is_ok()
    {
        return true;
    }
    let uri = format!("file://{path_str}\r\n");
    if let Ok(mut child) = Command::new("wl-copy")
        .args(["--type", "text/uri-list"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        && let Some(mut stdin) = child.stdin.take()
    {
        let _ = stdin.write_all(uri.as_bytes());
        drop(stdin);
        if child.wait().is_ok_and(|s| s.success()) {
            return true;
        }
    }
    false
}
