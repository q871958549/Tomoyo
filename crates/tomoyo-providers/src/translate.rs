//! Fast zero-blocking `tr` / `fy` Translation provider backed by local history/wordbook cache
//! and the ephemeral multi-engine Bilingual Translator popup (`__tomoyo_translator__`).

use std::sync::{Arc, RwLock};
use tomoyo_core::translate::{resolve_target_lang, TranslateStore};
use tomoyo_core::{Lang, Provider, TomoyoConfig, PLUGIN_TRANSLATE};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, PreviewSpec, QueryContext,
    ResultCategory, ResultItem,
};

pub struct TranslateProvider {
    config: Arc<RwLock<TomoyoConfig>>,
}

impl TranslateProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        Self { config }
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }

    fn default_target(&self) -> String {
        self.config
            .read()
            .map(|c| c.translate.target_lang.clone())
            .unwrap_or_else(|_| "auto".into())
    }
}

impl Provider for TranslateProvider {
    fn id(&self) -> &'static str {
        "builtin.translate"
    }

    fn name(&self) -> &'static str {
        "Multi-Engine Translator (tr / fy)"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["tr", "fy"]
    }

    fn matches_prefix(&self, token_lower: &str) -> bool {
        self.config.read().map(|c| c.is_plugin_enabled(PLUGIN_TRANSLATE)).unwrap_or(true)
            && self.prefixes().contains(&token_lower)
    }

    fn supports_global(&self) -> bool {
        false
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        if !self.config.read().map(|c| c.is_plugin_enabled(PLUGIN_TRANSLATE)).unwrap_or(true) {
            return Vec::new();
        }
        let lang = self.lang();
        let (target_override, text) = parse_translate_args(&ctx.args);
        let default_tgt = self.default_target();
        let eff_target = if text.is_empty() {
            target_override.clone().unwrap_or(default_tgt)
        } else {
            resolve_target_lang(&text, target_override.as_deref().unwrap_or(&default_tgt))
        };

        let mut items = Vec::new();
        let store = TranslateStore::load_default();

        if !text.is_empty() {
            let preview_snip: String = text.chars().take(64).collect();
            let tgt_label = target_lang_label(&eff_target);
            items.push(ResultItem {
                id: "tr:open_query".into(),
                provider_id: self.id().into(),
                title: match lang {
                    Lang::Zh => format!("翻译「{preview_snip}」→ {tgt_label}"),
                    Lang::En => format!("Translate \"{preview_snip}\" → {tgt_label}"),
                },
                subtitle: lang
                    .pick(
                        "回车打开多源对照翻译窗（Bing / MyMemory / DeepL / OpenAI / Ollama，Esc 释放 100% 内存）",
                        "Enter to open multi-engine bilingual popup (0 idle RAM on Esc)",
                    )
                    .into(),
                category: ResultCategory::Plugin,
                score: 10_600,
                icon: IconSpec::Monogram("译".into()),
                badge: Some(tgt_label.into()),
                copy_text: Some(text.clone()),
                actions: vec![ActionSpec {
                    id: "open_tr".into(),
                    label: lang.pick("多引擎并行翻译", "Translate Across Enabled Engines").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::OpenTranslator {
                            text: text.clone(),
                            target_lang: Some(eff_target.clone()),
                        },
                    },
                }],
                preview: Some(PreviewSpec::Text {
                    title: format!("待翻译原文 → {tgt_label}"),
                    body: text.clone(),
                }),
            });

            // Instant local cache / wordbook hits (0 network wait!)
            let low = text.to_lowercase();
            for (i, wb) in store
                .wordbook
                .iter()
                .filter(|w| w.word.to_lowercase().contains(&low) || w.translation.to_lowercase().contains(&low))
                .take(4)
                .enumerate()
            {
                items.push(cache_item(
                    self.id(),
                    &format!("tr:wb:{i}"),
                    &format!("★ {} → {}", wb.word, wb.translation),
                    &format!("生词本缓存 {} {}", wb.phonetic, wb.note),
                    &wb.translation,
                    9_800 - (i as i64 * 10),
                    "生词",
                ));
            }
            for (i, h) in store
                .history
                .iter()
                .filter(|h| h.source.to_lowercase().contains(&low))
                .take(4)
                .enumerate()
            {
                items.push(cache_item(
                    self.id(),
                    &format!("tr:hist:{i}"),
                    &format!("{} → {}", clip_str(&h.source, 28), clip_str(&h.translated, 42)),
                    &format!("翻译历史 ({}) · 回车直接贴回译文 · Ctrl+Enter 仅复制", h.engine),
                    &h.translated,
                    9_500 - (i as i64 * 10),
                    &h.target_lang,
                ));
            }
            return items;
        }

        // Empty args: offer clipboard quick-translate + open workbench + recent history
        let clip_text = ctx
            .clipboard
            .as_ref()
            .map(|c| c.text.trim().to_string())
            .filter(|s| !s.is_empty());

        items.push(ResultItem {
            id: "tr:open_empty".into(),
            provider_id: self.id().into(),
            title: match &clip_text {
                Some(c) => format!("打开多源翻译窗（带入剪贴板：{}）", clip_str(c, 36)),
                None => lang.pick("打开多源翻译工作台（支持划词 Alt+T / 历史 / 生词本）", "Open Multi-Engine Translator").into(),
            },
            subtitle: lang
                .pick(
                    "也可输入 `tr <原文>` 或 `tr en/zh/ja/ko <原文>` · 划词按 Alt+T 即时翻译",
                    "Type `tr <text>` or `tr en/zh/ja <text>` · Press Alt+T anywhere to translate selection",
                )
                .into(),
            category: ResultCategory::Plugin,
            score: 10_500,
            icon: IconSpec::Monogram("译".into()),
            badge: Some("Alt+T".into()),
            copy_text: clip_text.clone(),
            actions: vec![ActionSpec {
                id: "open_tr".into(),
                label: lang.pick("打开翻译窗口", "Open Translator").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::InternalCommand {
                    command: InternalCommandKind::OpenTranslator {
                        text: clip_text.unwrap_or_default(),
                        target_lang: None,
                    },
                },
            }],
            preview: None,
        });

        for (i, h) in store.history.iter().take(6).enumerate() {
            items.push(cache_item(
                self.id(),
                &format!("tr:recent:{i}"),
                &format!("{} → {}", clip_str(&h.source, 28), clip_str(&h.translated, 42)),
                &format!("近期翻译 ({}) · Enter 直贴译文 · Ctrl+Enter 仅复制", h.engine),
                &h.translated,
                9_400 - (i as i64 * 10),
                &h.target_lang,
            ));
        }
        items
    }
}

fn parse_translate_args(raw: &str) -> (Option<String>, String) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, String::new());
    }
    let Some((first, rest)) = trimmed.split_once(char::is_whitespace) else {
        return (None, trimmed.to_string());
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return (None, trimmed.to_string());
    }
    let mapped = match first.to_ascii_lowercase().as_str() {
        "zh" | "cn" | "zh-cn" | "zh-hans" => Some("zh-Hans"),
        "en" | "eng" => Some("en"),
        "ja" | "jp" => Some("ja"),
        "ko" | "kr" => Some("ko"),
        "fr" => Some("fr"),
        "de" => Some("de"),
        _ => None,
    };
    match mapped {
        Some(code) => (Some(code.to_string()), rest.to_string()),
        None => (None, trimmed.to_string()),
    }
}

fn target_lang_label(code: &str) -> &'static str {
    match code {
        "zh-Hans" => "简体中文",
        "en" => "English",
        "ja" => "日本語",
        "ko" => "한국어",
        "fr" => "Français",
        "de" => "Deutsch",
        _ => "自动互译",
    }
}

fn cache_item(
    provider_id: &str,
    id: &str,
    title: &str,
    subtitle: &str,
    translated: &str,
    score: i64,
    badge: &str,
) -> ResultItem {
    ResultItem {
        id: id.into(),
        provider_id: provider_id.into(),
        title: title.into(),
        subtitle: subtitle.trim().into(),
        category: ResultCategory::Clipboard,
        score,
        icon: IconSpec::Monogram("译".into()),
        badge: Some(badge.into()),
        copy_text: Some(translated.into()),
        actions: vec![
            ActionSpec {
                id: "paste_tr".into(),
                label: "粘贴译文到原窗口".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText {
                    text: translated.into(),
                    paste_immediately: true,
                },
            },
            ActionSpec {
                id: "copy_tr".into(),
                label: "仅复制译文".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText {
                    text: translated.into(),
                    paste_immediately: false,
                },
            },
        ],
        preview: Some(PreviewSpec::Text {
            title: "译文详情".into(),
            body: translated.into(),
        }),
    }
}

fn clip_str(s: &str, max_chars: usize) -> String {
    let single: String = s.chars().map(|c| if matches!(c, '\r' | '\n' | '\t') { ' ' } else { c }).take(max_chars).collect();
    if s.chars().count() > max_chars { format!("{}…", single.trim()) } else { single.trim().into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_optional_language_prefix_in_tr_query() {
        assert_eq!(parse_translate_args("en 你好世界"), (Some("en".into()), "你好世界".into()));
        assert_eq!(parse_translate_args("ja hello"), (Some("ja".into()), "hello".into()));
        assert_eq!(parse_translate_args("hello world"), (None, "hello world".into()));
    }
}
