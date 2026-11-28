//! CLI `--recognize` entry point and fallback hint/error card builders for `tomoyo-plugin-ocr`.

use crate::ocr_engine::{available_ocr_languages, recognize_image, LangPref};
use crate::ocr_rpc;
use std::path::Path;
use tomoyo_sdk::{IconSpec, PreviewSpec, ResultCategory, ResultItem};

pub(crate) fn hint_item() -> ResultItem {
    let langs = available_ocr_languages().join(", ");
    ResultItem {
        id: "ocr:hint".into(),
        provider_id: "dev.tomoyo.ocr".into(),
        title: "离线 WinRT OCR 文字识别（支持免贴图直识最近截图）".into(),
        subtitle: "用法：截图后直接输入 ocr / ocr en / ocr zh，或 Ctrl+V 贴图后输入 ocr".into(),
        category: ResultCategory::Plugin,
        score: 9_100,
        icon: IconSpec::Monogram("OCR".into()),
        badge: Some("OCR".into()),
        copy_text: None,
        actions: vec![],
        preview: Some(PreviewSpec::KeyValue {
            title: "Windows Offline OCR".into(),
            entries: vec![
                ("Engine".into(), "Windows.Media.Ocr (ClearType 3x + Cubic)".into()),
                ("Idle RAM".into(), "0 MB (Auto-reaped after 15s)".into()),
                ("Languages".into(), if langs.is_empty() { "None detected".into() } else { langs }),
                ("Options".into(), "ocr | ocr en | ocr zh | ocr raw | ocr <path>".into()),
            ],
        }),
    }
}

pub(crate) fn error_item(msg: &str) -> ResultItem {
    ResultItem {
        id: "ocr:err".into(),
        provider_id: "dev.tomoyo.ocr".into(),
        title: "OCR 识别提示".into(),
        subtitle: msg.to_string(),
        category: ResultCategory::Plugin,
        score: 9_000,
        icon: IconSpec::Monogram("OCR".into()),
        badge: Some("ERR".into()),
        copy_text: Some(msg.to_string()),
        actions: vec![],
        preview: Some(PreviewSpec::Text {
            title: "OCR 异常详情".into(),
            body: msg.to_string(),
        }),
    }
}

pub(crate) fn run_recognize_cli(args: &[String]) {
    let raw_mode = args.iter().any(|a| a == "--raw");
    let json_mode = args.iter().any(|a| a == "--json");
    let lang_flag = args.windows(2).find(|w| w[0] == "--lang").map(|w| w[1].to_ascii_lowercase());
    let pref = if args.iter().any(|a| a == "--en") || lang_flag.as_deref() == Some("en") {
        LangPref::En
    } else if args.iter().any(|a| a == "--zh") || lang_flag.as_deref() == Some("zh") {
        LangPref::Zh
    } else {
        LangPref::Auto
    };
    let img_path = Path::new(&args[1]);
    match recognize_image(img_path, pref, raw_mode) {
        Ok(out) => {
            ocr_rpc::persist_last_ocr_snapshot(img_path, &out);
            if json_mode {
                let snap = serde_json::json!({
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
                println!("{snap}");
            } else {
                println!(
                    "[OCR {} | {} line(s) | {}x{}->{}x{} | {} ms]\n{}",
                    out.lang_tag,
                    out.lines.len(),
                    out.src_dims.0,
                    out.src_dims.1,
                    out.proc_dims.0,
                    out.proc_dims.1,
                    out.elapsed_ms,
                    out.text
                );
            }
        }
        Err(e) => {
            eprintln!("[OCR ERROR] {e}");
            std::process::exit(1);
        }
    }
}
