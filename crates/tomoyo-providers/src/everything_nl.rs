//! Zero-latency offline natural-language filter translator for file searches (`f ` / `ev `).

use tomoyo_core::Lang;

pub(crate) struct NlTranslation {
    pub everything_query: String,
    pub score_keywords: String,
    pub translated: bool,
}

pub(crate) fn split_tokens_preserving_quotes(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        let mut in_quotes = false;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'"' {
                in_quotes = !in_quotes;
                i += 1;
            } else if !in_quotes && b.is_ascii_whitespace() {
                break;
            } else {
                let ch_len = raw[i..].chars().next().map_or(1, |c| c.len_utf8());
                i += ch_len;
            }
        }
        if start < i {
            out.push(&raw[start..i]);
        }
    }
    out
}

fn is_raw_everything_token(low: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "ext:",
        "dm:",
        "dc:",
        "size:",
        "path:",
        "folder:",
        "file:",
        "content:",
        "regex:",
        "case:",
        "wholefilename:",
        "startwith:",
        "endwith:",
    ];
    PREFIXES.iter().any(|p| low.starts_with(p))
}

fn normalize_size_suffix(rest: &str) -> Option<String> {
    let split = rest.find(|c: char| !c.is_ascii_digit())?;
    let (num, unit_raw) = rest.split_at(split);
    if num.is_empty() || num.len() > 6 {
        return None;
    }
    let unit = match unit_raw.to_ascii_lowercase().as_str() {
        "b" => "b",
        "k" | "kb" => "kb",
        "m" | "mb" => "mb",
        "g" | "gb" => "gb",
        "t" | "tb" => "tb",
        _ => return None,
    };
    Some(format!("{num}{unit}"))
}

fn translate_token(tok: &str, _lang: Lang) -> Option<String> {
    let low = tok.to_ascii_lowercase();
    if let Some(mapped) = match low.as_str() {
        "今天" | "今日" | "today" => Some("dm:today"),
        "昨天" | "昨日" | "yesterday" => Some("dm:yesterday"),
        "近7天" | "最近7天" | "近七天" | "最近一周" | "近一周" | "last7days" => {
            Some("dm:last7days")
        }
        "本周" | "这周" | "thisweek" => Some("dm:thisweek"),
        "本月" | "这个月" | "thismonth" => Some("dm:thismonth"),
        "今年" | "thisyear" => Some("dm:thisyear"),
        "大文件" | "large" => Some("size:>50mb"),
        "超大文件" | "huge" => Some("size:>500mb"),
        "仅文件夹" | "只要文件夹" | "文件夹" | "目录" | "folders" => Some("folder:"),
        "下载目录" | "下载里" | "下载文件夹" | "downloads" => Some("path:Downloads"),
        "桌面" | "桌面上" | "desktop" => Some("path:Desktop"),
        "文档目录" | "我的文档" | "documents" => Some("path:Documents"),
        "pdf" => Some("ext:pdf"),
        "表格" | "excel" => Some("ext:xlsx;xls;csv;xlsm"),
        "word" => Some("ext:docx;doc;rtf"),
        "ppt" | "幻灯片" => Some("ext:pptx;ppt"),
        "图片" | "照片" => Some("ext:png;jpg;jpeg;webp;gif;bmp;svg"),
        "视频" => Some("ext:mp4;mkv;mov;avi;webm;flv"),
        "音频" | "音乐" => Some("ext:mp3;flac;wav;m4a;ogg;aac"),
        "压缩包" | "压缩文件" => Some("ext:zip;rar;7z;tar;gz"),
        "代码" | "源码" => Some("ext:rs;ts;tsx;js;py;go;c;cpp;h;java;cs"),
        _ => None,
    } {
        return Some(mapped.into());
    }

    for prefix in ["大于", "超过", ">"] {
        if let Some(rest) = low.strip_prefix(prefix)
            && let Some(norm) = normalize_size_suffix(rest.trim_start_matches('='))
        {
            return Some(format!("size:>{norm}"));
        }
    }
    for prefix in ["小于", "不到", "<"] {
        if let Some(rest) = low.strip_prefix(prefix)
            && let Some(norm) = normalize_size_suffix(rest.trim_start_matches('='))
        {
            return Some(format!("size:<{norm}"));
        }
    }
    None
}

pub(crate) fn translate_natural_query(raw: &str, lang: Lang) -> NlTranslation {
    let mut ev_tokens: Vec<String> = Vec::new();
    let mut kw_tokens: Vec<String> = Vec::new();
    let mut translated = false;

    for tok in split_tokens_preserving_quotes(raw.trim()) {
        let low = tok.to_ascii_lowercase();
        let next = if is_raw_everything_token(&low) {
            tok.to_string()
        } else if let Some(mapped) = translate_token(tok, lang) {
            translated = true;
            mapped
        } else {
            kw_tokens.push(tok.to_string());
            tok.to_string()
        };
        if !ev_tokens.iter().any(|t| t.eq_ignore_ascii_case(&next)) {
            ev_tokens.push(next);
        }
    }

    NlTranslation {
        everything_query: ev_tokens.join(" "),
        score_keywords: kw_tokens.join(" "),
        translated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_mixed_chinese_natural_filters() {
        let res = translate_natural_query("今天 大于10m pdf 合同", Lang::Zh);
        assert!(res.translated);
        assert_eq!(res.everything_query, "dm:today size:>10mb ext:pdf 合同");
        assert_eq!(res.score_keywords, "合同");
    }

    #[test]
    fn translates_location_size_and_folder_phrases() {
        let dl = translate_natural_query("下载目录 小于500k 压缩包", Lang::Zh);
        assert!(dl.translated);
        assert_eq!(dl.everything_query, "path:Downloads size:<500kb ext:zip;rar;7z;tar;gz");
        assert!(dl.score_keywords.is_empty());

        let dir = translate_natural_query("仅文件夹 tomoyo", Lang::Zh);
        assert!(dir.translated);
        assert_eq!(dir.everything_query, "folder: tomoyo");
        assert_eq!(dir.score_keywords, "tomoyo");
    }

    #[test]
    fn preserves_raw_everything_syntax_and_quotes() {
        let raw = translate_natural_query("ext:rs dm:today \"2026 报告\"", Lang::Zh);
        assert!(!raw.translated);
        assert_eq!(raw.everything_query, "ext:rs dm:today \"2026 报告\"");
        assert_eq!(raw.score_keywords, "\"2026 报告\"");
    }
}
