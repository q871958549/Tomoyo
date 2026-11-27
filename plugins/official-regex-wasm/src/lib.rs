//! Official Zero-Permission Pure-WASM Regex Lab (`dev.tomoyo.regex`).
//!
//! Runs inside Tomoyo's `wasmi` fuel-metered sandbox (16 MB cap, 0 ambient OS permissions).
//! Keywords: `re`, `regex`.

mod engine;
mod parser;
mod presets;

use engine::{highlight_matches, CompiledRegex};
use presets::PRESETS;
use tomoyo_wasm_sdk::{
    export_tomoyo_wasm_plugin, ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse, IconSpec,
    PreviewSpec, QueryContext, ResultCategory, ResultItem, TomoyoWasmPlugin,
};

const PID: &str = "dev.tomoyo.regex";

struct OfficialRegexLab;

impl TomoyoWasmPlugin for OfficialRegexLab {
    fn query(ctx: &QueryContext) -> Vec<ResultItem> {
        let args = ctx.args.trim();
        if args.is_empty() {
            return build_overview_and_presets("");
        }
        if let Some(rest) = args.strip_prefix(':') {
            let (id_part, sample_part) = match rest.split_once(char::is_whitespace) {
                Some((a, b)) => (a.trim(), b.trim()),
                None => (rest.trim(), ""),
            };
            if let Some(p) = PRESETS.iter().find(|p| p.id.eq_ignore_ascii_case(id_part)) {
                let sample = if sample_part.is_empty() { p.sample } else { sample_part };
                return evaluate_regex(p.pattern, p.flags, sample);
            }
            let matching = build_overview_and_presets(id_part);
            if !matching.is_empty() {
                return matching;
            }
        }

        if let Some((pat, flags, sample)) = parse_regex_input(args) {
            return evaluate_regex(pat, flags, sample);
        }

        vec![ResultItem {
            id: "re:syntax_hint".into(),
            provider_id: PID.into(),
            title: "输入格式：`re /<模式>/<标志> <测试文本>` 或 `re :<预设>`".into(),
            subtitle: "例：re /(\\w+)@([\\w.]+)/g alice@tomoyo.dev · 支持 g/i 标志与最多 9 个捕获组".into(),
            category: ResultCategory::Plugin,
            score: 9_600,
            icon: IconSpec::Monogram("RE".into()),
            badge: Some("SYNTAX".into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "try_email".into(),
                label: "试用预设：re :email".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::FillQuery { query: "re :email".into() },
            }],
            preview: None,
        }]
    }

    fn execute(req: &ExecuteRequest) -> ExecuteResponse {
        if req.action_id == "test_regex" {
            let raw = req.payload.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
            let single: String = raw.replace(['\r', '\n'], " ").chars().take(180).collect();
            return ExecuteResponse {
                hide_window: false,
                fill_query: Some(format!("re /(\\S+)/g {single}")),
                ..Default::default()
            };
        }
        ExecuteResponse::default()
    }
}

fn parse_regex_input(raw: &str) -> Option<(&str, &str, &str)> {
    if let Some(after_slash) = raw.strip_prefix('/') {
        let bytes = after_slash.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\\' { i += 2; continue; }
            if bytes[i] == b'/' {
                let pat = &after_slash[..i];
                let rest = &after_slash[i + 1..];
                let (flags, sample) = match rest.split_once(char::is_whitespace) {
                    Some((f, s)) => (f.trim(), s.trim()),
                    None => (rest.trim(), ""),
                };
                return Some((pat, if flags.is_empty() { "g" } else { flags }, sample));
            }
            i += 1;
        }
    }
    if let Some((pat, sample)) = raw.split_once("::") {
        return Some((pat.trim(), "g", sample.trim()));
    }
    None
}

fn evaluate_regex(pattern: &str, flags: &str, sample: &str) -> Vec<ResultItem> {
    let ci = flags.contains('i') || flags.contains('I');
    let global = flags.contains('g') || flags.contains('G') || flags.is_empty();
    let re = match CompiledRegex::compile(pattern, ci) {
        Ok(r) => r,
        Err(err) => {
            return vec![ResultItem {
                id: "re:compile_err".into(),
                provider_id: PID.into(),
                title: format!("正则语法错误：{err}"),
                subtitle: format!("/{pattern}/{flags}"),
                category: ResultCategory::Plugin,
                score: 9_800,
                icon: IconSpec::Monogram("!".into()),
                badge: Some("ERR".into()),
                copy_text: Some(pattern.into()),
                actions: vec![],
                preview: None,
            }];
        }
    };

    let matches = match re.find_matches(sample, global) {
        Ok(m) => m,
        Err(err) => {
            return vec![ResultItem {
                id: "re:runtime_err".into(),
                provider_id: PID.into(),
                title: format!("匹配安全中止：{err}"),
                subtitle: "为防止灾难性回溯阻塞沙箱，单次匹配限制 40,000 步".into(),
                category: ResultCategory::Plugin,
                score: 9_800,
                icon: IconSpec::Monogram("!".into()),
                badge: Some("GUARD".into()),
                copy_text: None,
                actions: vec![],
                preview: None,
            }];
        }
    };

    let hl = highlight_matches(sample, &matches);
    let mut rows = vec![
        ("表达式".into(), format!("/{pattern}/{flags}")),
        ("匹配数".into(), format!("{} 处", matches.len())),
    ];
    if let Some(first) = matches.first() {
        rows.push(("$0 (首匹配)".into(), first.text.clone()));
        for (gi, g) in first.groups.iter().enumerate().take(4) {
            rows.push((format!("${}", gi + 1), g.clone().unwrap_or_else(|| "<未捕获>".into())));
        }
    }

    let mut out = vec![ResultItem {
        id: format!("re:summary:{}", matches.len()),
        provider_id: PID.into(),
        title: if matches.is_empty() {
            format!("未匹配：/{pattern}/{flags}")
        } else {
            format!("命中 {} 处 · 首匹配「{}」", matches.len(), matches[0].text)
        },
        subtitle: hl.chars().take(110).collect(),
        category: ResultCategory::Plugin,
        score: 9_900,
        icon: IconSpec::Monogram("RE".into()),
        badge: Some(format!("{} HIT", matches.len())),
        copy_text: Some(matches.first().map(|m| m.text.clone()).unwrap_or_else(|| pattern.into())),
        actions: vec![
            ActionSpec {
                id: "copy_pat".into(),
                label: "复制正则表达式".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText { text: pattern.into(), paste_immediately: false },
            },
            ActionSpec {
                id: "copy_hl".into(),
                label: "复制高亮预览文本".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText { text: hl.clone(), paste_immediately: false },
            },
        ],
        preview: Some(PreviewSpec::RichCard {
            title: "Regex Lab · 实时匹配与捕获组".into(),
            subtitle: format!("/{pattern}/{flags}"),
            badges: vec![format!("{} matches", matches.len()), "WASM".into(), "ReDoS Guard".into()],
            rows,
            body: format!("高亮结果：\n{hl}"),
        }),
    }];

    for (idx, m) in matches.iter().take(6).enumerate() {
        let caps_summary: String = m.groups.iter().enumerate()
            .filter_map(|(gi, v)| v.as_ref().map(|s| format!("${}={s}", gi + 1)))
            .collect::<Vec<_>>().join("  ");
        out.push(ResultItem {
            id: format!("re:hit:{idx}:{}", m.start),
            provider_id: PID.into(),
            title: format!("#{}: {}", idx + 1, m.text),
            subtitle: if caps_summary.is_empty() {
                format!("字符位置 {}..{}", m.start, m.end)
            } else {
                format!("{}..{} · {caps_summary}", m.start, m.end)
            },
            category: ResultCategory::Plugin,
            score: 9_500 - (idx as i64 * 20),
            icon: IconSpec::Monogram(format!("#{}", idx + 1)),
            badge: Some("MATCH".into()),
            copy_text: Some(m.text.clone()),
            actions: vec![ActionSpec {
                id: "copy_hit".into(),
                label: "复制此匹配结果".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText { text: m.text.clone(), paste_immediately: false },
            }],
            preview: None,
        });
    }
    out
}

fn build_overview_and_presets(filter: &str) -> Vec<ResultItem> {
    let low = filter.to_ascii_lowercase();
    PRESETS.iter().enumerate()
        .filter(|(_, p)| low.is_empty() || p.id.contains(&low) || p.label.to_lowercase().contains(&low))
        .map(|(i, p)| ResultItem {
            id: format!("re:preset:{}", p.id),
            provider_id: PID.into(),
            title: format!("re :{}  ·  {}", p.id, p.label),
            subtitle: format!("/{}/{}  | 示例: {}", p.pattern, p.flags, p.sample),
            category: ResultCategory::Plugin,
            score: 9_600 - (i as i64 * 15),
            icon: IconSpec::Monogram("RE".into()),
            badge: Some("PRESET".into()),
            copy_text: Some(p.pattern.into()),
            actions: vec![
                ActionSpec {
                    id: "try_preset".into(),
                    label: "加载并实时测试预设".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::FillQuery { query: format!("re :{} {}", p.id, p.sample) },
                },
                ActionSpec {
                    id: "copy_preset".into(),
                    label: "仅复制正则表达式".into(),
                    shortcut: Some("Ctrl+Enter".into()),
                    kind: ActionKind::CopyText { text: p.pattern.into(), paste_immediately: false },
                },
            ],
            preview: None,
        })
        .collect()
}

export_tomoyo_wasm_plugin!(OfficialRegexLab);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_slash_regex_presets_and_super_panel_action() {
        let mut ctx = QueryContext::new("re /(\\w+)@([\\w.]+)/g alice@example.com", 1, 10);
        ctx.keyword = Some("re".into());
        ctx.args = "/(\\w+)@([\\w.]+)/g alice@example.com".into();
        let hits = OfficialRegexLab::query(&ctx);
        assert!(hits[0].title.contains("命中 1 处"));
        assert_eq!(hits[1].copy_text.as_deref(), Some("alice@example.com"));

        ctx.args = ":ipv4 listen=10.0.0.8".into();
        let p_hits = OfficialRegexLab::query(&ctx);
        assert!(p_hits[0].title.contains("10.0.0.8"));

        let resp = OfficialRegexLab::execute(&ExecuteRequest::new(
            "test_regex",
            "sel",
            serde_json::json!({ "text": "v3.4.0" }),
        ));
        assert_eq!(resp.fill_query.as_deref(), Some("re /(\\S+)/g v3.4.0"));
        assert!(!resp.hide_window);
    }
}
