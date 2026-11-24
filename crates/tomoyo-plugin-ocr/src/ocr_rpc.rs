//! Protocol v3 Hybrid RPC backend for the standalone Offline OCR Inspector WebView (`ui/index.html`).

use crate::ocr_engine::{tomoyo_data_dir, OcrOutput};
use base64::Engine;
use serde_json::{json, Value};
use std::path::Path;

const MAX_INLINE_IMG_BYTES: u64 = 10 * 1024 * 1024;

pub fn persist_last_ocr_snapshot(img_path: &Path, out: &OcrOutput) {
    let dir = tomoyo_data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let snap = json!({
        "image_path": img_path.to_string_lossy(),
        "text": out.text,
        "raw_text": out.raw_text,
        "lang_tag": out.lang_tag,
        "lang_note": out.lang_note,
        "lines_count": out.lines.len(),
        "chars_count": out.text.chars().count(),
        "src_dims": [out.src_dims.0, out.src_dims.1],
        "proc_dims": [out.proc_dims.0, out.proc_dims.1],
        "elapsed_ms": out.elapsed_ms,
    });
    let _ = std::fs::write(dir.join("last_ocr.json"), snap.to_string());
}

/// Loads the persisted `logs/last_ocr.json` snapshot and attaches a bounded `data:image/...;base64,...`
/// URI for the original image so the Hybrid WebView requires zero host-side custom URI schemes.
pub fn load_last_ocr_payload(fallback_img: &str) -> Value {
    let snap_path = tomoyo_data_dir().join("logs").join("last_ocr.json");
    let mut snap: Value = std::fs::read_to_string(&snap_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));

    let img_path = snap
        .get("image_path")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback_img)
        .to_string();

    let image_url = encode_image_data_uri(Path::new(&img_path));
    if let Some(obj) = snap.as_object_mut() {
        obj.insert("image_path".into(), json!(img_path));
        obj.insert("image_url".into(), json!(image_url));
    }
    snap
}

fn encode_image_data_uri(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_INLINE_IMG_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        _ => "image/png",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_local_png_as_data_uri_for_hybrid_webview() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_ocr_rpc_{}.png", std::process::id()));
        std::fs::write(&tmp, [0x89u8, b'P', b'N', b'G']).unwrap();
        let uri = encode_image_data_uri(&tmp).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
        let _ = std::fs::remove_file(&tmp);
    }
}
