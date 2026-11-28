//! Offline Windows WinRT `OcrEngine` wrapper with adaptive Cubic upscaling,
//! language selection, CJK inter-character space cleanup, and latest clipboard image lookup.

pub use crate::ocr_postprocess::clean_cjk_spaces;
#[cfg(windows)]
use crate::ocr_postprocess::score_ocr_lines;
#[cfg(windows)]
use crate::ocr_preprocess::{
    build_cleartype_subpixel_bgra, build_high_contrast_bgra, compute_ocr_layout, estimate_bg_rgb,
    pad_bgra,
};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::Instant;
use std::time::SystemTime;

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

/// Returns `%LOCALAPPDATA%\Tomoyo` on Windows, or XDG (`$XDG_DATA_HOME/tomoyo`,
/// legacy `~/.tomoyo` if existing, else `~/.local/share/tomoyo`) on Linux.
pub fn tomoyo_data_dir() -> PathBuf {
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Tomoyo");
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
            && !xdg.trim().is_empty()
        {
            return PathBuf::from(xdg.trim()).join("tomoyo");
        }
        if let Ok(home) = std::env::var("HOME") {
            let legacy = PathBuf::from(&home).join(".tomoyo");
            if legacy.is_dir() {
                return legacy;
            }
            return PathBuf::from(home).join(".local").join("share").join("tomoyo");
        }
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

#[cfg(windows)]
pub fn available_ocr_languages() -> Vec<String> {
    use windows::Media::Ocr::OcrEngine;
    let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0..list.Size().unwrap_or(0) {
        if let Ok(lang) = list.GetAt(i)
            && let Ok(tag) = lang.LanguageTag() {
                out.push(tag.to_string());
            }
    }
    out
}

#[cfg(not(windows))]
pub fn available_ocr_languages() -> Vec<String> {
    crate::ocr_linux::available_ocr_languages_linux()
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
        BitmapTransform, ColorManagementMode, ExifOrientationMode, SoftwareBitmap,
    };
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
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

    let (scaled_w, scaled_h, pad, use_transform) =
        compute_ocr_layout(src_w, src_h, max_dim, raw_mode);
    let transform = BitmapTransform::new().map_err(|e| format!("BitmapTransform 失败: {e}"))?;
    if use_transform {
        let _ = transform.SetScaledWidth(scaled_w);
        let _ = transform.SetScaledHeight(scaled_h);
        let interp = if scaled_w > src_w || scaled_h > src_h {
            BitmapInterpolationMode::Cubic
        } else {
            BitmapInterpolationMode::Fant
        };
        let _ = transform.SetInterpolationMode(interp);
    }
    let px_provider = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )
        .map_err(|e| format!("图像像素解码调用失败: {e}"))?
        .join()
        .map_err(|e| format!("图像像素解码失败: {e}"))?;
    let base_bgra = px_provider
        .DetachPixelData()
        .map_err(|e| format!("提取像素缓冲失败: {e}"))?
        .to_vec();

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

    let mut lang_tag = engine
        .RecognizerLanguage()
        .and_then(|l| l.LanguageTag())
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "OCR".into());

    let run_with_engine = |eng: &OcrEngine, w: u32, h: u32, bytes: &[u8]| -> Result<Vec<String>, String> {
        let writer = DataWriter::new().map_err(|e| format!("DataWriter 失败: {e}"))?;
        writer.WriteBytes(bytes).map_err(|e| format!("写入位图缓冲失败: {e}"))?;
        let ibuf = writer.DetachBuffer().map_err(|e| format!("DetachBuffer 失败: {e}"))?;
        let bmp = SoftwareBitmap::CreateCopyFromBuffer(
            &ibuf,
            BitmapPixelFormat::Bgra8,
            w as i32,
            h as i32,
        )
        .map_err(|e| format!("创建 SoftwareBitmap 失败: {e}"))?;
        let res = eng
            .RecognizeAsync(&bmp)
            .map_err(|e| format!("OCR 识别调用失败: {e}"))?
            .join()
            .map_err(|e| format!("OCR 识别失败: {e}"))?;
        let mut out_lines = Vec::new();
        if let Ok(line_list) = res.Lines() {
            let n = line_list.Size().unwrap_or(0);
            out_lines.reserve(n as usize);
            for i in 0..n {
                if let Ok(ln) = line_list.GetAt(i)
                    && let Ok(txt) = ln.Text()
                {
                    let s = txt.to_string();
                    if !s.trim().is_empty() {
                        out_lines.push(s);
                    }
                }
            }
        }
        if out_lines.is_empty()
            && let Ok(full) = res.Text()
        {
            let s = full.to_string();
            if !s.trim().is_empty() {
                out_lines.push(s);
            }
        }
        Ok(out_lines)
    };

    let bg_rgb = estimate_bg_rgb(&base_bgra, scaled_w, scaled_h);
    let (pass_a_w, pass_a_h, pass_a_buf) = pad_bgra(&base_bgra, scaled_w, scaled_h, pad, bg_rgb);
    let mut best_lines = run_with_engine(&engine, pass_a_w, pass_a_h, &pass_a_buf)?;
    let mut best_score = score_ocr_lines(&best_lines);
    let (mut proc_w, mut proc_h) = (pass_a_w, pass_a_h);
    let mut best_buf = pass_a_buf;

    // Pass B: ClearType 3x RGB subpixel decode on unscaled UI crops eliminates horizontal character fusion.
    if !raw_mode
        && src_w > 0
        && src_h > 0
        && (src_w as u64) * (src_h as u64) <= 450_000
        && src_w * 3 + pad * 2 <= max_dim
        && src_h * 3 + pad * 2 <= max_dim
        && let Ok(raw_tf) = BitmapTransform::new()
        && let Ok(op) = decoder.GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Straight,
            &raw_tf,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )
        && let Ok(orig_px) = op.join()
        && let Ok(orig_data) = orig_px.DetachPixelData()
    {
        let orig_bytes = orig_data.to_vec();
        let orig_bg = estimate_bg_rgb(&orig_bytes, src_w, src_h);
        let (ct_w, ct_h, ct_buf) =
            build_cleartype_subpixel_bgra(&orig_bytes, src_w, src_h, pad, orig_bg);
        if let Ok(ct_lines) = run_with_engine(&engine, ct_w, ct_h, &ct_buf) {
            let ct_score = score_ocr_lines(&ct_lines);
            if (!ct_lines.is_empty() && best_lines.is_empty()) || ct_score > best_score {
                best_lines = ct_lines;
                best_score = ct_score;
                proc_w = ct_w;
                proc_h = ct_h;
                best_buf = ct_buf;
            }
        }
    }

    // Pass C: Chromatic high-contrast pass rescues low-contrast colored text on dark/tinted UI cards.
    if !raw_mode && (src_w as u64) * (src_h as u64) <= 1_600_000 {
        let (hc_w, hc_h, hc_buf) =
            build_high_contrast_bgra(&base_bgra, scaled_w, scaled_h, pad, bg_rgb);
        if let Ok(hc_lines) = run_with_engine(&engine, hc_w, hc_h, &hc_buf) {
            let hc_score = score_ocr_lines(&hc_lines);
            if (!hc_lines.is_empty() && best_lines.is_empty()) || hc_score > best_score {
                best_lines = hc_lines;
                best_score = hc_score;
                proc_w = hc_w;
                proc_h = hc_h;
                best_buf = hc_buf;
            }
        }
    }

    // Dual-language smart fallback: if Auto used a non-English model on an ASCII-dominant crop
    // and an English recognizer is installed, re-evaluate the winning bitmap cleanly.
    if pref == LangPref::Auto
        && !lang_tag.to_ascii_lowercase().starts_with("en")
        && let Some(en_tag) = installed.iter().find(|t| t.to_ascii_lowercase().starts_with("en"))
    {
        let total_non_ws = best_lines.iter().flat_map(|l| l.chars()).filter(|c| !c.is_whitespace()).count();
        let han_cnt = best_lines
            .iter()
            .flat_map(|l| l.chars())
            .filter(|c| matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}'))
            .count();
        let ascii_cnt = best_lines.iter().flat_map(|l| l.chars()).filter(char::is_ascii_alphanumeric).count();
        if ascii_cnt >= 4 && han_cnt * 10 <= total_non_ws.max(1)
            && let Ok(lang) = Language::CreateLanguage(&HSTRING::from(en_tag))
            && let Ok(en_eng) = OcrEngine::TryCreateFromLanguage(&lang)
            && let Ok(en_lines) = run_with_engine(&en_eng, proc_w, proc_h, &best_buf)
            && score_ocr_lines(&en_lines) > best_score
        {
            best_lines = en_lines;
            lang_tag = en_tag.clone();
        }
    }

    let raw_lines = best_lines;

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
    path: &Path,
    pref: LangPref,
    raw_mode: bool,
) -> Result<OcrOutput, String> {
    crate::ocr_linux::recognize_image_linux(path, pref, raw_mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr_preprocess::compute_ocr_layout;

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
        assert_eq!(compute_ocr_layout(305, 57, 2600, false), (1220, 228, 48, true));
        assert_eq!(compute_ocr_layout(1200, 600, 4096, false), (2400, 1200, 24, true));
        assert_eq!(compute_ocr_layout(1920, 953, 4096, true), (1920, 953, 0, false));
        assert_eq!(compute_ocr_layout(5120, 2160, 4096, false), (4096, 1728, 0, true));
    }
}
