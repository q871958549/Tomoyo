//! Zero-polling context-aware clipboard suggestion card on empty launcher summon.
//!
//! Surfaces at most ONE high-signal contextual action card when the user opens Tomoyo with an
//! empty query and the clipboard holds a fresh URL, JSON payload, color literal, image, or
//! translatable snippet — respecting `disabled_plugins` with zero background RAM overhead.

use super::AppShell;
use crate::super_panel::context_probe::{classify_text_kind, normalize_url, TextKind};
use tomoyo_core::{Lang, TomoyoConfig, PLUGIN_CAPTURE, PLUGIN_NOTES, PLUGIN_OCR, PLUGIN_TRANSLATE};
use tomoyo_protocol::{
    ActionKind, ActionSpec, ClipboardSnapshot, IconSpec, PreviewSpec, ResultCategory, ResultItem,
    SearchFilter,
};
use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;
use windows::Win32::System::Ole::CF_DIB;

const FRESH_CLIP_SECS: i64 = 90;
const MAX_JSON_PREVIEW_BYTES: usize = 16 * 1024;

impl AppShell {
    /// Prepends at most one contextual clipboard suggestion card on empty `All` view.
    /// Skipped in headless `--self-test` (`self.tray.is_none()`) to keep benchmarks deterministic.
    pub(super) fn inject_context_clipboard_card(&mut self) {
        if self.tray.is_none()
            || !self.current_query.trim().is_empty()
            || self.active_filter != SearchFilter::All
            || self.attached.is_some()
        {
            return;
        }
        let Ok(cfg) = self.config.read().map(|c| c.clone()) else {
            return;
        };
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let has_dib = unsafe { IsClipboardFormatAvailable(CF_DIB.0 as u32).is_ok() };
        let Some(card) = build_context_card(
            self.last_clip_snapshot.as_ref(),
            has_dib,
            now_unix,
            &cfg,
            self.lang,
        ) else {
            return;
        };
        if self.results.iter().any(|r| r.id == card.id) {
            return;
        }
        self.results.insert(0, card);
        if self.results.len() > 16 {
            self.results.truncate(16);
        }
    }
}

pub(crate) fn build_context_card(
    snap: Option<&ClipboardSnapshot>,
    has_dib_image: bool,
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

    if has_dib_image && (has_cap || has_ocr) {
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
}
