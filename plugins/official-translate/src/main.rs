//! Official Tomoyo Protocol v3 Hybrid Plugin: Multi-Engine Bilingual Translator (`tr` / `fy`).
//!
//! Provides instant `tr`/`fy` local history & wordbook search, `Alt+T` lossless selection-translate
//! via `Permission::SelectionRead`, dynamic system tray action, and bidirectional Hybrid RPC
//! (`get_init`, `translate`, `save_word`, `delete_word`, `clear_history`) for `ui/index.html`.

mod engines;
mod http_curl;
mod store;

use engines::{resolve_target_lang, translate_parallel};
use serde_json::{json, Value};
use store::{read_last_req, resolve_runtime_config, write_last_req, TranslateStore};
use tomoyo_sdk::{
    ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse, IconSpec, OpenWebViewRequest,
    PluginCallRequest, PluginEventKind, PluginEventPayload, PluginMeta, PreviewSpec, QueryContext,
    ResultCategory, ResultItem, TomoyoPlugin,
};

const PLUGIN_ID: &str = "dev.tomoyo.translate";

struct TranslatePlugin;

impl TomoyoPlugin for TranslatePlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("多源对照翻译 (Multi-Engine Translator)", env!("CARGO_PKG_VERSION"))
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let cfg = resolve_runtime_config(&ctx.plugin_settings);
        let (target_override, text) = parse_translate_args(&ctx.args);
        let eff_target = if text.is_empty() {
            target_override.clone().unwrap_or_else(|| cfg.target_lang.clone())
        } else {
            resolve_target_lang(&text, target_override.as_deref().unwrap_or(&cfg.target_lang))
        };

        let mut items = Vec::new();
        let store = TranslateStore::load_default();

        if !text.is_empty() {
            let preview_snip: String = text.chars().take(64).collect();
            let tgt_label = target_lang_label(&eff_target);
            items.push(ResultItem {
                id: "tr:open_query".into(),
                provider_id: PLUGIN_ID.into(),
                title: format!("翻译「{preview_snip}」→ {tgt_label}"),
                subtitle: "回车打开多源对照翻译窗（Google / 有道词典 / DeepL / OpenAI / Ollama，Esc 释放内存）".into(),
                category: ResultCategory::Plugin,
                score: 10_600,
                icon: IconSpec::Monogram("译".into()),
                badge: Some(tgt_label.into()),
                copy_text: Some(text.clone()),
                actions: vec![ActionSpec {
                    id: "open_tr".into(),
                    label: "多引擎并行翻译".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::PluginExecute {
                        plugin_id: PLUGIN_ID.into(),
                        action_id: "open_tr".into(),
                        payload: json!({ "text": text, "target_lang": eff_target }),
                    },
                }],
                preview: Some(PreviewSpec::Text {
                    title: format!("待翻译原文 → {tgt_label}"),
                    body: text.clone(),
                }),
            });

            let low = text.to_lowercase();
            for (i, wb) in store
                .wordbook
                .iter()
                .filter(|w| w.word.to_lowercase().contains(&low) || w.translation.to_lowercase().contains(&low))
                .take(4)
                .enumerate()
            {
                items.push(cache_item(
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

        let clip_text = ctx
            .clipboard
            .as_ref()
            .map(|c| c.text.trim().to_string())
            .filter(|s| !s.is_empty());

        items.push(ResultItem {
            id: "tr:open_empty".into(),
            provider_id: PLUGIN_ID.into(),
            title: match &clip_text {
                Some(c) => format!("打开多源翻译窗（带入剪贴板：{}）", clip_str(c, 36)),
                None => "打开多源翻译工作台（支持划词 Alt+T / 历史 / 生词本）".into(),
            },
            subtitle: "也可输入 `tr <原文>` 或 `tr en/zh/ja/ko <原文>` · 划词按 Alt+T 即时翻译".into(),
            category: ResultCategory::Plugin,
            score: 10_500,
            icon: IconSpec::Monogram("译".into()),
            badge: Some("Alt+T".into()),
            copy_text: clip_text.clone(),
            actions: vec![ActionSpec {
                id: "open_tr".into(),
                label: "打开翻译窗口".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "open_tr".into(),
                    payload: json!({ "text": clip_text.unwrap_or_default(), "target_lang": "" }),
                },
            }],
            preview: None,
        });

        for (i, h) in store.history.iter().take(6).enumerate() {
            items.push(cache_item(
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

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        if req.action_id == "open_tr" {
            let text = req.payload.get("text").and_then(Value::as_str).unwrap_or("");
            let target_lang = req.payload.get("target_lang").and_then(Value::as_str).unwrap_or("");
            return open_translator_response(text, target_lang);
        }
        ExecuteResponse::default()
    }

    fn on_event(&self, ev: &PluginEventPayload) -> ExecuteResponse {
        if ev.event == PluginEventKind::HotkeyTriggered {
            let selected = ev.data.get("selected_text").and_then(Value::as_str).unwrap_or("");
            return open_translator_response(selected, "");
        }
        ExecuteResponse::default()
    }

    fn call(&self, req: &PluginCallRequest) -> Result<Value, String> {
        let cfg = resolve_runtime_config(&req.plugin_settings);
        match req.method.as_str() {
            "get_init" => {
                let store = TranslateStore::load_default();
                let raw_q = req.params.get("initial_query").and_then(Value::as_str).unwrap_or("").trim();
                let (mut text, mut req_tgt) = parse_initial_query(raw_q);
                if text.is_empty() && req_tgt.is_empty() {
                    let (last_t, last_lang) = read_last_req();
                    text = last_t;
                    req_tgt = last_lang;
                }
                let target_lang = if req_tgt.is_empty() { cfg.target_lang } else { req_tgt };
                Ok(json!({
                    "text": text,
                    "target_lang": target_lang,
                    "engines": cfg.engines,
                    "history": store.history,
                    "wordbook": store.wordbook,
                }))
            }
            "translate" => {
                let text = req.params.get("text").and_then(Value::as_str).unwrap_or("");
                let src = req.params.get("source_lang").and_then(Value::as_str).unwrap_or("auto");
                let tgt = req.params.get("target_lang").and_then(Value::as_str).unwrap_or("");
                let (batch, store) = translate_parallel(&cfg, &req.plugin_secrets, text, src, tgt);
                Ok(json!({
                    "data": batch,
                    "store": { "history": store.history, "wordbook": store.wordbook },
                }))
            }
            "save_word" => {
                let mut store = TranslateStore::load_default();
                let w = req.params.get("word").and_then(Value::as_str).unwrap_or("");
                let tr = req.params.get("translation").and_then(Value::as_str).unwrap_or("");
                let ph = req.params.get("phonetic").and_then(Value::as_str).unwrap_or("");
                let note = req.params.get("note").and_then(Value::as_str).unwrap_or("");
                let ok = store.save_word(w, tr, ph, note);
                if ok {
                    store.save_default();
                }
                Ok(json!({ "ok": ok, "history": store.history, "wordbook": store.wordbook }))
            }
            "delete_word" => {
                let mut store = TranslateStore::load_default();
                let w = req.params.get("word").and_then(Value::as_str).unwrap_or("");
                let ok = store.delete_word(w);
                if ok {
                    store.save_default();
                }
                Ok(json!({ "ok": ok, "history": store.history, "wordbook": store.wordbook }))
            }
            "clear_history" => {
                let mut store = TranslateStore::load_default();
                store.clear_history();
                store.save_default();
                Ok(json!({ "ok": true, "history": store.history, "wordbook": store.wordbook }))
            }
            other => Err(format!("unsupported translator RPC method `{other}`")),
        }
    }
}

fn open_translator_response(text: &str, target_lang: &str) -> ExecuteResponse {
    let trimmed = text.trim();
    write_last_req(trimmed, target_lang);
    let initial_query = if trimmed.chars().count() <= 2000 {
        json!({ "text": trimmed, "target_lang": target_lang }).to_string()
    } else {
        String::new()
    };
    ExecuteResponse {
        hide_window: true,
        open_webview: Some(OpenWebViewRequest {
            entry: None,
            title: Some("Tomoyo 多源翻译".into()),
            initial_query,
            width: Some(780),
            height: Some(620),
        }),
        ..Default::default()
    }
}

fn parse_initial_query(raw: &str) -> (String, String) {
    if raw.is_empty() {
        return (String::new(), String::new());
    }
    if raw.starts_with('{')
        && let Ok(v) = serde_json::from_str::<Value>(raw)
    {
        let t = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let l = v.get("target_lang").and_then(Value::as_str).unwrap_or("").to_string();
        return (t, l);
    }
    (raw.to_string(), String::new())
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

fn cache_item(id: &str, title: &str, subtitle: &str, translated: &str, score: i64, badge: &str) -> ResultItem {
    ResultItem {
        id: id.into(),
        provider_id: PLUGIN_ID.into(),
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
                kind: ActionKind::CopyText { text: translated.into(), paste_immediately: true },
            },
            ActionSpec {
                id: "copy_tr".into(),
                label: "仅复制译文".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText { text: translated.into(), paste_immediately: false },
            },
        ],
        preview: Some(PreviewSpec::Text { title: "译文详情".into(), body: translated.into() }),
    }
}

fn clip_str(s: &str, max_chars: usize) -> String {
    let single: String = s.chars().map(|c| if matches!(c, '\r' | '\n' | '\t') { ' ' } else { c }).take(max_chars).collect();
    if s.chars().count() > max_chars { format!("{}…", single.trim()) } else { single.trim().into() }
}

fn main() {
    if std::env::args().any(|a| a == "--self-test") {
        let p = TranslatePlugin;
        let items = p.query(&QueryContext::new("tr en 你好世界", 1, 10));
        assert!(!items.is_empty());
        let init = p
            .call(&PluginCallRequest {
                method: "get_init".into(),
                params: json!({ "initial_query": "hello" }),
                plugin_settings: Default::default(),
                plugin_secrets: Default::default(),
            })
            .expect("get_init ok");
        assert_eq!(init["text"], "hello");
        println!("[OK] tomoyo-plugin-translate v{} self-test passed", env!("CARGO_PKG_VERSION"));
        return;
    }
    tomoyo_sdk::run(TranslatePlugin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefix_and_handles_selection_hotkey_event() {
        assert_eq!(parse_translate_args("ja hello"), (Some("ja".into()), "hello".into()));
        let resp = TranslatePlugin.on_event(&PluginEventPayload {
            event: PluginEventKind::HotkeyTriggered,
            data: json!({ "hotkey_id": "selection_translate", "selected_text": "resilient" }),
        });
        assert!(resp.hide_window);
        assert!(resp.open_webview.is_some());
    }
}
