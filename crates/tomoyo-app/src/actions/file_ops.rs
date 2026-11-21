//! Bounded local file efficiency operations for Tomoyo's contextual File Action Flow.

use super::clipboard_win::set_clipboard_text;
use super::shell_win::shell_open;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_COPY_TEXT_FILE_BYTES: u64 = 128 * 1024;
const MAX_HASH_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Launches `tomoyo-capture.exe --pin <image_path>` to pin any image file onto the desktop.
pub fn spawn_pin_image_file(path: &str) -> Result<(), String> {
    if !Path::new(path).is_file() {
        return Err("图片文件不存在".into());
    }
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("tomoyo-capture.exe")))
        .filter(|p| p.exists())
        .ok_or_else(|| "未找到 tomoyo-capture.exe".to_string())?;
    Command::new(exe)
        .args(["--pin", path])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动贴图进程失败: {e}"))
}

/// Safely unpacks a `.zip` archive into a non-colliding sibling folder using Zip-Slip guards.
pub fn extract_zip_into_sibling_dir(zip_path: &str) -> Result<String, String> {
    let zp = Path::new(zip_path);
    if !zp.is_file() {
        return Err("压缩包不存在".into());
    }
    let parent = zp.parent().ok_or_else(|| "无法定位父目录".to_string())?;
    let stem = zp.file_stem().and_then(|s| s.to_str()).unwrap_or("extracted");
    let mut dest = parent.join(stem);
    let mut suffix = 2u32;
    while dest.exists() && suffix < 100 {
        dest = parent.join(format!("{stem}_{suffix}"));
        suffix += 1;
    }
    tomoyo_core::unpack_zip_to_dir(zp, &dest)?;
    let dest_str = dest.to_string_lossy().into_owned();
    let _ = shell_open(&dest_str, None, None);
    Ok(dest_str)
}

/// Reads up to 128 KB of a UTF-8 text file and copies it to the system clipboard.
pub fn copy_file_contents_bounded(path: &str) -> Result<usize, String> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).map_err(|e| format!("读取文件失败: {e}"))?;
    if meta.len() > MAX_COPY_TEXT_FILE_BYTES {
        return Err(format!("文件超过 128 KB 上限（当前 {} KB）", meta.len() / 1024));
    }
    let bytes = std::fs::read(p).map_err(|e| format!("读取文件失败: {e}"))?;
    if bytes.iter().take(512).any(|&b| b == 0) {
        return Err("检测到二进制文件，已取消纯文本复制".into());
    }
    let text = String::from_utf8_lossy(&bytes);
    if !set_clipboard_text(&text) {
        return Err("写入系统剪贴板失败".into());
    }
    Ok(text.chars().count())
}

/// Streams up to 64 MB of a local file to compute its lowercase SHA-256 hex digest and copies it.
pub fn compute_file_sha256_bounded(path: &str) -> Result<String, String> {
    let p = PathBuf::from(path);
    let meta = std::fs::metadata(&p).map_err(|e| format!("读取文件失败: {e}"))?;
    if meta.len() > MAX_HASH_FILE_BYTES {
        return Err("文件超过 64 MB 哈希上限".into());
    }
    let mut f = File::open(&p).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 16_384];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("读取失败: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !set_clipboard_text(&digest) {
        return Err("写入剪贴板失败".into());
    }
    Ok(digest)
}
