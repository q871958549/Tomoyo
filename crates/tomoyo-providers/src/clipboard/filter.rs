//! Clipboard semantic classification, structured `@type` / `#tag` query parsing,
//! Pinyin-aware multi-token matching, and single-line plain-text normalization.

use serde::{Deserialize, Serialize};
use tomoyo_core::{score_match, IndexedText};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardSubtype {
    Text,
    Link,
    Color,
    Path,
    Code,
    Image,
    Files,
}

impl ClipboardSubtype {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Link => "link",
            Self::Color => "color",
            Self::Path => "path",
            Self::Code => "code",
            Self::Image => "image",
            Self::Files => "files",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "link" => Self::Link,
            "color" => Self::Color,
            "path" => Self::Path,
            "code" => Self::Code,
            "image" => Self::Image,
            "files" => Self::Files,
            _ => Self::Text,
        }
    }

    pub fn badge_label(self) -> &'static str {
        match self {
            Self::Text => "TXT",
            Self::Link => "URL",
            Self::Color => "CLR",
            Self::Path => "PATH",
            Self::Code => "CODE",
            Self::Image => "IMG",
            Self::Files => "FILE",
        }
    }
}

/// Infers semantic subtype from copied text with zero regex dependency.
pub fn classify_text_subtype(raw: &str) -> ClipboardSubtype {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ClipboardSubtype::Text;
    }
    let single_line = !trimmed.contains('\n') && !trimmed.contains('\r');
    if single_line {
        let low = trimmed.to_ascii_lowercase();
        if (low.starts_with("https://") || low.starts_with("http://"))
            && !trimmed.chars().any(char::is_whitespace)
            && trimmed.len() >= 10
        {
            return ClipboardSubtype::Link;
        }
        if is_color_literal(&low) {
            return ClipboardSubtype::Color;
        }
        if is_windows_path_literal(trimmed) {
            return ClipboardSubtype::Path;
        }
    }
    if looks_like_code(trimmed) {
        return ClipboardSubtype::Code;
    }
    ClipboardSubtype::Text
}

fn is_color_literal(low: &str) -> bool {
    if let Some(hex) = low.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    (low.starts_with("rgb(") || low.starts_with("rgba(") || low.starts_with("hsl("))
        && low.ends_with(')')
        && low.len() <= 40
}

fn is_windows_path_literal(s: &str) -> bool {
    let cleaned = s.trim_matches('"');
    let b = cleaned.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let unc = cleaned.starts_with(r"\\") && cleaned.len() >= 5;
    (drive || unc) && cleaned.len() <= 260
}

fn looks_like_code(s: &str) -> bool {
    let trimmed = s.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return trimmed.contains(':') || trimmed.contains('"');
    }
    let lines = trimmed.lines().count();
    if lines >= 2 {
        let cues = ["fn ", "pub ", "const ", "let ", "def ", "class ", "import ", "SELECT ", "=>", "();", "};"];
        return cues.iter().any(|c| trimmed.contains(c));
    }
    false
}

#[derive(Debug, Clone, Default)]
pub struct ParsedClipQuery {
    pub pinned_only: bool,
    pub subtype: Option<ClipboardSubtype>,
    pub tag: Option<String>,
    pub tokens: Vec<String>,
}

impl ParsedClipQuery {
    pub fn parse(raw: &str) -> Self {
        let mut out = Self::default();
        for tok in raw.split_whitespace() {
            let low = tok.to_ascii_lowercase();
            match low.as_str() {
                "@pin" | "@fav" | "@置顶" | "@收藏" => out.pinned_only = true,
                "@img" | "@image" | "@图片" => out.subtype = Some(ClipboardSubtype::Image),
                "@text" | "@txt" | "@文本" => out.subtype = Some(ClipboardSubtype::Text),
                "@link" | "@url" | "@链接" => out.subtype = Some(ClipboardSubtype::Link),
                "@file" | "@files" | "@path" | "@文件" => out.subtype = Some(ClipboardSubtype::Files),
                "@color" | "@颜色" => out.subtype = Some(ClipboardSubtype::Color),
                "@code" | "@代码" => out.subtype = Some(ClipboardSubtype::Code),
                _ => {
                    if let Some(tag) = tok.strip_prefix('#') {
                        let clean = tag.trim();
                        if !clean.is_empty() && !is_color_literal(&low) {
                            out.tag = Some(clean.to_lowercase());
                            continue;
                        }
                    }
                    out.tokens.push(low);
                }
            }
        }
        out
    }

    pub fn is_unfiltered(&self) -> bool {
        !self.pinned_only && self.subtype.is_none() && self.tag.is_none() && self.tokens.is_empty()
    }
}

/// Checks whether a clipboard entry matches all query tokens (case-insensitive substring or Pinyin).
pub fn matches_clip_tokens(haystack_lower: &str, raw_title: &str, tokens: &[String]) -> bool {
    if tokens.is_empty() {
        return true;
    }
    let mut pinyin_idx: Option<IndexedText> = None;
    for tok in tokens {
        if haystack_lower.contains(tok.as_str()) {
            continue;
        }
        if tok.is_ascii() && !raw_title.is_ascii() {
            let idx = pinyin_idx.get_or_insert_with(|| IndexedText::new(raw_title));
            if score_match(tok, idx).is_some() {
                continue;
            }
        }
        return false;
    }
    true
}

/// Collapses line breaks and redundant spaces into a single clean plain-text line (`Shift+Enter`).
pub fn normalize_single_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_links_colors_paths_and_code() {
        assert_eq!(classify_text_subtype("https://github.com/q871958549/Tomoyo"), ClipboardSubtype::Link);
        assert_eq!(classify_text_subtype("#3b5bdb"), ClipboardSubtype::Color);
        assert_eq!(classify_text_subtype("rgb(12, 34, 56)"), ClipboardSubtype::Color);
        assert_eq!(classify_text_subtype(r"C:\Users\Admin\report.pdf"), ClipboardSubtype::Path);
        assert_eq!(classify_text_subtype("{\"ok\": true}"), ClipboardSubtype::Code);
        assert_eq!(classify_text_subtype("普通备忘文本"), ClipboardSubtype::Text);
    }

    #[test]
    fn parses_type_tag_pin_and_tokens() {
        let q = ParsedClipQuery::parse("@pin @link #工作 github tomoyo");
        assert!(q.pinned_only);
        assert_eq!(q.subtype, Some(ClipboardSubtype::Link));
        assert_eq!(q.tag.as_deref(), Some("工作"));
        assert_eq!(q.tokens, vec!["github", "tomoyo"]);

        let color_q = ParsedClipQuery::parse("#3b5bdb");
        assert!(color_q.tag.is_none());
        assert_eq!(color_q.tokens, vec!["#3b5bdb"]);
    }

    #[test]
    fn normalizes_multiline_text_into_single_line() {
        assert_eq!(normalize_single_line("  hello \r\n\t  world  \n rust "), "hello world rust");
    }
}
