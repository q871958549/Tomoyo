//! Ephemeral Split-Pane OCR Inspector & Editor bootstrap (`__tomoyo_ocr_viewer__`).

use std::path::{Path, PathBuf};

pub const OCR_VIEWER_ENTRY: &str = "__tomoyo_ocr_viewer__";

const OCR_VIEWER_HTML_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ocr.html.gz"));

pub fn is_ocr_viewer_entry(entry: &str) -> bool {
    entry == OCR_VIEWER_ENTRY
}

pub fn assemble_ocr_viewer_html() -> String {
    crate::inflate_embedded_html(OCR_VIEWER_HTML_GZ)
}

pub struct OcrViewerBootstrap {
    pub init_script: String,
    pub image_dir: Option<PathBuf>,
}

/// Reads `%LOCALAPPDATA%\Tomoyo\logs\last_ocr.json`, resolves the image parent directory for
/// virtual-host mapping (`https://tomoyo-ocr-img.local/<file>`), and builds the injected JS data.
pub fn load_ocr_viewer_bootstrap(theme: &str, fallback_img_path: &str) -> OcrViewerBootstrap {
    let snap_path = tomoyo_core::logs_dir().join("last_ocr.json");
    let mut snap: serde_json::Value = std::fs::read_to_string(&snap_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let img_path_str = snap
        .get("image_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_img_path)
        .to_string();

    let (image_dir, image_url) = resolve_image_virtual_host(&img_path_str);

    if let Some(obj) = snap.as_object_mut() {
        obj.insert("theme".into(), serde_json::json!(theme));
        obj.insert("image_path".into(), serde_json::json!(img_path_str));
        obj.insert("image_url".into(), serde_json::json!(image_url));
    }

    let init_script = format!(
        "window.__TOMOYO_OCR_DATA__ = {};",
        serde_json::to_string(&snap).unwrap_or_else(|_| "{}".into())
    );
    OcrViewerBootstrap {
        init_script,
        image_dir,
    }
}

fn resolve_image_virtual_host(img_path: &str) -> (Option<PathBuf>, Option<String>) {
    if img_path.is_empty() {
        return (None, None);
    }
    let p = Path::new(img_path);
    let Some(parent) = p.parent().filter(|d| d.is_dir()) else {
        return (None, None);
    };
    let Some(fname) = p.file_name().and_then(|n| n.to_str()) else {
        return (None, None);
    };
    let encoded = percent_encode_segment(fname);
    let scheme = if cfg!(target_os = "linux") {
        "tomoyo-ocr-img://local"
    } else {
        "https://tomoyo-ocr-img.local"
    };
    (
        Some(parent.to_path_buf()),
        Some(format!("{scheme}/{encoded}")),
    )
}

fn percent_encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_non_ascii_filenames_for_virtual_host() {
        assert_eq!(percent_encode_segment("截图 1.png"), "%E6%88%AA%E5%9B%BE%201.png");
        assert_eq!(percent_encode_segment("img_27c8.png"), "img_27c8.png");
    }

    #[test]
    fn ocr_viewer_html_contains_expected_bootstrap_hooks() {
        let html = assemble_ocr_viewer_html();
        assert!(html.contains("__TOMOYO_OCR_DATA__"));
        assert!(html.contains("copy_text"));
        assert!(html.contains("report_js_error"));
    }
}
