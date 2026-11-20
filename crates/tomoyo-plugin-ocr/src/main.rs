//! On-demand offline Windows WinRT OCR plugin (`ocr` trigger & Super Match image OCR).
//!
//! Runs inside a Windows Job Object with a 15-second idle timeout so background RAM stays at
//! strictly 0 MB when not recognizing text, while leveraging OS-native `Windows.Media.Ocr`.

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
        let Some(img_path) = resolve_target_image(ctx) else {
            return vec![hint_item()];
        };
        if !img_path.is_file() {
            return vec![error_item(&format!("文件不存在：{}", img_path.display()))];
        }

        match recognize_image_path(&img_path) {
            Ok((text, lines_count, lang_tag)) => {
                if text.trim().is_empty() {
                    return vec![error_item("未在图片中识别到可读文本")];
                }
                let one_line: String = text
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                vec![ResultItem {
                    id: "ocr:result".into(),
                    provider_id: "dev.tomoyo.ocr".into(),
                    title: one_line.chars().take(72).collect(),
                    subtitle: format!(
                        "识别完成 · {lines_count} 行 · {lang_tag} · 回车直贴原文"
                    ),
                    category: ResultCategory::Plugin,
                    score: 10_600,
                    icon: IconSpec::Monogram("OCR".into()),
                    badge: Some(lang_tag),
                    copy_text: Some(text.clone()),
                    actions: vec![
                        ActionSpec {
                            id: "paste_ocr".into(),
                            label: "复制并直接粘贴识别文本".into(),
                            shortcut: Some("Enter".into()),
                            kind: ActionKind::CopyText {
                                text: text.clone(),
                                paste_immediately: true,
                            },
                        },
                        ActionSpec {
                            id: "copy_ocr".into(),
                            label: "仅复制识别文本到剪贴板".into(),
                            shortcut: Some("Ctrl+Enter".into()),
                            kind: ActionKind::CopyText {
                                text: text.clone(),
                                paste_immediately: false,
                            },
                        },
                    ],
                    preview: Some(PreviewSpec::Text {
                        title: format!("OCR 识别结果 ({lines_count} 行)"),
                        body: text,
                    }),
                }]
            }
            Err(err) => vec![error_item(&err)],
        }
    }

    fn execute(&self, _req: &ExecuteRequest) -> ExecuteResponse {
        ExecuteResponse::default()
    }
}

fn resolve_target_image(ctx: &QueryContext) -> Option<PathBuf> {
    let arg = ctx.args.trim().trim_matches('"');
    if !arg.is_empty() {
        return Some(PathBuf::from(arg));
    }
    match ctx.attached.as_ref()? {
        AttachedContext::Image { path, .. } => Some(PathBuf::from(path)),
        AttachedContext::Files { paths } => paths
            .iter()
            .map(PathBuf::from)
            .find(|p| is_image_ext(p)),
    }
}

fn is_image_ext(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "bmp" | "webp"))
}

fn hint_item() -> ResultItem {
    let langs = available_ocr_languages().join(", ");
    ResultItem {
        id: "ocr:hint".into(),
        provider_id: "dev.tomoyo.ocr".into(),
        title: "离线 WinRT OCR 文字识别（0 常驻内存）".into(),
        subtitle: "用法：先 Ctrl+V 粘贴截图然后输入 ocr，或输入 `ocr C:\\path\\to\\img.png`".into(),
        category: ResultCategory::Plugin,
        score: 9_100,
        icon: IconSpec::Monogram("OCR".into()),
        badge: Some("OCR".into()),
        copy_text: None,
        actions: vec![],
        preview: Some(PreviewSpec::KeyValue {
            title: "Windows Offline OCR".into(),
            entries: vec![
                ("Engine".into(), "Windows.Media.Ocr (Native WinRT)".into()),
                ("Idle RAM".into(), "0 MB (Auto-reaped after 15s)".into()),
                ("Languages".into(), if langs.is_empty() { "None detected".into() } else { langs }),
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
        copy_text: None,
        actions: vec![],
        preview: None,
    }
}

#[cfg(windows)]
fn available_ocr_languages() -> Vec<String> {
    use windows::Media::Ocr::OcrEngine;
    let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let count = list.Size().unwrap_or(0);
    for i in 0..count {
        if let Ok(lang) = list.GetAt(i) {
            if let Ok(tag) = lang.LanguageTag() {
                out.push(tag.to_string());
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn available_ocr_languages() -> Vec<String> {
    Vec::new()
}

#[cfg(windows)]
fn recognize_image_path(path: &Path) -> Result<(String, usize, String), String> {
    use windows::core::{h, HSTRING};
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let clean = abs.to_string_lossy();
    let win_path = clean.strip_prefix(r"\\?\").unwrap_or(&clean);
    let hpath = HSTRING::from(win_path);

    let file = StorageFile::GetFileFromPathAsync(&hpath)
        .map_err(|e| format!("打开图片失败: {e}"))?
        .join()
        .map_err(|e| format!("读取 StorageFile 失败: {e}"))?;
    let stream = file
        .OpenAsync(FileAccessMode::Read)
        .map_err(|e| format!("打开文件流失败: {e}"))?
        .join()
        .map_err(|e| format!("读取图像流失败: {e}"))?;
    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| format!("图像解码器初始化失败: {e}"))?
        .join()
        .map_err(|e| format!("图像解码失败: {e}"))?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| format!("获取位图失败: {e}"))?
        .join()
        .map_err(|e| format!("获取 SoftwareBitmap 失败: {e}"))?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .or_else(|_| {
            Language::CreateLanguage(h!("zh-Hans-CN"))
                .and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
        })
        .or_else(|_| {
            Language::CreateLanguage(h!("en-US"))
                .and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
        })
        .map_err(|e| format!("未找到可用的 Windows OCR 语言包: {e}"))?;

    let tag = engine
        .RecognizerLanguage()
        .and_then(|l| l.LanguageTag())
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "OCR".into());

    let res = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| format!("OCR 识别调用失败: {e}"))?
        .join()
        .map_err(|e| format!("OCR 识别失败: {e}"))?;

    let line_count = res.Lines().map(|l| l.Size().unwrap_or(0) as usize).unwrap_or(0);
    let mut lines = Vec::with_capacity(line_count);
    if let Ok(line_list) = res.Lines() {
        for i in 0..line_list.Size().unwrap_or(0) {
            if let Ok(ln) = line_list.GetAt(i) {
                if let Ok(txt) = ln.Text() {
                    lines.push(txt.to_string());
                }
            }
        }
    }
    let joined = if lines.is_empty() {
        res.Text().map(|s| s.to_string()).unwrap_or_default()
    } else {
        lines.join("\r\n")
    };
    let cnt = lines.len().max(usize::from(!joined.is_empty()));
    Ok((joined, cnt, tag))
}

#[cfg(not(windows))]
fn recognize_image_path(_path: &Path) -> Result<(String, usize, String), String> {
    Err("Windows.Media.Ocr is only available on Windows 10/11".into())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--self-test") {
        let langs = available_ocr_languages();
        println!("[OK] tomoyo-plugin-ocr WinRT languages: {:?}", langs);
        return;
    }
    if args.len() >= 2 && args[0] == "--recognize" {
        match recognize_image_path(Path::new(&args[1])) {
            Ok((text, lines, tag)) => {
                println!("[OCR {tag} | {lines} line(s)]\n{text}");
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
}
