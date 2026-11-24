//! Bounded, binary-safe file & metadata preview generator for the right-side preview pane.
//! Strictly caps disk reads to 8 KB so previewing multi-GB files never spikes RAM or I/O.

use chrono::{DateTime, Local};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const MAX_PREVIEW_READ_BYTES: usize = 8192;
const MAX_PREVIEW_LINES: usize = 26;

#[derive(Debug, Clone, Default)]
pub struct ResolvedPreview {
    pub title: String,
    pub meta_line: String,
    pub badges: Vec<String>,
    pub body: String,
    pub is_monospace: bool,
    /// Optional bounded RGBA thumbnail (`(width, height, rgba_bytes)`) for image previews.
    /// Dropped immediately on hibernation -> zero lingering RAM!
    pub image_rgba: Option<(u32, u32, Vec<u8>)>,
}

/// Decodes a local image (PNG/JPEG/WebP/BMP/GIF/ICO via Win32 WIC, falling back to pure-Rust PNG)
/// into a bounded RGBA thumbnail (`<= max_w x max_h`).
pub fn read_image_thumbnail(path_str: &str, max_w: u32, max_h: u32) -> ResolvedPreview {
    let path = Path::new(path_str);
    let title = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "图片预览".into());
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let ext_label = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_uppercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "IMG".into());

    let decoded = crate::wic_thumb::decode_image_thumbnail_wic(path, max_w, max_h)
        .or_else(|| crate::wic_thumb::decode_png_thumbnail(path, max_w, max_h));

    match decoded {
        Some((sw, sh, dw, dh, rgba)) => ResolvedPreview {
            title,
            meta_line: format!("{ext_label} · {sw}×{sh} · {}", format_size(file_size)),
            body: format!("路径：{path_str}"),
            is_monospace: false,
            image_rgba: Some((dw, dh, rgba)),
            ..Default::default()
        },
        None => ResolvedPreview {
            title,
            meta_line: format!("图片 · {}", format_size(file_size)),
            body: format!("路径：{path_str}\n\n（暂不支持解码该图片格式缩略图，按回车可直接打开）"),
            is_monospace: false,
            ..Default::default()
        },
    }
}

/// Safely reads bounded metadata + text snippet for a file or folder path.
pub fn read_file_preview(path_str: &str) -> ResolvedPreview {
    let path = Path::new(path_str);
    let title = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_str.to_string());

    let Ok(meta) = std::fs::metadata(path) else {
        return ResolvedPreview {
            title,
            meta_line: "Unavailable".into(),
            body: format!("Path:\n{path_str}\n\nFile is not accessible or no longer exists."),
            is_monospace: false,
            image_rgba: None,
            badges: Vec::new(),
        };
    };

    let modified = meta
        .modified()
        .ok()
        .map(|st| {
            let dt: DateTime<Local> = st.into();
            dt.format("%Y-%m-%d %H:%M").to_string()
        })
        .unwrap_or_else(|| "-".into());

    if meta.is_dir() {
        let mut entries_preview = Vec::new();
        let mut total = 0usize;
        if let Ok(rd) = std::fs::read_dir(path) {
            for ent in rd.flatten() {
                total += 1;
                if entries_preview.len() < 18 {
                    let is_d = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    let mark = if is_d { "[DIR] " } else { "      " };
                    entries_preview.push(format!("{mark}{}", ent.file_name().to_string_lossy()));
                }
            }
        }
        let body = if entries_preview.is_empty() {
            "(Empty folder)".into()
        } else {
            let mut s = entries_preview.join("\n");
            if total > entries_preview.len() {
                s.push_str(&format!("\n... +{} more entries", total - entries_preview.len()));
            }
            s
        };
        return ResolvedPreview {
            title,
            meta_line: format!("Folder · {total} items · Modified {modified}"),
            body,
            is_monospace: true,
            image_rgba: None,
            badges: Vec::new(),
        };
    }

    let size = meta.len();
    let size_str = format_size(size);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "ico" | "tif" | "tiff"
    ) && size <= 16 * 1024 * 1024
    {
        return read_image_thumbnail(path_str, 280, 220);
    }

    if let Some(rich) = crate::preview_docs::try_preview_document(path, &ext, &title, size, &size_str, &modified) {
        return rich;
    }

    let meta_line = format!(
        "{} · {} · {}",
        if ext.is_empty() { "FILE".into() } else { ext.to_uppercase() },
        size_str,
        modified
    );

    if size == 0 {
        return ResolvedPreview {
            title,
            meta_line,
            body: "(Empty file)".into(),
            is_monospace: false,
            image_rgba: None,
            badges: Vec::new(),
        };
    }

    // Bounded read (<= 8 KB)
    let mut buf = vec![0u8; (size as usize).min(MAX_PREVIEW_READ_BYTES)];
    let Ok(mut f) = File::open(path) else {
        return ResolvedPreview {
            title,
            meta_line,
            body: format!("Path:\n{path_str}\n\n(Unable to read file contents)"),
            is_monospace: false,
            image_rgba: None,
            badges: Vec::new(),
        };
    };
    let n = f.read(&mut buf).unwrap_or(0);
    buf.truncate(n);

    // Binary file detection: NUL byte in first 512 bytes or non-UTF8 binary extension
    let inspect_len = buf.len().min(512);
    let has_nul = buf[..inspect_len].contains(&0);
    if has_nul || is_known_binary_ext(&ext) {
        return ResolvedPreview {
            title,
            meta_line,
            body: format!(
                "Binary / Media File\n\nPath:\n{path_str}\n\nSize: {size_str} ({size} bytes)\nModified: {modified}\n\nTip: Press Enter to open · Tab for Explorer / Terminal · Drag row to drop elsewhere"
            ),
            is_monospace: false,
            image_rgba: None,
            badges: Vec::new(),
        };
    }

    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text
        .lines()
        .take(MAX_PREVIEW_LINES)
        .map(|l| l.replace('\t', "  "))
        .collect();
    if size as usize > MAX_PREVIEW_READ_BYTES || text.lines().count() > MAX_PREVIEW_LINES {
        lines.push("... (preview truncated)".into());
    }

    ResolvedPreview {
        title,
        meta_line,
        body: lines.join("\n"),
        is_monospace: true,
        image_rgba: None,
        badges: Vec::new(),
    }
}

fn is_known_binary_ext(ext: &str) -> bool {
    matches!(
        ext,
        "exe"
            | "dll"
            | "pdb"
            | "lnk"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "bmp"
            | "ico"
            | "mp4"
            | "mkv"
            | "mp3"
            | "wav"
            | "zip"
            | "7z"
            | "rar"
            | "tar"
            | "gz"
            | "pdf"
            | "docx"
            | "xlsx"
            | "pptx"
            | "wasm"
            | "db"
            | "sqlite"
    )
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}
