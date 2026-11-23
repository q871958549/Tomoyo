//! On-demand offline Windows WinRT OCR plugin (`ocr` trigger & Super Match image OCR).
//!
//! Runs inside a Windows Job Object with a 15-second idle timeout so background RAM stays at
//! 0 MB when idle, while providing adaptive upscaling, CJK space cleanup, single-line copies,
//! and an ephemeral split-pane OCR Inspector/Editor view.

mod ocr_engine;
#[cfg(not(windows))]
mod ocr_linux;
mod ocr_preprocess;

use ocr_engine::{
    available_ocr_languages, find_latest_clipboard_image, is_image_ext, recognize_image,
    tomoyo_data_dir, LangPref, OcrOutput,
};
use std::path::{Path, PathBuf};
use tomoyo_sdk::{
    ActionKind, ActionSpec, AttachedContext, ExecuteRequest, ExecuteResponse, IconSpec, PluginMeta,
    PreviewSpec, QueryContext, ResultCategory, ResultItem, TomoyoPlugin,
};

struct OcrPlugin;

impl TomoyoPlugin for OcrPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("Offline Windows OCR", env!("CARGO_PKG_VERSION"))
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let (pref, raw_mode, explicit_path) = resolve_ocr_options(ctx);
        let Some((img_path, source_label)) = resolve_target_image(ctx, explicit_path.as_deref()) else {
            return vec![hint_item()];
        };
        if !img_path.is_file() {
            return vec![error_item(&format!("图片文件不存在：{}", img_path.display()))];
        }

        match recognize_image(&img_path, pref, raw_mode) {
            Ok(out) => {
                eprintln!(
                    "[ocr] source={source_label} dims={}x{}->{}x{} lang={} lines={} chars={} ms={}",
                    out.src_dims.0,
                    out.src_dims.1,
                    out.proc_dims.0,
                    out.proc_dims.1,
                    out.lang_tag,
                    out.lines.len(),
                    out.text.chars().count(),
                    out.elapsed_ms
                );
                if out.text.trim().is_empty() {
                    return vec![error_item("未在图片中识别到可读文本（可尝试放大截图或切换 `ocr en`）")];
                }
                persist_last_ocr_snapshot(&img_path, &out);
                build_ocr_result_items(&img_path, &out, source_label)
            }
            Err(err) => {
                eprintln!("[ocr] error path={} detail={err}", img_path.display());
                vec![error_item(&err)]
            }
        }
    }

    fn execute(&self, _req: &ExecuteRequest) -> ExecuteResponse {
        ExecuteResponse::default()
    }
}

fn resolve_ocr_options(ctx: &QueryContext) -> (LangPref, bool, Option<String>) {
    let (mut pref, mut raw_mode, explicit_path) = parse_ocr_args(&ctx.args);
    let tokens: Vec<String> = ctx.args.split_whitespace().map(|s| s.to_ascii_lowercase()).collect();
    let has_lang_arg = tokens.iter().any(|t| matches!(t.as_str(), "en" | "eng" | "english" | "zh" | "cn" | "chinese"));
    if !has_lang_arg {
        match ctx.plugin_settings.get("default_lang").and_then(|v| v.as_str()) {
            Some("zh") => pref = LangPref::Zh,
            Some("en") => pref = LangPref::En,
            _ => {}
        }
    }
    if !raw_mode && ctx.plugin_settings.get("raw_output").and_then(|v| v.as_bool()) == Some(true) {
        raw_mode = true;
    }
    (pref, raw_mode, explicit_path)
}

fn parse_ocr_args(raw: &str) -> (LangPref, bool, Option<String>) {
    let mut pref = LangPref::Auto;
    let mut raw_mode = false;
    let mut rest_parts: Vec<&str> = Vec::new();
    for tok in raw.split_whitespace() {
        match tok.to_ascii_lowercase().as_str() {
            "en" | "eng" | "english" if rest_parts.is_empty() => pref = LangPref::En,
            "zh" | "cn" | "chinese" if rest_parts.is_empty() => pref = LangPref::Zh,
            "raw" if rest_parts.is_empty() => raw_mode = true,
            _ => rest_parts.push(tok),
        }
    }
    let path_arg = rest_parts.join(" ");
    let cleaned = path_arg.trim().trim_matches('"').to_string();
    (pref, raw_mode, (!cleaned.is_empty()).then_some(cleaned))
}

fn resolve_target_image(ctx: &QueryContext, explicit_path: Option<&str>) -> Option<(PathBuf, &'static str)> {
    if let Some(p) = explicit_path {
        return Some((PathBuf::from(p), "路径参数"));
    }
    if let Some(att) = ctx.attached.as_ref() {
        match att {
            AttachedContext::Image { path, .. } => return Some((PathBuf::from(path), "粘贴图片")),
            AttachedContext::Files { paths } => {
                if let Some(p) = paths.iter().map(PathBuf::from).find(|p| is_image_ext(p)) {
                    return Some((p, "粘贴文件"));
                }
            }
        }
    }
    find_latest_clipboard_image().map(|p| (p, "最近剪贴板截图"))
}

fn persist_last_ocr_snapshot(img_path: &Path, out: &OcrOutput) {
    let dir = tomoyo_data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
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
    let _ = std::fs::write(dir.join("last_ocr.json"), snap.to_string());
}

fn build_ocr_result_items(img_path: &Path, out: &OcrOutput, source_label: &str) -> Vec<ResultItem> {
    let lines_n = out.lines.len();
    let chars_n = out.text.chars().count();
    let merged_single: String = out
        .lines
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let preview_body = match &out.lang_note {
        Some(note) => format!("⚠ {note}\n\n{}", out.text),
        None => out.text.clone(),
    };
    let scale_tag = if out.proc_dims != out.src_dims {
        format!("{}×{}→{}×{}", out.src_dims.0, out.src_dims.1, out.proc_dims.0, out.proc_dims.1)
    } else {
        format!("{}×{}", out.src_dims.0, out.src_dims.1)
    };

    let mut items = Vec::with_capacity(8);

    // Card 1: Ephemeral Split-Pane OCR Inspector/Editor Window (0 idle RAM)
    items.push(ResultItem {
        id: "ocr:viewer".into(),
        provider_id: "dev.tomoyo.ocr".into(),
        title: format!("打开 OCR 审校与编辑窗口（{lines_n} 行 · {chars_n} 字）"),
        subtitle: format!(
            "{source_label} · {scale_tag} · {} · {} ms · 原图对照 / 自由划选复制 / 去换行",
            out.lang_tag, out.elapsed_ms
        ),
        category: ResultCategory::Plugin,
        score: 10_800,
        icon: IconSpec::Monogram("OCR".into()),
        badge: Some(out.lang_tag.clone()),
        copy_text: Some(out.text.clone()),
        actions: vec![
            ActionSpec {
                id: "open_viewer".into(),
                label: "打开原图对照与可编辑窗口".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::OpenWebView {
                    plugin_id: "dev.tomoyo.ocr".into(),
                    title: "Tomoyo OCR 审校窗口".into(),
                    entry_path: "__tomoyo_ocr_viewer__".into(),
                    utools_compat: false,
                    initial_query: img_path.to_string_lossy().into_owned(),
                    width: 1080,
                    height: 680,
                    max_memory_mb: 256,
                    icon_path: None,
                    display_mode: None,
                },
            },
            ActionSpec {
                id: "copy_all".into(),
                label: "复制全部识别文本".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText {
                    text: out.text.clone(),
                    paste_immediately: false,
                },
            },
            ActionSpec {
                id: "copy_single_line".into(),
                label: "合并为单行并复制".into(),
                shortcut: Some("Shift+Enter".into()),
                kind: ActionKind::CopyText {
                    text: merged_single.clone(),
                    paste_immediately: false,
                },
            },
        ],
        preview: Some(PreviewSpec::Text {
            title: format!("OCR 识别全文 ({lines_n} 行 · {})", out.lang_tag),
            body: preview_body.clone(),
        }),
    });

    // Card 2: Direct Copy / Paste Full Recognized Text
    let first_snippet: String = merged_single.chars().take(60).collect();
    items.push(ResultItem {
        id: "ocr:copy_all".into(),
        provider_id: "dev.tomoyo.ocr".into(),
        title: format!("复制全文：{first_snippet}"),
        subtitle: "Enter 复制到剪贴板 · Ctrl+Enter 直接贴回原窗口 · Shift+Enter 复制单行".into(),
        category: ResultCategory::Clipboard,
        score: 10_600,
        icon: IconSpec::Monogram("TXT".into()),
        badge: Some(format!("{lines_n} 行")),
        copy_text: Some(out.text.clone()),
        actions: vec![
            ActionSpec {
                id: "copy_only".into(),
                label: "复制全部文本到剪贴板".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText {
                    text: out.text.clone(),
                    paste_immediately: false,
                },
            },
            ActionSpec {
                id: "paste_all".into(),
                label: "复制并直接粘贴到原窗口".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText {
                    text: out.text.clone(),
                    paste_immediately: true,
                },
            },
            ActionSpec {
                id: "copy_merged".into(),
                label: "去除换行合并为单行复制".into(),
                shortcut: Some("Shift+Enter".into()),
                kind: ActionKind::CopyText {
                    text: merged_single,
                    paste_immediately: false,
                },
            },
        ],
        preview: Some(PreviewSpec::Text {
            title: format!("OCR 识别全文 ({lines_n} 行 · {})", out.lang_tag),
            body: preview_body,
        }),
    });

    // Cards 3..8: Top individual recognized lines for 1-keystroke single-line copy
    let mut seen = std::collections::HashSet::new();
    for (idx, line) in out
        .lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| l.chars().count() >= 3 && seen.insert((*l).to_string()))
        .take(6)
        .enumerate()
    {
        items.push(ResultItem {
            id: format!("ocr:line:{idx}"),
            provider_id: "dev.tomoyo.ocr".into(),
            title: line.chars().take(76).collect(),
            subtitle: format!("第 {} 条识别行 · Enter 复制该行 · Ctrl+Enter 直贴该行", idx + 1),
            category: ResultCategory::Clipboard,
            score: 10_400 - (idx as i64 * 10),
            icon: IconSpec::Monogram("LN".into()),
            badge: Some(format!("L{}", idx + 1)),
            copy_text: Some(line.to_string()),
            actions: vec![
                ActionSpec {
                    id: "copy_line".into(),
                    label: "复制该行".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::CopyText {
                        text: line.to_string(),
                        paste_immediately: false,
                    },
                },
                ActionSpec {
                    id: "paste_line".into(),
                    label: "直贴该行到原窗口".into(),
                    shortcut: Some("Ctrl+Enter".into()),
                    kind: ActionKind::CopyText {
                        text: line.to_string(),
                        paste_immediately: true,
                    },
                },
            ],
            preview: Some(PreviewSpec::Text {
                title: format!("单行内容 (#{})", idx + 1),
                body: line.to_string(),
            }),
        });
    }

    items
}

fn hint_item() -> ResultItem {
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
                ("Engine".into(), "Windows.Media.Ocr (Adaptive 2x Cubic)".into()),
                ("Idle RAM".into(), "0 MB (Auto-reaped after 15s)".into()),
                ("Languages".into(), if langs.is_empty() { "None detected".into() } else { langs }),
                ("Options".into(), "ocr | ocr en | ocr zh | ocr raw | ocr <path>".into()),
            ],
        }),
    }
}

fn error_item(msg: &str) -> ResultItem {
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--self-test") {
        let langs = available_ocr_languages();
        let engine_kind = if cfg!(windows) { "WinRT" } else { "Tesseract" };
        println!("[OK] tomoyo-plugin-ocr {engine_kind} languages: {:?}", langs);
        return;
    }
    if args.len() >= 2 && args[0] == "--recognize" {
        let raw_mode = args.iter().any(|a| a == "--raw");
        let json_mode = args.iter().any(|a| a == "--json");
        let pref = if args.iter().any(|a| a == "--en") {
            LangPref::En
        } else if args.iter().any(|a| a == "--zh") {
            LangPref::Zh
        } else {
            LangPref::Auto
        };
        let img_path = Path::new(&args[1]);
        match recognize_image(img_path, pref, raw_mode) {
            Ok(out) => {
                persist_last_ocr_snapshot(img_path, &out);
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
        return;
    }
    tomoyo_sdk::run(OcrPlugin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_available_ocr_languages_without_panicking() {
        let langs = available_ocr_languages();
        assert!(!langs.is_empty() || cfg!(not(windows)));
    }

    #[test]
    fn parses_ocr_language_and_raw_flags() {
        assert_eq!(parse_ocr_args("en"), (LangPref::En, false, None));
        assert_eq!(parse_ocr_args("raw C:\\a.png"), (LangPref::Auto, true, Some("C:\\a.png".into())));
        assert_eq!(parse_ocr_args("zh \"D:\\my shot.png\""), (LangPref::Zh, false, Some("D:\\my shot.png".into())));
        let mut ctx = QueryContext::new("ocr", 1, 10);
        ctx.args.clear();
        ctx.plugin_settings.insert("default_lang".into(), serde_json::json!("en"));
        ctx.plugin_settings.insert("raw_output".into(), serde_json::json!(true));
        assert_eq!(resolve_ocr_options(&ctx), (LangPref::En, true, None));
    }
}
