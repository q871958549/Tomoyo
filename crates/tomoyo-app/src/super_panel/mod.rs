//! Tomoyo Native Super Panel: uTools-style zero-WebView floating cursor panel with
//! lossless selection/file probing, instant inline translation, and Protocol v3 plugin actions.

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

use crate::renderer::Rgb;
use std::path::Path;
use tomoyo_core::{Lang, TomoyoConfig, PLUGIN_CAPTURE, PLUGIN_CLIPBOARD, PLUGIN_NOTES, PLUGIN_OCR, PLUGIN_TRANSLATE};
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::ActionKind;

#[derive(Debug, Clone)]
pub enum SuperActionKind {
    /// Expands or copies the inline translation inside the Super Panel.
    QuickTranslate,
    /// Opens `dev.tomoyo.translate` full multi-engine workbench for `text`.
    OpenTranslator(String),
    /// Copies text cleanly to clipboard (with optional toast).
    CopyText { text: String, toast: String },
    /// Opens an HTTP(S) URL in the default browser.
    OpenUrl(String),
    /// Searches `text` in the user's primary web search engine.
    WebSearch(String),
    /// Opens Tomoyo main launcher pre-filled with `query`.
    FillLauncher(String),
    /// Opens Tomoyo main launcher with attached file paths (Super Match).
    AttachFiles(Vec<String>),
    /// Launches a path / reveals in explorer / opens terminal via `ActionKind`.
    CoreAction(ActionKind),
    /// Opens Tomoyo Settings Center.
    OpenSettings,
}

#[derive(Debug, Clone)]
pub struct SuperAction {
    #[cfg_attr(not(test), allow(dead_code))]
    pub id: String,
    pub badge: String,
    pub badge_rgb: Rgb,
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
        target_lang: String,
    },
    Error { message: String },
}

/// Builds the ordered 2-column Super Panel action tiles tailored to the detected context.
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

    match ctx {
        SuperContext::Text { raw, kind } => {
            if has_tr {
                actions.push(SuperAction {
                    id: "sp:quick_tr".into(),
                    badge: "译".into(),
                    badge_rgb: Rgb(0x3B, 0x82, 0xF6),
                    label: lang.pick("快捷翻译", "Quick Translate").into(),
                    kind: SuperActionKind::QuickTranslate,
                });
                actions.push(SuperAction {
                    id: "sp:open_tr".into(),
                    badge: "对".into(),
                    badge_rgb: Rgb(0x63, 0x66, 0xF1),
                    label: lang.pick("多源对照翻译", "Multi-Engine").into(),
                    kind: SuperActionKind::OpenTranslator(raw.clone()),
                });
            }
            if *kind == TextKind::Url
                && let Some(url) = context_probe::normalize_url(raw)
            {
                actions.push(SuperAction {
                    id: "sp:open_url".into(),
                    badge: "链".into(),
                    badge_rgb: Rgb(0x10, 0xB9, 0x81),
                    label: lang.pick("浏览器打开链接", "Open URL").into(),
                    kind: SuperActionKind::OpenUrl(url),
                });
            }
            if *kind == TextKind::Path {
                let clean = raw.trim().trim_matches('"').to_string();
                actions.push(SuperAction {
                    id: "sp:open_path".into(),
                    badge: "径".into(),
                    badge_rgb: Rgb(0xF5, 0x9E, 0x0B),
                    label: lang.pick("打开目标路径", "Open Path").into(),
                    kind: SuperActionKind::CoreAction(ActionKind::LaunchPath {
                        path: clean.clone(),
                        args: None,
                        working_dir: None,
                    }),
                });
                actions.push(SuperAction {
                    id: "sp:reveal_path".into(),
                    badge: "定".into(),
                    badge_rgb: Rgb(0x0E, 0xA5, 0xE9),
                    label: lang.pick("资源管理器定位", "Reveal File").into(),
                    kind: SuperActionKind::CoreAction(ActionKind::RevealInExplorer { path: clean }),
                });
            }
            if *kind == TextKind::Color && has_cap {
                actions.push(SuperAction {
                    id: "sp:pin_color".into(),
                    badge: "色".into(),
                    badge_rgb: Rgb(0xEC, 0x48, 0x99),
                    label: lang.pick("桌面贴图色卡", "Pin Color Card").into(),
                    kind: SuperActionKind::CoreAction(ActionKind::PluginExecute {
                        plugin_id: PLUGIN_CAPTURE.into(),
                        action_id: "pin".into(),
                        payload: serde_json::json!({ "text": raw }),
                    }),
                });
            }
            actions.push(SuperAction {
                id: "sp:web_search".into(),
                badge: "搜".into(),
                badge_rgb: Rgb(0x06, 0xB6, 0xD4),
                label: lang.pick("网页搜索", "Web Search").into(),
                kind: SuperActionKind::WebSearch(raw.clone()),
            });
            actions.push(SuperAction {
                id: "sp:copy_plain".into(),
                badge: "复".into(),
                badge_rgb: Rgb(0x64, 0x74, 0x8B),
                label: lang.pick("复制纯文本", "Copy Plain Text").into(),
                kind: SuperActionKind::CopyText {
                    text: raw.clone(),
                    toast: lang.pick("✓ 已复制纯文本", "✓ Copied plain text").into(),
                },
            });
            if has_notes {
                let first_line: String = raw.replace(['\r', '\n'], " ").chars().take(180).collect();
                actions.push(SuperAction {
                    id: "sp:save_note".into(),
                    badge: "记".into(),
                    badge_rgb: Rgb(0xF5, 0x9E, 0x0B),
                    label: lang.pick("存为本地速记", "Save Quick Note").into(),
                    kind: SuperActionKind::FillLauncher(format!("note + {first_line}")),
                });
            }
            let snip: String = raw.replace(['\r', '\n'], " ").chars().take(120).collect();
            actions.push(SuperAction {
                id: "sp:search_tomoyo".into(),
                badge: "T".into(),
                badge_rgb: Rgb(0x4F, 0x46, 0xE5),
                label: lang.pick("送入 Tomoyo 搜索", "Search in Tomoyo").into(),
                kind: SuperActionKind::FillLauncher(snip),
            });

            // Protocol v3 plugin declarative `[[text_actions]]`
            for spec in plugin_host.text_actions_for(raw, kind.as_str()) {
                if actions.len() >= 10 {
                    break;
                }
                if actions.iter().any(|a| a.label == spec.label) {
                    continue;
                }
                actions.push(SuperAction {
                    id: spec.id,
                    badge: "插".into(),
                    badge_rgb: Rgb(0x8B, 0x5C, 0xF6),
                    label: spec.label,
                    kind: SuperActionKind::CoreAction(spec.kind),
                });
            }
        }
        SuperContext::Files { paths, all_images } => {
            let joined = paths.join("\r\n");
            let first = paths.first().cloned().unwrap_or_default();
            let dir = Path::new(&first)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| first.clone());

            actions.push(SuperAction {
                id: "sp:file_match".into(),
                badge: "T".into(),
                badge_rgb: Rgb(0x4F, 0x46, 0xE5),
                label: lang.pick("Tomoyo 智能匹配", "Tomoyo Super Match").into(),
                kind: SuperActionKind::AttachFiles(paths.clone()),
            });
            actions.push(SuperAction {
                id: "sp:copy_paths".into(),
                badge: "径".into(),
                badge_rgb: Rgb(0x0E, 0xA5, 0xE9),
                label: lang.pick("复制完整路径", "Copy Full Path").into(),
                kind: SuperActionKind::CopyText {
                    text: joined,
                    toast: lang.pick("✓ 已复制文件路径", "✓ Copied file path(s)").into(),
                },
            });
            actions.push(SuperAction {
                id: "sp:open_term".into(),
                badge: ">_".into(),
                badge_rgb: Rgb(0x10, 0xB9, 0x81),
                label: lang.pick("在此打开终端", "Open Terminal Here").into(),
                kind: SuperActionKind::CoreAction(ActionKind::OpenTerminalAt { dir }),
            });
            if *all_images && has_ocr {
                actions.push(SuperAction {
                    id: "sp:img_ocr".into(),
                    badge: "字".into(),
                    badge_rgb: Rgb(0x8B, 0x5C, 0xF6),
                    label: lang.pick("离线识图 OCR", "Offline OCR").into(),
                    kind: SuperActionKind::CoreAction(ActionKind::PluginExecute {
                        plugin_id: PLUGIN_OCR.into(),
                        action_id: "ocr_file".into(),
                        payload: serde_json::json!({ "path": first }),
                    }),
                });
            }
            if *all_images && has_cap {
                actions.push(SuperAction {
                    id: "sp:img_pin".into(),
                    badge: "贴".into(),
                    badge_rgb: Rgb(0xEC, 0x48, 0x99),
                    label: lang.pick("桌面一键贴图", "Pin Image").into(),
                    kind: SuperActionKind::CoreAction(ActionKind::PluginExecute {
                        plugin_id: PLUGIN_CAPTURE.into(),
                        action_id: "pin".into(),
                        payload: serde_json::json!({ "path": first }),
                    }),
                });
            }
            if paths.len() == 1 {
                for spec in plugin_host.file_actions_for_path(&first) {
                    if actions.len() >= 10 {
                        break;
                    }
                    actions.push(SuperAction {
                        id: spec.id,
                        badge: "插".into(),
                        badge_rgb: Rgb(0xF5, 0x9E, 0x0B),
                        label: spec.label,
                        kind: SuperActionKind::CoreAction(spec.kind),
                    });
                }
            }
        }
        SuperContext::Empty => {
            if has_cap {
                actions.push(SuperAction {
                    id: "sp:cap_region".into(),
                    badge: "截".into(),
                    badge_rgb: Rgb(0x3B, 0x82, 0xF6),
                    label: lang.pick("区域截图", "Region Capture").into(),
                    kind: SuperActionKind::CoreAction(ActionKind::PluginExecute {
                        plugin_id: PLUGIN_CAPTURE.into(),
                        action_id: "region".into(),
                        payload: serde_json::json!({}),
                    }),
                });
                actions.push(SuperAction {
                    id: "sp:cap_pin".into(),
                    badge: "贴".into(),
                    badge_rgb: Rgb(0xEC, 0x48, 0x99),
                    label: lang.pick("剪贴板贴图", "Pin Clipboard").into(),
                    kind: SuperActionKind::CoreAction(ActionKind::PluginExecute {
                        plugin_id: PLUGIN_CAPTURE.into(),
                        action_id: "pin".into(),
                        payload: serde_json::json!({}),
                    }),
                });
            }
            if has_ocr {
                actions.push(SuperAction {
                    id: "sp:ocr".into(),
                    badge: "字".into(),
                    badge_rgb: Rgb(0x8B, 0x5C, 0xF6),
                    label: lang.pick("截图识图取字", "Screen OCR").into(),
                    kind: SuperActionKind::CoreAction(ActionKind::PluginExecute {
                        plugin_id: PLUGIN_CAPTURE.into(),
                        action_id: "ocr".into(),
                        payload: serde_json::json!({}),
                    }),
                });
            }
            if has_tr {
                actions.push(SuperAction {
                    id: "sp:tr_empty".into(),
                    badge: "译".into(),
                    badge_rgb: Rgb(0x63, 0x66, 0xF1),
                    label: lang.pick("多源对照翻译", "Translator").into(),
                    kind: SuperActionKind::OpenTranslator(String::new()),
                });
            }
            if has_clip {
                actions.push(SuperAction {
                    id: "sp:clip".into(),
                    badge: "剪".into(),
                    badge_rgb: Rgb(0x0E, 0xA5, 0xE9),
                    label: lang.pick("剪贴板历史", "Clipboard History").into(),
                    kind: SuperActionKind::FillLauncher("cb ".into()),
                });
            }
            if has_notes {
                actions.push(SuperAction {
                    id: "sp:notes".into(),
                    badge: "记".into(),
                    badge_rgb: Rgb(0xF5, 0x9E, 0x0B),
                    label: lang.pick("快速备忘录", "Quick Notes").into(),
                    kind: SuperActionKind::FillLauncher("note ".into()),
                });
            }
            actions.push(SuperAction {
                id: "sp:launcher".into(),
                badge: "T".into(),
                badge_rgb: Rgb(0x4F, 0x46, 0xE5),
                label: lang.pick("呼出 Tomoyo", "Open Tomoyo").into(),
                kind: SuperActionKind::FillLauncher(String::new()),
            });
            actions.push(SuperAction {
                id: "sp:settings".into(),
                badge: "设".into(),
                badge_rgb: Rgb(0x64, 0x74, 0x8B),
                label: lang.pick("设置中心", "Settings Center").into(),
                kind: SuperActionKind::OpenSettings,
            });
        }
    }

    actions.truncate(10);
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_context_tailored_actions_for_text_files_and_empty() {
        let cfg = TomoyoConfig::default();
        let host = PluginHost::new(vec![]);
        let text_acts = build_actions_for_context(
            &SuperContext::Text { raw: "hello world".into(), kind: TextKind::Plain },
            &cfg,
            &host,
            Lang::Zh,
        );
        assert!(text_acts.iter().any(|a| a.id == "sp:quick_tr"));
        assert!(text_acts.iter().any(|a| a.id == "sp:web_search"));

        let url_acts = build_actions_for_context(
            &SuperContext::Text { raw: "https://example.com".into(), kind: TextKind::Url },
            &cfg,
            &host,
            Lang::Zh,
        );
        assert!(url_acts.iter().any(|a| a.id == "sp:open_url"));

        let empty_acts = build_actions_for_context(&SuperContext::Empty, &cfg, &host, Lang::Zh);
        assert!(empty_acts.iter().any(|a| a.id == "sp:cap_region"));
        assert!(empty_acts.iter().any(|a| a.id == "sp:launcher"));
    }
}
