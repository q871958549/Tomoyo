//! Official Tomoyo STDIO Plugin: Cross-Platform Window Layout & Foreground Context (`dev.tomoyo.winlayout`).

mod apply;
mod layouts;

use apply::{apply_preset, probe_active_window};
use layouts::PRESETS;
use serde_json::Value;
use std::collections::BTreeMap;
use tomoyo_sdk::{
    ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse, IconSpec, PluginMeta, QueryContext,
    ResultCategory, ResultItem, TomoyoPlugin,
};

const PID: &str = "dev.tomoyo.winlayout";

struct WinLayoutPlugin;

fn runtime_prefs(settings: &BTreeMap<String, Value>) -> (i32, i32) {
    let gap = settings
        .get("gap_px")
        .and_then(Value::as_i64)
        .map(|n| (n as i32).clamp(0, 48))
        .unwrap_or(0);
    let pct = settings
        .get("center_ratio")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.trim().parse().ok()))
        .map(|n| (n as i32).clamp(50, 95))
        .unwrap_or(72);
    (gap, pct)
}

fn layout_action(id: &str, label: &str, shortcut: Option<&str>, preset: &str, gap: i32, pct: i32) -> ActionSpec {
    ActionSpec {
        id: id.into(),
        label: label.into(),
        shortcut: shortcut.map(str::to_string),
        kind: ActionKind::PluginExecute {
            plugin_id: PID.into(),
            action_id: id.into(),
            payload: serde_json::json!({ "preset": preset, "gap": gap, "pct": pct }),
        },
    }
}

impl TomoyoPlugin for WinLayoutPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("窗口布局与上下文", env!("CARGO_PKG_VERSION"))
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let needle = ctx.args.trim().to_ascii_lowercase();
        let terms: Vec<&str> = needle.split_whitespace().filter(|t| !t.is_empty()).collect();
        let (gap_px, center_pct) = runtime_prefs(&ctx.plugin_settings);
        let mut out = Vec::new();

        if let Some(active) = probe_active_window() {
            let meta_line = format!("[{}] {}", active.app, active.title);
            out.push(ResultItem {
                id: "wl:ctx:active".into(),
                provider_id: PID.into(),
                title: format!("当前活动窗口: {}", active.title),
                subtitle: format!("{} · 回车居中专注 ({center_pct}%) · Ctrl+Enter 复制标题", active.app),
                category: ResultCategory::Plugin,
                score: 9_950,
                icon: IconSpec::Monogram("CTX".into()),
                badge: Some("ACTIVE".into()),
                copy_text: Some(meta_line.clone()),
                actions: vec![
                    layout_action("apply:center", "居中专注当前窗口", Some("Enter"), "center", gap_px, center_pct),
                    layout_action("apply:always-top", "切换窗口置顶", Some("Alt+Enter"), "always-top", gap_px, center_pct),
                    ActionSpec {
                        id: "copy_ctx".into(),
                        label: "复制当前窗口信息".into(),
                        shortcut: Some("Ctrl+Enter".into()),
                        kind: ActionKind::CopyText { text: meta_line, paste_immediately: false },
                    },
                ],
                preview: None,
            });
        }

        let mut scored: Vec<(i64, &layouts::LayoutPreset)> = PRESETS
            .iter()
            .filter_map(|p| {
                if terms.is_empty() {
                    return Some((9_400, p));
                }
                let hay = format!("{} {} {} {}", p.id, p.title, p.subtitle, p.aliases).to_ascii_lowercase();
                if terms.iter().all(|t| hay.contains(t)) {
                    let boost = if p.id == needle { 9_900 } else if p.id.starts_with(&needle) { 9_750 } else { 9_450 };
                    Some((boost, p))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by_key(|a| std::cmp::Reverse(a.0));

        for (i, (score, p)) in scored.into_iter().take(ctx.limit.max(6)).enumerate() {
            let aid = format!("apply:{}", p.id);
            out.push(ResultItem {
                id: format!("wl:{}", p.id),
                provider_id: PID.into(),
                title: p.title.into(),
                subtitle: format!("{} · 边距 {gap_px}px", p.subtitle),
                category: ResultCategory::Plugin,
                score: score - i as i64,
                icon: IconSpec::Monogram(p.badge.split_whitespace().next().unwrap_or("WL").into()),
                badge: Some(p.badge.into()),
                copy_text: Some(format!("win {}", p.id)),
                actions: vec![layout_action(&aid, "应用窗口布局", Some("Enter"), p.id, gap_px, center_pct)],
                preview: None,
            });
        }
        out
    }

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        let preset = req
            .action_id
            .strip_prefix("apply:")
            .map(str::to_string)
            .or_else(|| req.payload.get("preset").and_then(Value::as_str).map(str::to_string));
        if let Some(id) = preset {
            let gap = req.payload.get("gap").and_then(Value::as_i64).unwrap_or(0) as i32;
            let pct = req.payload.get("pct").and_then(Value::as_i64).unwrap_or(72) as i32;
            match apply_preset(&id, gap, pct) {
                Ok(label) => {
                    return ExecuteResponse {
                        hide_window: true,
                        toast: Some(format!("已应用窗口布局：{label}")),
                        ..Default::default()
                    };
                }
                Err(e) => {
                    return ExecuteResponse {
                        hide_window: false,
                        toast: Some(e),
                        ..Default::default()
                    };
                }
            }
        }
        ExecuteResponse::default()
    }
}

fn main() {
    if std::env::args().any(|a| a == "--self-test") {
        let mut ctx = QueryContext::new("win left", 1, 10);
        ctx.keyword = Some("win".into());
        ctx.args = "left".into();
        let hits = WinLayoutPlugin.query(&ctx);
        assert!(hits.iter().any(|r| r.id == "wl:left"));
        let kwin_js = apply::build_kwin_layout_js("center", 8, 72);
        assert!(kwin_js.contains("Object.assign"));
        println!("[OK] tomoyo-plugin-winlayout v{} self-test passed ({} presets)", env!("CARGO_PKG_VERSION"), PRESETS.len());
        return;
    }
    tomoyo_sdk::run(WinLayoutPlugin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_english_and_chinese_layout_queries_and_prefs() {
        let mut ctx = QueryContext::new("win 左", 1, 10);
        ctx.keyword = Some("win".into());
        ctx.args = "左".into();
        ctx.plugin_settings.insert("gap_px".into(), Value::from(12));
        ctx.plugin_settings.insert("center_ratio".into(), Value::from("80"));
        let hits = WinLayoutPlugin.query(&ctx);
        assert!(hits.iter().any(|r| r.id == "wl:left" && r.subtitle.contains("边距 12px")));

        let (g, p) = runtime_prefs(&ctx.plugin_settings);
        assert_eq!((g, p), (12, 80));
    }
}
