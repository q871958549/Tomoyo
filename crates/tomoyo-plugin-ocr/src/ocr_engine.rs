//! Offline Windows WinRT `OcrEngine` wrapper with adaptive Cubic upscaling,
//! language selection, CJK inter-character space cleanup, and latest clipboard image lookup.

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangPref {
    Auto,
    En,
    Zh,
}

#[derive(Debug, Clone)]
pub struct OcrOutput {
    pub text: String,
    pub raw_text: String,
    pub lines: Vec<String>,
    pub lang_tag: String,
    pub src_dims: (u32, u32),
    pub proc_dims: (u32, u32),
    pub elapsed_ms: u64,
    pub lang_note: Option<String>,
}

/// Returns `%LOCALAPPDATA%\Tomoyo` on Windows or `~/.tomoyo` elsewhere.
pub fn tomoyo_data_dir() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Tomoyo");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".tomoyo");
    }
    PathBuf::from(".tomoyo")
}

/// Finds the most recently saved clipboard screenshot under `%LOCALAPPDATA%\Tomoyo\clipboard-images`
/// so users can run `ocr` immediately after taking a screenshot without needing `Ctrl+V` first.
pub fn find_latest_clipboard_image() -> Option<PathBuf> {
    let dir = tomoyo_data_dir().join("clipboard-images");
    let rd = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for ent in rd.flatten() {
        let path = ent.path();
        if !path.is_file() || !is_image_ext(&path) {
            continue;
        }
        let mtime = ent
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, p)| p)
}

pub fn is_image_ext(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif" | "tif" | "tiff"
            )
        })
}

/// Removes spurious spaces that Windows `zh-Hans-CN` OCR inserts between adjacent CJK characters
/// and normalizes full-width `．`/`：`/`／` inside numeric/IP/time tokens while keeping ASCII word spacing.
pub fn clean_cjk_spaces(line: &str) -> String {
    let chars: Vec<char> = line.trim().chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(line.len());
    for i in 0..chars.len() {
        let ch = chars[i];
        let prev = out.chars().next_back();
        let next = chars[i + 1..].iter().copied().find(|&c| c != ' ');
        if ch == ' ' {
            let drop_space = match (prev, next) {
                (Some(p), Some(n)) => {
                    (is_cjk_or_punct(p) && is_cjk_or_punct(n))
                        || (is_cjk_punct(p) && n.is_ascii_alphanumeric())
                        || (p.is_ascii_alphanumeric() && is_cjk_punct(n))
                        || (p.is_ascii_digit() && matches!(n, '.' | ':' | '/'))
                        || (matches!(p, '.' | ':' | '/') && n.is_ascii_digit())
                }
                _ => true,
            };
            if !drop_space && prev != Some(' ') {
                out.push(' ');
            }
        } else {
            let between_digits = prev.is_some_and(|p| p.is_ascii_digit())
                && next.is_some_and(|n| n.is_ascii_digit());
            let mapped = if between_digits {
                match ch {
                    '．' => '.',
                    '：' => ':',
                    '／' => '/',
                    _ => ch,
                }
            } else {
                ch
            };
            out.push(mapped);
        }
    }
    out
}

fn is_cjk_or_punct(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{3000}'..='\u{303F}' | '\u{FF01}'..='\u{FF5E}')
}

fn is_cjk_punct(c: char) -> bool {
    matches!(
        c,
        '，' | '。' | '；' | '：' | '！' | '？' | '（' | '）' | '【' | '】' | '《' | '》' | '、' | '“' | '”' | '‘' | '’' | '．' | '／'
    )
}

#[cfg(windows)]
pub fn available_ocr_languages() -> Vec<String> {
    use windows::Media::Ocr::OcrEngine;
    let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0..list.Size().unwrap_or(0) {
        if let Ok(lang) = list.GetAt(i) {
            if let Ok(tag) = lang.LanguageTag() {
                out.push(tag.to_string());
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn available_ocr_languages() -> Vec<String> {
    Vec::new()
}

#[cfg(windows)]
pub fn recognize_image(
    path: &Path,
    pref: LangPref,
    raw_mode: bool,
) -> Result<OcrOutput, String> {
    use windows::core::{h, HSTRING};
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat,
        BitmapTransform, ColorManagementMode, ExifOrientationMode,
    };
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    let started = Instant::now();
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

    let src_w = decoder.PixelWidth().unwrap_or(0);
    let src_h = decoder.PixelHeight().unwrap_or(0);
    let max_dim = OcrEngine::MaxImageDimension().unwrap_or(2600).max(1024);

    // Upscale low-DPI / small-font screenshots up to 2x (bounded by WinRT MaxImageDimension)
    // and downscale oversized multi-monitor captures so OcrEngine stays accurate and never fails.
    let (proc_w, proc_h, use_transform) = compute_ocr_dimensions(src_w, src_h, max_dim, raw_mode);
    let bitmap = if use_transform {
        let transform = BitmapTransform::new().map_err(|e| format!("BitmapTransform 失败: {e}"))?;
        let _ = transform.SetScaledWidth(proc_w);
        let _ = transform.SetScaledHeight(proc_h);
        let _ = transform.SetInterpolationMode(BitmapInterpolationMode::Cubic);
        decoder
            .GetSoftwareBitmapTransformedAsync(
                BitmapPixelFormat::Bgra8,
                BitmapAlphaMode::Premultiplied,
                &transform,
                ExifOrientationMode::RespectExifOrientation,
                ColorManagementMode::DoNotColorManage,
            )
            .map_err(|e| format!("图像缩放解码调用失败: {e}"))?
            .join()
            .map_err(|e| format!("图像缩放解码失败: {e}"))?
    } else {
        decoder
            .GetSoftwareBitmapAsync()
            .map_err(|e| format!("获取位图失败: {e}"))?
            .join()
            .map_err(|e| format!("获取 SoftwareBitmap 失败: {e}"))?
    };

    let installed = available_ocr_languages();
    let mut lang_note = None;
    let engine = match pref {
        LangPref::En => {
            let en_tag = installed
                .iter()
                .find(|t| t.to_ascii_lowercase().starts_with("en"))
                .cloned();
            if let Some(tag) = en_tag {
                let htag = HSTRING::from(&tag);
                Language::CreateLanguage(&htag)
                    .and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
                    .map_err(|e| format!("初始化英文 OCR 引擎失败: {e}"))?
            } else {
                lang_note = Some(format!(
                    "系统尚未安装英文 OCR 语言包（当前仅 {:?}），已回退默认引擎。可在 Windows 设置添加「英语(美国)」语言包提升纯英文表格识别率。",
                    installed
                ));
                OcrEngine::TryCreateFromUserProfileLanguages()
                    .or_else(|_| {
                        Language::CreateLanguage(h!("zh-Hans-CN"))
                            .and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
                    })
                    .map_err(|e| format!("未找到可用的 Windows OCR 语言包: {e}"))?
            }
        }
        LangPref::Zh => Language::CreateLanguage(h!("zh-Hans-CN"))
            .and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
            .or_else(|_| OcrEngine::TryCreateFromUserProfileLanguages())
            .map_err(|e| format!("未找到中文 OCR 语言包: {e}"))?,
        LangPref::Auto => OcrEngine::TryCreateFromUserProfileLanguages()
            .or_else(|_| {
                Language::CreateLanguage(h!("zh-Hans-CN"))
                    .and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
            })
            .or_else(|_| {
                Language::CreateLanguage(h!("en-US"))
                    .and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
            })
            .map_err(|e| format!("未找到可用的 Windows OCR 语言包: {e}"))?,
    };

    let lang_tag = engine
        .RecognizerLanguage()
        .and_then(|l| l.LanguageTag())
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "OCR".into());

    let res = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| format!("OCR 识别调用失败: {e}"))?
        .join()
        .map_err(|e| format!("OCR 识别失败: {e}"))?;

    let mut raw_lines = Vec::new();
    if let Ok(line_list) = res.Lines() {
        let n = line_list.Size().unwrap_or(0);
        raw_lines.reserve(n as usize);
        for i in 0..n {
            if let Ok(ln) = line_list.GetAt(i) {
                if let Ok(txt) = ln.Text() {
                    let s = txt.to_string();
                    if !s.trim().is_empty() {
                        raw_lines.push(s);
                    }
                }
            }
        }
    }
    if raw_lines.is_empty() {
        if let Ok(full) = res.Text() {
            let s = full.to_string();
            if !s.trim().is_empty() {
                raw_lines.push(s);
            }
        }
    }

    let raw_text = raw_lines.join("\r\n");
    let lines: Vec<String> = if raw_mode {
        raw_lines
    } else {
        raw_lines.iter().map(|l| clean_cjk_spaces(l)).collect()
    };
    let text = lines.join("\r\n");
    let elapsed_ms = started.elapsed().as_millis() as u64;

    Ok(OcrOutput {
        text,
        raw_text,
        lines,
        lang_tag,
        src_dims: (src_w, src_h),
        proc_dims: (proc_w, proc_h),
        elapsed_ms,
        lang_note,
    })
}

#[cfg(not(windows))]
pub fn recognize_image(
    _path: &Path,
    _pref: LangPref,
    _raw_mode: bool,
) -> Result<OcrOutput, String> {
    Err("Windows.Media.Ocr is only available on Windows 10/11".into())
}

pub fn compute_ocr_dimensions(src_w: u32, src_h: u32, max_dim: u32, raw_mode: bool) -> (u32, u32, bool) {
    if src_w == 0 || src_h == 0 || max_dim == 0 {
        return (src_w, src_h, false);
    }
    let max_side = src_w.max(src_h);
    if max_side > max_dim {
        let scale = max_dim as f64 / max_side as f64;
        return (
            ((src_w as f64 * scale).round() as u32).max(1),
            ((src_h as f64 * scale).round() as u32).max(1),
            true,
        );
    }
    if raw_mode {
        return (src_w, src_h, false);
    }
    let scale = (max_dim as f64 / max_side as f64).min(2.0);
    if scale >= 1.2 {
        (
            ((src_w as f64 * scale).round() as u32).min(max_dim).max(1),
            ((src_h as f64 * scale).round() as u32).min(max_dim).max(1),
            true,
        )
    } else {
        (src_w, src_h, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_spurious_cjk_spaces_while_preserving_english_words() {
        assert_eq!(clean_cjk_spaces("离 线 识 图 取 字"), "离线识图取字");
        assert_eq!(clean_cjk_spaces("Security Posture Awareness"), "Security Posture Awareness");
        assert_eq!(clean_cjk_spaces("华为 防 火 墙 192.168.1.7"), "华为防火墙 192.168.1.7");
        assert_eq!(clean_cjk_spaces("192 ． 168 ． 1 ． 7：60186"), "192.168.1.7:60186");
        assert_eq!(clean_cjk_spaces("2026 ／ 11 ／ 20 14：49：51"), "2026/11/20 14:49:51");
        assert_eq!(clean_cjk_spaces("内存 34 ． 2 MB"), "内存 34.2 MB");
    }

    #[test]
    fn computes_bounded_upscale_and_downscale_dimensions() {
        assert_eq!(compute_ocr_dimensions(1920, 953, 4096, false), (3840, 1906, true));
        assert_eq!(compute_ocr_dimensions(1920, 953, 2600, false), (2600, 1291, true));
        assert_eq!(compute_ocr_dimensions(1920, 953, 4096, true), (1920, 953, false));
        assert_eq!(compute_ocr_dimensions(5120, 2160, 4096, false), (4096, 1728, true));
    }
}
