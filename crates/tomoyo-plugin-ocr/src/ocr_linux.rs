//! Native Linux offline OCR adapter powered by system `tesseract` CLI.

use crate::ocr_engine::{clean_cjk_spaces, LangPref, OcrOutput};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub fn available_ocr_languages_linux() -> Vec<String> {
    let Ok(out) = Command::new("tesseract").arg("--list-langs").output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("List of") && *l != "osd")
        .map(ToString::to_string)
        .collect()
}

pub fn recognize_image_linux(
    path: &Path,
    pref: LangPref,
    raw_mode: bool,
) -> Result<OcrOutput, String> {
    let started = Instant::now();
    if !path.is_file() {
        return Err(format!("图片文件不存在: {}", path.display()));
    }
    let installed = available_ocr_languages_linux();
    if installed.is_empty() {
        return Err(
            "未检测到 tesseract 或任何语言包。Arch/CachyOS 请安装: sudo pacman -S tesseract tesseract-data-eng tesseract-data-chi_sim"
                .into(),
        );
    }

    let has = |name: &str| installed.iter().any(|l| l.eq_ignore_ascii_case(name));
    let has_zh = has("chi_sim");
    let has_en = has("eng");
    let (tess_lang, lang_note) = match pref {
        LangPref::En => {
            if has_en {
                ("eng".to_string(), None)
            } else {
                (installed[0].clone(), Some(format!("未安装英文 eng 语言包（当前仅 {:?}），已回退使用默认语言。", installed)))
            }
        }
        LangPref::Zh => {
            if has_zh && has_en {
                ("chi_sim+eng".to_string(), None)
            } else if has_zh {
                ("chi_sim".to_string(), None)
            } else {
                let fb = if has_en { "eng".to_string() } else { installed[0].clone() };
                (fb, Some("未检测到中文 tessdata (chi_sim)，已回退英文识别。Arch/CachyOS 可执行: sudo pacman -S tesseract-data-chi_sim".into()))
            }
        }
        LangPref::Auto => {
            if has_zh && has_en {
                ("chi_sim+eng".to_string(), None)
            } else if has_zh {
                ("chi_sim".to_string(), None)
            } else if has_en {
                (
                    "eng".to_string(),
                    Some("当前仅检测到英文 Tesseract 语言包 (eng)；若需中英混排识字请安装 tesseract-data-chi_sim".into()),
                )
            } else {
                (installed[0].clone(), None)
            }
        }
    };

    let run = |psm: Option<&str>| {
        let mut cmd = Command::new("tesseract");
        cmd.arg(path).arg("stdout").args(["-l", &tess_lang]);
        if let Some(p) = psm {
            cmd.args(["--psm", p]);
        }
        cmd.output()
    };

    let out = match run(Some("6")) {
        Ok(o) if o.status.success() => o,
        _ => run(None).map_err(|e| format!("调用 tesseract 失败: {e}"))?,
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Tesseract 识别失败: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let raw_lines: Vec<String> = stdout
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .map(ToString::to_string)
        .collect();
    let raw_text = raw_lines.join("\n");
    let lines: Vec<String> = if raw_mode {
        raw_lines
    } else {
        raw_lines.iter().map(|l| clean_cjk_spaces(l)).collect()
    };
    let text = lines.join("\n");
    let src_dims = read_png_dims(path).unwrap_or((0, 0));

    Ok(OcrOutput {
        text,
        raw_text,
        lines,
        lang_tag: format!("tesseract:{tess_lang}"),
        src_dims,
        proc_dims: src_dims,
        elapsed_ms: started.elapsed().as_millis() as u64,
        lang_note,
    })
}

fn read_png_dims(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut hdr = [0u8; 24];
    if f.read_exact(&mut hdr).is_err() || &hdr[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let w = u32::from_be_bytes([hdr[16], hdr[17], hdr[18], hdr[19]]);
    let h = u32::from_be_bytes([hdr[20], hdr[21], hdr[22], hdr[23]]);
    Some((w, h))
}
