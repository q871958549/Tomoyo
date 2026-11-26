//! Pure-Rust clipboard/selection semantic classifier and context-aware suggestion card builder.

use std::path::Path;
use tomoyo_core::{Lang, TomoyoConfig, PLUGIN_CAPTURE, PLUGIN_NOTES, PLUGIN_OCR, PLUGIN_TRANSLATE};
use tomoyo_protocol::{
    ActionKind, ActionSpec, ClipboardSnapshot, IconSpec, PreviewSpec, ResultCategory, ResultItem,
};

const FRESH_CLIP_SECS: i64 = 90;
const MAX_JSON_PREVIEW_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Plain,
    Url,
    Json,
    Color,
    Path,
    Code,
}

impl TextKind {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "text",
            Self::Url => "url",
            Self::Json => "json",
            Self::Color => "color",
            Self::Path => "path",
            Self::Code => "code",
        }
    }
}

/// Classifies text into a semantic kind for contextual launcher cards and Super Panel actions.
pub fn classify_text_kind(text: &str) -> TextKind {
    let t = text.trim();
    if normalize_url(t).is_some() {
        return TextKind::Url;
    }
    if is_color_literal(t) {
        return TextKind::Color;
    }
    if ((t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']')))
        && serde_json::from_str::<serde_json::Value>(t).is_ok()
    {
        return TextKind::Json;
    }
    if looks_like_local_path(t) {
        return TextKind::Path;
    }
    if looks_like_code(t) {
        return TextKind::Code;
    }
    TextKind::Plain
}

pub fn normalize_url(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return None;
    }
    let low = t.to_ascii_lowercase();
    if low.starts_with("https://") || low.starts_with("http://") {
        tomoyo_core::is_safe_http_url(t).then(|| t.to_string())
    } else if low.starts_with("www.") {
        let full = format!("https://{t}");
        tomoyo_core::is_safe_http_url(&full).then_some(full)
    } else {
        None
    }
}

fn is_color_literal(t: &str) -> bool {
    if let Some(hex) = t.strip_prefix('#') {
        return matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let low = t.to_ascii_lowercase();
    (low.starts_with("rgb(") || low.starts_with("rgba(")) && low.ends_with(')') && low.len() <= 36
}

fn looks_like_local_path(t: &str) -> bool {
    if t.contains('\n') || t.len() < 3 || t.len() > 260 {
        return false;
    }
    let unquoted = t.trim_matches('"');
    let bytes = unquoted.as_bytes();
    let is_win = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    let is_unix = unquoted.starts_with('/') || unquoted.starts_with("~/");
    (is_win || is_unix) && Path::new(unquoted).exists()
}

fn looks_like_code(t: &str) -> bool {
    t.lines().count() >= 3
        && (t.contains('{') || t.contains("fn ") || t.contains("def ") || t.contains("const ") || t.contains("import "))
}

pub(crate) fn build_context_card(
    snap: Option<&ClipboardSnapshot>,
    has_image: bool,
    now_unix: i64,
    cfg: &TomoyoConfig,
    lang: Lang,
) -> Option<ResultItem> {
    let has_tr = cfg.is_plugin_enabled(PLUGIN_TRANSLATE);
    let has_cap = cfg.is_plugin_enabled(PLUGIN_CAPTURE);
    let has_ocr = cfg.is_plugin_enabled(PLUGIN_OCR);
    let has_notes = cfg.is_plugin_enabled(PLUGIN_NOTES);

    if let Some(clip) = snap {
        let age = now_unix.saturating_sub(clip.captured_at_unix);
        let trimmed = clip.text.trim();
        if (0..=FRESH_CLIP_SECS).contains(&age) && !trimmed.is_empty() {
            match classify_text_kind(trimmed) {
                TextKind::Url => {
                    if let Some(url) = normalize_url(trimmed) {
                        let short = clip_label(&url, 54);
                        return Some(ResultItem {
                            id: "ctx:clip:url".into(),
                            provider_id: "builtin.context".into(),
                            title: format!("{} {short}", lang.pick("打开剪贴板链接：", "Open Clipboard URL:")),
                            subtitle: lang.pick("检测到刚刚复制的网址 · Enter 浏览器直达 · Ctrl+Enter 仅复制", "Fresh URL on clipboard · Enter to open").into(),
                            category: ResultCategory::Clipboard,
                            score: 10_200,
                            icon: IconSpec::Monogram("↗".into()),
                            badge: Some("URL".into()),
                            copy_text: Some(url.clone()),
                            actions: vec![
                                ActionSpec {
                                    id: "open_url".into(),
                                    label: lang.pick("在浏览器中打开链接", "Open in Browser").into(),
                                    shortcut: Some("Enter".into()),
                                    kind: ActionKind::OpenUrl { url: url.clone() },
                                },
                                ActionSpec {
                                    id: "copy_url".into(),
                                    label: lang.pick("复制规范化网址", "Copy Normalized URL").into(),
                                    shortcut: Some("Ctrl+Enter".into()),
                                    kind: ActionKind::CopyText { text: url.clone(), paste_immediately: false },
                                },
                            ],
                            preview: (url.chars().count() > 54).then(|| PreviewSpec::Text {
                                title: "Clipboard URL".into(),
                                body: url,
                            }),
                        });
                    }
                }
                TextKind::Json if trimmed.len() <= MAX_JSON_PREVIEW_BYTES => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed)
                        && let Ok(pretty) = serde_json::to_string_pretty(&v)
                    {
                        let lines = pretty.lines().count();
                        return Some(ResultItem {
                            id: "ctx:clip:json".into(),
                            provider_id: "builtin.context".into(),
                            title: lang.pick("美化并复制剪贴板 JSON", "Pretty-Print Clipboard JSON").into(),
                            subtitle: format!(
                                "{} ({lines} {}) · Enter {} · Ctrl+Enter {}",
                                lang.pick("检测到有效 JSON 数据", "Valid JSON detected"),
                                lang.pick("行", "lines"),
                                lang.pick("复制格式化结果", "copy formatted"),
                                lang.pick("直接粘贴", "paste"),
                            ),
                            category: ResultCategory::Clipboard,
                            score: 10_200,
                            icon: IconSpec::Monogram("{}".into()),
                            badge: Some("JSON".into()),
                            copy_text: Some(pretty.clone()),
                            actions: vec![
                                ActionSpec {
                                    id: "copy_pretty".into(),
                                    label: lang.pick("复制美化后的 JSON", "Copy Pretty JSON").into(),
                                    shortcut: Some("Enter".into()),
                                    kind: ActionKind::CopyText { text: pretty.clone(), paste_immediately: false },
                                },
                                ActionSpec {
                                    id: "paste_pretty".into(),
                                    label: lang.pick("直接粘贴美化 JSON", "Paste Pretty JSON").into(),
                                    shortcut: Some("Ctrl+Enter".into()),
                                    kind: ActionKind::CopyText { text: pretty.clone(), paste_immediately: true },
                                },
                            ],
                            preview: Some(PreviewSpec::Text {
                                title: lang.pick("JSON 美化预览", "Formatted JSON").into(),
                                body: pretty.chars().take(4000).collect(),
                            }),
                        });
                    }
                }
                TextKind::Color => {
                    let mut actions = Vec::new();
                    if has_cap {
                        actions.push(ActionSpec {
                            id: "pin_color".into(),
                            label: lang.pick("贴图色卡到桌面", "Pin Color Card").into(),
                            shortcut: Some("Enter".into()),
                            kind: ActionKind::PluginExecute {
                                plugin_id: PLUGIN_CAPTURE.into(),
                                action_id: "pin_text".into(),
                                payload: serde_json::json!({ "text": trimmed }),
                            },
                        });
                    }
                    let toggled = if trimmed.starts_with('#') {
                        if trimmed.chars().any(|c| c.is_ascii_lowercase()) {
                            trimmed.to_ascii_uppercase()
                        } else {
                            trimmed.to_ascii_lowercase()
                        }
                    } else {
                        trimmed.to_string()
                    };
                    actions.push(ActionSpec {
                        id: "copy_color".into(),
                        label: format!("{} {toggled}", lang.pick("复制色值", "Copy")),
                        shortcut: Some(if actions.is_empty() { "Enter" } else { "Ctrl+Enter" }.into()),
                        kind: ActionKind::CopyText { text: toggled, paste_immediately: false },
                    });
                    return Some(ResultItem {
                        id: "ctx:clip:color".into(),
                        provider_id: "builtin.context".into(),
                        title: format!("{} {trimmed}", lang.pick("剪贴板色值：", "Clipboard Color:")),
                        subtitle: lang.pick("检测到刚刚复制的颜色代码 · 一键桌面贴图色卡或转换大小写", "Fresh color literal on clipboard").into(),
                        category: ResultCategory::Clipboard,
                        score: 10_200,
                        icon: IconSpec::Monogram("◈".into()),
                        badge: Some("COLOR".into()),
                        copy_text: Some(trimmed.to_string()),
                        actions,
                        preview: None,
                    });
                }
                TextKind::Plain | TextKind::Code if has_tr && (3..=600).contains(&trimmed.chars().count()) => {
                    let short = clip_label(trimmed, 42);
                    let mut actions = vec![ActionSpec {
                        id: "open_tr".into(),
                        label: lang.pick("打开多源翻译对照", "Translate Selection").into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::PluginExecute {
                            plugin_id: PLUGIN_TRANSLATE.into(),
                            action_id: "open_tr".into(),
                            payload: serde_json::json!({ "text": trimmed, "target_lang": "" }),
                        },
                    }];
                    if has_notes {
                        actions.push(ActionSpec {
                            id: "save_note".into(),
                            label: lang.pick("一键存为本地备忘", "Save to Quick Notes").into(),
                            shortcut: Some("Ctrl+Enter".into()),
                            kind: ActionKind::PluginExecute {
                                plugin_id: PLUGIN_NOTES.into(),
                                action_id: "save_selection_note".into(),
                                payload: serde_json::json!({ "text": trimmed }),
                            },
                        });
                    }
                    return Some(ResultItem {
                        id: "ctx:clip:text".into(),
                        provider_id: "builtin.context".into(),
                        title: format!("{}「{short}」", lang.pick("快捷翻译剪贴板：", "Translate Clipboard:")),
                        subtitle: lang.pick("检测到刚刚复制的文本 · Enter 多源翻译 · Ctrl+Enter 一键存备忘", "Fresh clipboard text · Enter to translate").into(),
                        category: ResultCategory::Clipboard,
                        score: 10_100,
                        icon: IconSpec::Monogram("译".into()),
                        badge: Some("CLIP".into()),
                        copy_text: Some(trimmed.to_string()),
                        actions,
                        preview: (trimmed.contains('\n') || trimmed.chars().count() > 42).then(|| PreviewSpec::Text {
                            title: lang.pick("剪贴板文本上下文", "Clipboard Context").into(),
                            body: trimmed.chars().take(2000).collect(),
                        }),
                    });
                }
                _ => {}
            }
        }
    }

    if has_image && (has_cap || has_ocr) {
        let mut actions = Vec::new();
        if has_cap {
            actions.push(ActionSpec {
                id: "pin_img".into(),
                label: lang.pick("将剪贴板图片贴图置顶", "Pin Clipboard Image").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_CAPTURE.into(),
                    action_id: "run_pin_clipboard".into(),
                    payload: serde_json::json!({ "mode": "pin_clipboard" }),
                },
            });
        }
        if has_ocr {
            actions.push(ActionSpec {
                id: "ocr_img".into(),
                label: lang.pick("离线 OCR 识别图片文字", "Offline OCR Image").into(),
                shortcut: Some(if actions.is_empty() { "Enter" } else { "Ctrl+Enter" }.into()),
                kind: ActionKind::FillQuery { query: "ocr".into() },
            });
        }
        return Some(ResultItem {
            id: "ctx:clip:image".into(),
            provider_id: "builtin.context".into(),
            title: lang.pick("剪贴板图片：贴图置顶 / 离线 OCR 识字", "Clipboard Image: Pin or Offline OCR").into(),
            subtitle: lang.pick("检测到剪贴板中有位图/截图 · Enter 桌面贴图 · Ctrl+Enter 离线识字", "Image ready on clipboard").into(),
            category: ResultCategory::Clipboard,
            score: 10_150,
            icon: IconSpec::Monogram("IMG".into()),
            badge: Some("IMG".into()),
            copy_text: None,
            actions,
            preview: None,
        });
    }

    None
}

fn clip_label(raw: &str, max_chars: usize) -> String {
    let single: String = raw
        .chars()
        .map(|c| if matches!(c, '\r' | '\n' | '\t') { ' ' } else { c })
        .take(max_chars)
        .collect();
    if raw.chars().count() > max_chars {
        format!("{}…", single.trim())
    } else {
        single.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_context_cards_for_fresh_url_json_color_and_image_only() {
        let cfg = TomoyoConfig::default();
        let now = 1_700_000_000;

        let url_snap = ClipboardSnapshot { text: "https://github.com/q871958549/Tomoyo".into(), captured_at_unix: now - 10 };
        let url_card = build_context_card(Some(&url_snap), false, now, &cfg, Lang::Zh).unwrap();
        assert_eq!(url_card.id, "ctx:clip:url");
        assert!(matches!(url_card.actions[0].kind, ActionKind::OpenUrl { .. }));

        let json_snap = ClipboardSnapshot { text: r#"{"ok":true,"v":3}"#.into(), captured_at_unix: now - 5 };
        let json_card = build_context_card(Some(&json_snap), false, now, &cfg, Lang::Zh).unwrap();
        assert_eq!(json_card.id, "ctx:clip:json");
        assert!(json_card.copy_text.as_deref().unwrap().contains('\n'));

        let stale_snap = ClipboardSnapshot { text: "https://example.com".into(), captured_at_unix: now - 300 };
        assert!(build_context_card(Some(&stale_snap), false, now, &cfg, Lang::Zh).is_none());

        let img_card = build_context_card(None, true, now, &cfg, Lang::Zh).unwrap();
        assert_eq!(img_card.id, "ctx:clip:image");
        assert_eq!(img_card.actions.len(), 2);

        let short_txt = ClipboardSnapshot { text: "TranslateMessage(&msg)".into(), captured_at_unix: now - 5 };
        let short_card = build_context_card(Some(&short_txt), false, now, &cfg, Lang::Zh).unwrap();
        assert!(short_card.preview.is_none());

        let long_txt = ClipboardSnapshot { text: "Line 1\nLine 2 longer multi-line snippet".into(), captured_at_unix: now - 5 };
        let long_card = build_context_card(Some(&long_txt), false, now, &cfg, Lang::Zh).unwrap();
        assert!(long_card.preview.is_some());
    }

    #[test]
    fn classifies_selected_text_kinds_accurately() {
        assert_eq!(classify_text_kind("https://github.com/q871958549/Tomoyo"), TextKind::Url);
        assert_eq!(classify_text_kind("www.rust-lang.org"), TextKind::Url);
        assert_eq!(classify_text_kind("#7C6AF7"), TextKind::Color);
        assert_eq!(classify_text_kind("rgb(16, 185, 129)"), TextKind::Color);
        assert_eq!(classify_text_kind(r#"{"ok":true,"n":42}"#), TextKind::Json);
        assert_eq!(classify_text_kind("serendipity"), TextKind::Plain);
        assert_eq!(classify_text_kind("选中文本快捷翻译"), TextKind::Plain);
        assert_eq!(
            classify_text_kind("fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}"),
            TextKind::Code
        );
        assert_eq!(normalize_url("www.example.com/a").as_deref(), Some("https://www.example.com/a"));
        assert!(normalize_url("javascript:alert(1)").is_none());
    }
}
