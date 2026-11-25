//! Tomoyo Native Super Panel: minimalist zero-WebView cursor popover with lossless
//! selection/file probing, instant inline translation, and Protocol v3 plugin actions.

pub mod context_probe;
pub mod inline_trans;
pub mod panel_layout;
pub mod panel_paint;
pub mod panel_wnd;
pub mod trigger;

pub use context_probe::{probe_foreground_selection, SuperContext, TextKind};
pub use panel_wnd::{close_active_panel, open_super_panel, SuperPanelExec, SuperPanelLaunch};
pub use trigger::{
    consume_hold_if_valid, is_panel_open, should_avoid_super_panel, sync_trigger_hook,
    uninstall_trigger_hook,
};

use std::path::Path;
use tomoyo_core::{Lang, TomoyoConfig, PLUGIN_CAPTURE, PLUGIN_CLIPBOARD, PLUGIN_NOTES, PLUGIN_OCR, PLUGIN_TRANSLATE};
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::ActionKind;

#[derive(Debug, Clone)]
pub enum SuperActionKind {
    QuickTranslate,
    OpenTranslator(String),
    CopyText { text: String, toast: String },
    OpenUrl(String),
    WebSearch(String),
    FillLauncher(String),
    AttachFiles(Vec<String>),
    CoreAction(ActionKind),
}

#[derive(Debug, Clone)]
pub struct SuperAction {
    #[cfg_attr(not(test), allow(dead_code))]
    pub id: String,
    pub glyph: String,
    pub label: String,
    pub kind: SuperActionKind,
}

#[derive(Debug, Clone)]
pub enum InlineTransState {
    Hidden,
    Loading,
    Ready {
        translated: String,
        engine_name: String,
        phonetic: Option<String>,
    },
    Error { message: String },
}

/// Builds a concise, minimalist list of up to 6 contextual actions for the Super Panel.
pub fn build_actions_for_context(
    ctx: &SuperContext,
    cfg: &TomoyoConfig,
    plugin_host: &PluginHost,
    lang: Lang,
) -> Vec<SuperAction> {
    let mut actions = Vec::new();
    let has_tr = cfg.is_plugin_enabled(PLUGIN_TRANSLATE);
    let has_notes = cfg.is_plugin_enabled(PLUGIN_NOTES);
    let has_cap = cfg.is_plugin_enabled(PLUGIN_CAPTURE);
    let has_ocr = cfg.is_plugin_enabled(PLUGIN_OCR);
    let has_clip = cfg.is_plugin_enabled(PLUGIN_CLIPBOARD);
    let auto_tr = cfg.super_panel.auto_translate_selection;

    match ctx {
        SuperContext::Text { raw, kind } => {
            if has_tr {
                if auto_tr {
                    actions.push(act("sp:open_tr", "译", lang.pick("多源对照翻译", "Multi-Engine Translate"), SuperActionKind::OpenTranslator(raw.clone())));
                } else {
                    actions.push(act("sp:quick_tr", "译", lang.pick("快捷翻译", "Quick Translate"), SuperActionKind::QuickTranslate));
                }
            }
            if *kind == TextKind::Url
                && let Some(url) = context_probe::normalize_url(raw)
            {
                actions.push(act("sp:open_url", "↗", lang.pick("打开链接", "Open URL"), SuperActionKind::OpenUrl(url)));
            }
            if *kind == TextKind::Path {
                let clean = raw.trim().trim_matches('"').to_string();
                actions.push(act(
                    "sp:open_path",
                    "▸",
                    lang.pick("打开路径", "Open Path"),
                    SuperActionKind::CoreAction(ActionKind::LaunchPath { path: clean, args: None, working_dir: None }),
                ));
            }
            if *kind == TextKind::Color && has_cap {
                actions.push(act(
                    "sp:pin_color",
                    "◈",
                    lang.pick("贴图色卡", "Pin Color Card"),
                    SuperActionKind::CoreAction(ActionKind::PluginExecute {
                        plugin_id: PLUGIN_CAPTURE.into(),
                        action_id: "pin".into(),
                        payload: serde_json::json!({ "text": raw }),
                    }),
                ));
            }
            for spec in plugin_host.text_actions_for(raw, kind.as_str()) {
                if actions.len() >= 4 {
                    break;
                }
                if !actions.iter().any(|a| a.label == spec.label) {
                    actions.push(act(&spec.id, "✦", &spec.label, SuperActionKind::CoreAction(spec.kind)));
                }
            }
            actions.push(act("sp:web_search", "⌕", lang.pick("网页搜索", "Web Search"), SuperActionKind::WebSearch(raw.clone())));
            if has_notes {
                let snip: String = raw.replace(['\r', '\n'], " ").chars().take(180).collect();
                actions.push(act("sp:save_note", "✎", lang.pick("存为速记", "Save Note"), SuperActionKind::FillLauncher(format!("note + {snip}"))));
            }
            actions.push(act(
                "sp:copy_plain",
                "❐",
                lang.pick("复制纯文本", "Copy Plain Text"),
                SuperActionKind::CopyText {
                    text: raw.clone(),
                    toast: lang.pick("✓ 已复制纯文本", "✓ Copied plain text").into(),
                },
            ));
            let snip: String = raw.replace(['\r', '\n'], " ").chars().take(120).collect();
            actions.push(act("sp:search_tomoyo", "T", lang.pick("送入 Tomoyo", "Send to Tomoyo"), SuperActionKind::FillLauncher(snip)));
        }
        SuperContext::Files { paths, all_images } => {
            let joined = paths.join("\r\n");
            let first = paths.first().cloned().unwrap_or_default();
            let dir = Path::new(&first)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| first.clone());

            actions.push(act("sp:file_match", "T", lang.pick("Tomoyo 智能匹配", "Tomoyo Match"), SuperActionKind::AttachFiles(paths.clone())));
            actions.push(act(
                "sp:copy_paths",
                "❐",
                lang.pick("复制路径", "Copy Path"),
                SuperActionKind::CopyText {
                    text: joined,
                    toast: lang.pick("✓ 已复制文件路径", "✓ Copied path(s)").into(),
                },
            ));
            actions.push(act("sp:open_term", ">", lang.pick("在此打开终端", "Open Terminal"), SuperActionKind::CoreAction(ActionKind::OpenTerminalAt { dir })));
            if *all_images && has_ocr {
                actions.push(act(
                    "sp:img_ocr",
                    "字",
                    lang.pick("离线识图 OCR", "Offline OCR"),
                    SuperActionKind::CoreAction(ActionKind::PluginExecute {
                        plugin_id: PLUGIN_OCR.into(),
                        action_id: "ocr_file".into(),
                        payload: serde_json::json!({ "path": first }),
                    }),
                ));
            }
            if *all_images && has_cap {
                actions.push(act(
                    "sp:img_pin",
                    "◈",
                    lang.pick("桌面贴图", "Pin Image"),
                    SuperActionKind::CoreAction(ActionKind::PluginExecute {
                        plugin_id: PLUGIN_CAPTURE.into(),
                        action_id: "pin".into(),
                        payload: serde_json::json!({ "path": first }),
                    }),
                ));
            }
            if paths.len() == 1 {
                for spec in plugin_host.file_actions_for_path(&first) {
                    if actions.len() >= 6 {
                        break;
                    }
                    actions.push(act(&spec.id, "✦", &spec.label, SuperActionKind::CoreAction(spec.kind)));
                }
            }
        }
        SuperContext::Empty => {
            if has_cap {
                actions.push(act("sp:cap_region", "✂", lang.pick("区域截图", "Region Capture"), SuperActionKind::CoreAction(ActionKind::PluginExecute {
                    plugin_id: PLUGIN_CAPTURE.into(), action_id: "region".into(), payload: serde_json::json!({}),
                })));
                actions.push(act("sp:cap_pin", "◈", lang.pick("桌面贴图", "Pin Clipboard"), SuperActionKind::CoreAction(ActionKind::PluginExecute {
                    plugin_id: PLUGIN_CAPTURE.into(), action_id: "pin".into(), payload: serde_json::json!({}),
                })));
            }
            if has_tr {
                actions.push(act("sp:tr_empty", "译", lang.pick("多源翻译", "Translator"), SuperActionKind::OpenTranslator(String::new())));
            }
            if has_clip {
                actions.push(act("sp:clip", "❐", lang.pick("剪贴板历史", "Clipboard"), SuperActionKind::FillLauncher("cb ".into())));
            }
            if has_notes {
                actions.push(act("sp:notes", "✎", lang.pick("快速备忘录", "Quick Notes"), SuperActionKind::FillLauncher("note ".into())));
            }
            actions.push(act("sp:launcher", "T", lang.pick("呼出 Tomoyo", "Open Tomoyo"), SuperActionKind::FillLauncher(String::new())));
        }
    }

    actions.truncate(6);
    actions
}

fn act(id: &str, glyph: &str, label: &str, kind: SuperActionKind) -> SuperAction {
    SuperAction {
        id: id.into(),
        glyph: glyph.into(),
        label: label.into(),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_minimalist_contextual_actions_capped_at_six() {
        let cfg = TomoyoConfig::default();
        let host = PluginHost::new(vec![]);
        let text_acts = build_actions_for_context(
            &SuperContext::Text { raw: "hello world".into(), kind: TextKind::Plain },
            &cfg,
            &host,
            Lang::Zh,
        );
        assert!(text_acts.len() <= 6);
        assert!(text_acts.iter().any(|a| a.id == "sp:open_tr"));
        assert!(text_acts.iter().any(|a| a.id == "sp:web_search"));

        let url_acts = build_actions_for_context(
            &SuperContext::Text { raw: "https://example.com".into(), kind: TextKind::Url },
            &cfg,
            &host,
            Lang::Zh,
        );
        assert!(url_acts.iter().any(|a| a.id == "sp:open_url"));

        let empty_acts = build_actions_for_context(&SuperContext::Empty, &cfg, &host, Lang::Zh);
        assert!(empty_acts.len() <= 6);
        assert!(empty_acts.iter().any(|a| a.id == "sp:cap_region"));
    }
}
