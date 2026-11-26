//! Fuzzy/Pinyin search over installed plugins + quick direct jump into Settings Center.

use tomoyo_core::{score_match, ExternalPluginBridge, IndexedText, Lang, PluginSearchEntry};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, PreviewSpec, QueryContext,
    ResultCategory, ResultItem, SearchFilter,
};

fn is_settings_intent_token(tok_lower: &str) -> bool {
    matches!(
        tok_lower,
        "设置" | "配置" | "首选项" | "管理" | "setting" | "settings" | "config" | "cfg" | "pref" | "prefs" | "option" | "options"
    )
}

fn is_plugin_browse_token(tok_lower: &str) -> bool {
    matches!(tok_lower, "插件" | "plugin" | "plugins")
}

fn score_plugin_match(target: &str, p: &PluginSearchEntry) -> Option<i64> {
    let mut best: Option<i64> = None;
    let mut bump = |v: i64| {
        best = Some(best.map_or(v, |prev| prev.max(v)));
    };

    let name_idx = IndexedText::new(&p.name);
    if let Some(s) = score_match(target, &name_idx) {
        bump(s);
    }

    let low = target.to_ascii_lowercase();
    for kw in &p.keywords {
        let kw_low = kw.trim().to_ascii_lowercase();
        if kw_low.is_empty() {
            continue;
        }
        if kw_low == low {
            bump(9_200);
        } else if kw_low.starts_with(&low) && low.len() >= 2 {
            bump(7_800);
        } else if kw_low.contains(&low) && low.len() >= 2 {
            bump(6_400);
        }
    }

    let id_low = p.id.to_ascii_lowercase();
    if id_low == low {
        bump(8_800);
    } else if id_low.contains(&low) && low.len() >= 3 {
        bump(6_200);
    }

    if target.chars().count() >= 2 && !p.description.trim().is_empty() {
        let desc_idx = IndexedText::new(&p.description);
        if let Some(s) = score_match(target, &desc_idx) {
            bump((s / 2).max(200));
        }
    }
    best
}

fn build_plugin_card(
    provider_id: &str,
    p: &PluginSearchEntry,
    raw_score: i64,
    settings_intent: bool,
    l: Lang,
) -> ResultItem {
    let route = format!("plugins:{}", p.id);
    let settings_act = ActionSpec {
        id: "plugin_settings".into(),
        label: l.pick("打开插件设置", "Plugin Settings").into(),
        shortcut: Some(if settings_intent || !p.enabled { "Enter" } else { "Ctrl+Enter" }.into()),
        kind: ActionKind::InternalCommand {
            command: InternalCommandKind::OpenSettingsRoute { route },
        },
    };

    let primary_kw = p.keywords.iter().find(|k| !k.trim().is_empty()).map(|k| k.trim().to_string());
    let use_act = if p.enabled {
        if let Some(open_ui) = &p.ui_open_action {
            Some(ActionSpec {
                id: "plugin_open_ui".into(),
                label: l.pick("打开插件窗口", "Open Plugin Window").into(),
                shortcut: None,
                kind: open_ui.clone(),
            })
        } else {
            primary_kw.as_ref().map(|kw| ActionSpec {
                id: "plugin_fill_kw".into(),
                label: format!("{} ({kw} …)", l.pick("进入插件指令", "Use Keyword")),
                shortcut: None,
                kind: ActionKind::FillQuery { query: format!("{kw} ") },
            })
        }
    } else {
        None
    };

    let mut actions = Vec::new();
    if settings_intent || !p.enabled {
        actions.push(settings_act);
        if let Some(u) = use_act {
            actions.push(u);
        }
    } else if let Some(u) = use_act {
        let has_ui = matches!(u.kind, ActionKind::OpenWebView { .. });
        actions.push(u);
        actions.push(settings_act);
        if has_ui && let Some(kw) = &primary_kw {
            actions.push(ActionSpec {
                id: "plugin_fill_kw".into(),
                label: format!("{} ({kw} …)", l.pick("填充搜索关键词", "Fill Keyword")),
                shortcut: None,
                kind: ActionKind::FillQuery { query: format!("{kw} ") },
            });
        }
    } else {
        actions.push(settings_act);
    }

    let icon = match p.icon_path.as_deref() {
        Some(path) if !path.trim().is_empty() && std::path::Path::new(path).exists() => {
            IconSpec::ImageFile(path.to_string())
        }
        _ => IconSpec::Monogram(
            p.name.chars().take(2).collect::<String>().to_uppercase(),
        ),
    };

    let kw_str = if p.keywords.is_empty() {
        l.pick("无触发词", "No keyword").to_string()
    } else {
        p.keywords.join(" / ")
    };

    let (title, subtitle, badge) = if !p.enabled {
        (
            format!("{} ({})", p.name, l.pick("已禁用", "Disabled")),
            format!(
                "{} · {}",
                l.pick("Enter 打开插件设置以启用", "Enter opens Plugin Settings to enable"),
                p.id
            ),
            l.pick("已禁用", "OFF"),
        )
    } else if settings_intent {
        (
            format!("⚙ {} · {}", p.name, l.pick("插件设置", "Settings")),
            format!(
                "{} · {kw_str} · {}",
                p.id,
                l.pick("Enter 直达插件设置抽屉", "Enter opens settings drawer")
            ),
            l.pick("插件设置", "CFG"),
        )
    } else {
        let hint = if p.ui_open_action.is_some() {
            l.pick("Enter 打开独立窗口 · Ctrl+Enter 快捷设置", "Enter Open UI · Ctrl+Enter Settings")
        } else if primary_kw.is_some() {
            l.pick("Enter 填充关键词 · Ctrl+Enter 快捷设置", "Enter Fill Keyword · Ctrl+Enter Settings")
        } else {
            l.pick("Enter 打开插件设置", "Enter Open Plugin Settings")
        };
        (p.name.clone(), format!("{hint} · {kw_str}"), l.pick("插件", "PLUGIN"))
    };

    let score = if settings_intent {
        raw_score + 600
    } else if p.enabled {
        raw_score - 150
    } else {
        raw_score - 400
    };

    let preview_body = if p.description.trim().is_empty() {
        format!("ID: {}\nRuntime: {}\nKeywords: {kw_str}", p.id, p.runtime_badge)
    } else {
        format!("{}\n\nID: {}\nRuntime: {}\nKeywords: {kw_str}", p.description.trim(), p.id, p.runtime_badge)
    };

    ResultItem {
        id: format!("plugin-entry:{}", p.id),
        provider_id: provider_id.into(),
        title,
        subtitle,
        category: ResultCategory::Plugin,
        score,
        icon,
        badge: Some(badge.into()),
        copy_text: Some(p.id.clone()),
        actions,
        preview: Some(PreviewSpec::RichCard {
            title: p.name.clone(),
            subtitle: p.id.clone(),
            badges: vec![
                p.runtime_badge.clone(),
                if p.enabled { l.pick("已启用", "Enabled").into() } else { l.pick("已禁用", "Disabled").into() },
            ],
            rows: vec![
                (l.pick("触发词", "Keywords").into(), kw_str),
                (l.pick("运行形态", "Runtime").into(), p.runtime_badge.clone()),
            ],
            body: preview_body,
        }),
    }
}

pub(super) fn query_installed_plugins(
    provider_id: &str,
    bridge: &dyn ExternalPluginBridge,
    ctx: &QueryContext,
    lang: Lang,
) -> Vec<ResultItem> {
    if !matches!(ctx.filter, SearchFilter::All | SearchFilter::App) {
        return Vec::new();
    }
    let trimmed = ctx.trimmed.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') {
        return Vec::new();
    }

    let mut settings_intent = false;
    let mut plugin_browse = false;
    let mut rest_tokens: Vec<&str> = Vec::new();

    for tok in trimmed.split_whitespace() {
        let low = tok.to_ascii_lowercase();
        if is_settings_intent_token(&low) {
            settings_intent = true;
        } else if is_plugin_browse_token(&low) {
            plugin_browse = true;
        } else {
            rest_tokens.push(tok);
        }
    }

    let target = rest_tokens.join(" ");
    let entries = bridge.search_entries();
    if target.is_empty() {
        if !plugin_browse {
            return Vec::new();
        }
        return entries
            .into_iter()
            .take(12)
            .enumerate()
            .map(|(i, p)| {
                let base = if p.enabled { 8_200 } else { 7_400 } - (i as i64) * 20;
                build_plugin_card(provider_id, &p, base, settings_intent, lang)
            })
            .collect();
    }

    // Avoid 1-char ASCII noise unless explicitly browsing plugins/settings or exact keyword match.
    let single_ascii = target.is_ascii() && target.len() == 1;
    let mut out = Vec::new();
    for p in entries {
        if single_ascii && !settings_intent && !plugin_browse {
            let exact_kw = p.keywords.iter().any(|k| k.eq_ignore_ascii_case(&target));
            if !exact_kw {
                continue;
            }
        }
        if let Some(s) = score_plugin_match(&target, &p) {
            out.push(build_plugin_card(provider_id, &p, s, settings_intent, lang));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomoyo_core::PluginSummary;

    struct MockBridge(Vec<PluginSearchEntry>);

    impl ExternalPluginBridge for MockBridge {
        fn matches_keyword(&self, keyword: &str) -> bool {
            self.0.iter().any(|p| p.enabled && p.keywords.iter().any(|k| k.eq_ignore_ascii_case(keyword)))
        }
        fn query_keyword(&self, _: &str, _: &QueryContext) -> Vec<ResultItem> { Vec::new() }
        fn query_global(&self, _: &QueryContext) -> Vec<ResultItem> { Vec::new() }
        fn reload(&self) {}
        fn reap_idle(&self) {}
        fn summary_lines(&self) -> Vec<PluginSummary> { Vec::new() }
        fn search_entries(&self) -> Vec<PluginSearchEntry> { self.0.clone() }
    }

    fn sample_bridge() -> MockBridge {
        MockBridge(vec![
            PluginSearchEntry {
                id: "dev.tomoyo.notes".into(),
                name: "随手备忘录".into(),
                description: "Markdown 笔记本".into(),
                runtime_badge: "Hybrid".into(),
                keywords: vec!["note".into(), "bw".into()],
                enabled: true,
                icon_path: None,
                ui_open_action: Some(ActionKind::OpenWebView {
                    plugin_id: "dev.tomoyo.notes".into(),
                    title: "备忘录".into(),
                    entry_path: "index.html".into(),
                    utools_compat: false,
                    initial_query: String::new(),
                    width: 960,
                    height: 640,
                    max_memory_mb: 256,
                    icon_path: None,
                    display_mode: Some("window".into()),
                }),
            },
            PluginSearchEntry {
                id: "dev.tomoyo.capture".into(),
                name: "极速截图".into(),
                description: "滚动长截图与贴图".into(),
                runtime_badge: "STDIO".into(),
                keywords: vec!["cap".into(), "jt".into()],
                enabled: false,
                icon_path: None,
                ui_open_action: None,
            },
        ])
    }

    fn mk_ctx(q: &str) -> QueryContext {
        QueryContext {
            raw: q.into(),
            trimmed: q.into(),
            keyword: None,
            args: String::new(),
            generation: 1,
            limit: 10,
            clipboard: None,
            attached: None,
            filter: SearchFilter::All,
            plugin_settings: Default::default(),
            plugin_secrets: Default::default(),
        }
    }

    #[test]
    fn matches_plugin_by_chinese_pinyin_and_keyword() {
        let br = sample_bridge();
        let hits = query_installed_plugins("sys", &br, &mk_ctx("备忘录"), Lang::Zh);
        assert_eq!(hits.len(), 1);
        assert!(matches!(hits[0].actions[0].kind, ActionKind::OpenWebView { .. }));
        assert!(matches!(
            &hits[0].actions[1].kind,
            ActionKind::InternalCommand {
                command: InternalCommandKind::OpenSettingsRoute { route }
            } if route == "plugins:dev.tomoyo.notes"
        ));

        let py = query_installed_plugins("sys", &br, &mk_ctx("bwl"), Lang::Zh);
        assert_eq!(py.len(), 1);
    }

    #[test]
    fn settings_intent_promotes_plugin_settings_to_primary_action() {
        let br = sample_bridge();
        let hits = query_installed_plugins("sys", &br, &mk_ctx("备忘录 设置"), Lang::Zh);
        assert_eq!(hits.len(), 1);
        assert!(matches!(
            &hits[0].actions[0].kind,
            ActionKind::InternalCommand {
                command: InternalCommandKind::OpenSettingsRoute { route }
            } if route == "plugins:dev.tomoyo.notes"
        ));
    }

    #[test]
    fn disabled_plugin_prompts_settings_on_enter() {
        let br = sample_bridge();
        let hits = query_installed_plugins("sys", &br, &mk_ctx("截图"), Lang::Zh);
        assert_eq!(hits.len(), 1);
        assert!(matches!(
            &hits[0].actions[0].kind,
            ActionKind::InternalCommand {
                command: InternalCommandKind::OpenSettingsRoute { route }
            } if route == "plugins:dev.tomoyo.capture"
        ));
    }
}
