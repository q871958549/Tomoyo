//! Streaming WinHTTP file download helper with progress reporting and atomic `.part` swap.

use std::path::Path;

/// Streams an HTTPS/HTTP download directly to `dest` via a 16 KB buffer.
#[cfg(windows)]
pub fn http_download_to_file(
    url: &str,
    dest: &Path,
    timeout_ms: u32,
    max_bytes: u64,
) -> Result<u64, String> {
    http_download_to_file_with_progress(url, dest, timeout_ms, max_bytes, |_, _| {})
}

/// Streams a download into a `.part` temp file, emits progress `(downloaded, total)`, and renames atomically.
#[cfg(windows)]
pub fn http_download_to_file_with_progress<F>(
    url: &str,
    dest: &Path,
    timeout_ms: u32,
    max_bytes: u64,
    mut on_progress: F,
) -> Result<u64, String>
where
    F: FnMut(u64, Option<u64>),
{
    use std::ffi::c_void;
    use std::io::{BufWriter, Write};
    use windows::Win32::Networking::WinHttp::{WinHttpQueryDataAvailable, WinHttpReadData};

    let stream = super::open_http_stream("GET", url, &[], None, timeout_ms, true)?;
    if stream.status_code != 200 {
        return Err(format!("HTTP download failed: status {}", stream.status_code));
    }
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let part_path = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));
    let _ = std::fs::remove_file(&part_path);
    let file = std::fs::File::create(&part_path)
        .map_err(|e| format!("create `{}`: {e}", part_path.display()))?;
    let mut writer = BufWriter::with_capacity(16 * 1024, file);
    let mut buf = [0u8; 16 * 1024];
    let mut total: u64 = 0;
    let expected = stream.content_length;
    on_progress(0, expected);
    let mut next_emit = 64 * 1024u64;

    let write_res: Result<(), String> = unsafe {
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(stream.req.0, &mut avail).is_err() || avail == 0 {
                break Ok(());
            }
            let rem = max_bytes.saturating_sub(total);
            if rem == 0 {
                break Err(format!("Download exceeded safety ceiling ({max_bytes} bytes)"));
            }
            let to_read = (avail as usize).min(buf.len()).min(rem as usize);
            let mut read: u32 = 0;
            if WinHttpReadData(
                stream.req.0,
                buf.as_mut_ptr() as *mut c_void,
                to_read as u32,
                &mut read,
            )
            .is_err()
                || read == 0
            {
                break Ok(());
            }
            if let Err(e) = writer.write_all(&buf[..read as usize]) {
                break Err(format!("write error: {e}"));
            }
            total += read as u64;
            if total >= next_emit {
                on_progress(total, expected);
                next_emit = total + 64 * 1024;
            }
        }
    };
    if let Err(e) = write_res {
        drop(writer);
        let _ = std::fs::remove_file(&part_path);
        return Err(e);
    }
    if let Err(e) = writer.flush() {
        drop(writer);
        let _ = std::fs::remove_file(&part_path);
        return Err(format!("flush error: {e}"));
    }
    drop(writer);
    on_progress(total, expected.or(Some(total)));
    let _ = std::fs::remove_file(dest);
    std::fs::rename(&part_path, dest).map_err(|e| format!("finalize `{}`: {e}", dest.display()))?;
    Ok(total)
}

#[cfg(target_os = "linux")]
pub fn http_download_to_file(
    url: &str,
    dest: &Path,
    timeout_ms: u32,
    max_bytes: u64,
) -> Result<u64, String> {
    http_download_to_file_with_progress(url, dest, timeout_ms, max_bytes, |_, _| {})
}

#[cfg(target_os = "linux")]
pub fn http_download_to_file_with_progress<F>(
    url: &str,
    dest: &Path,
    timeout_ms: u32,
    max_bytes: u64,
    mut on_progress: F,
) -> Result<u64, String>
where
    F: FnMut(u64, Option<u64>),
{
    use std::io::{BufWriter, Write};

    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let part_path = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));
    let _ = std::fs::remove_file(&part_path);
    let file = std::fs::File::create(&part_path)
        .map_err(|e| format!("create `{}`: {e}", part_path.display()))?;
    let mut writer = BufWriter::with_capacity(16 * 1024, file);
    let mut total: u64 = 0;

    let res = crate::linux_sys::curl_stream_download(
        url,
        timeout_ms,
        &mut |chunk| {
            let next = total.saturating_add(chunk.len() as u64);
            if next > max_bytes {
                return Err(format!("Download exceeded safety ceiling ({max_bytes} bytes)"));
            }
            writer
                .write_all(chunk)
                .map_err(|e| format!("write error: {e}"))?;
            total = next;
            Ok(())
        },
        &mut on_progress,
    );

    match res {
        Ok((200, expected)) => {
            if let Err(e) = writer.flush() {
                drop(writer);
                let _ = std::fs::remove_file(&part_path);
                return Err(format!("flush error: {e}"));
            }
            drop(writer);
            on_progress(total, expected.or(Some(total)));
            let _ = std::fs::remove_file(dest);
            std::fs::rename(&part_path, dest)
                .map_err(|e| format!("finalize `{}`: {e}", dest.display()))?;
            Ok(total)
        }
        Ok((status, _)) => {
            drop(writer);
            let _ = std::fs::remove_file(&part_path);
            Err(format!("HTTP download failed: status {status}"))
        }
        Err(e) => {
            drop(writer);
            let _ = std::fs::remove_file(&part_path);
            Err(e)
        }
    }
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn http_download_to_file(
    _url: &str,
    _dest: &Path,
    _timeout_ms: u32,
    _max_bytes: u64,
) -> Result<u64, String> {
    Err("HTTP download is only implemented for Windows and Linux".into())
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn http_download_to_file_with_progress<F>(
    _url: &str,
    _dest: &Path,
    _timeout_ms: u32,
    _max_bytes: u64,
    _on_progress: F,
) -> Result<u64, String>
where
    F: FnMut(u64, Option<u64>),
{
    Err("HTTP download is only implemented for Windows and Linux".into())
}
