//! 5-field Cron expression human-readable Chinese/English explainer for WASM DevTools.

use tomoyo_wasm_sdk::{
    ActionKind, ActionSpec, IconSpec, PreviewSpec, ResultCategory, ResultItem,
};

const PID: &str = "dev.tomoyo.devtools";

pub fn explain_cron_item(raw: &str) -> Option<ResultItem> {
    let trimmed = raw.trim();
    let normalized = match trimmed.to_ascii_lowercase().as_str() {
        "@yearly" | "@annually" => "0 0 1 1 *",
        "@monthly" => "0 0 1 * *",
        "@weekly" => "0 0 * * 0",
        "@daily" | "@midnight" => "0 0 * * *",
        "@hourly" => "0 * * * *",
        _ => trimmed,
    };
    let fields: Vec<&str> = normalized.split_whitespace().collect();
    if fields.len() != 5 {
        return None;
    }
    if !fields.iter().all(|f| is_valid_cron_token(f)) {
        return None;
    }

    let min_zh = describe_field(fields[0], "分", 0..=59, None);
    let hour_zh = describe_field(fields[1], "点", 0..=23, None);
    let dom_zh = describe_field(fields[2], "日", 1..=31, None);
    let mon_zh = describe_field(fields[3], "月", 1..=12, None);
    let dow_zh = describe_field(fields[4], "周", 0..=7, Some(weekday_name));

    let summary = build_human_summary(fields[0], fields[1], fields[2], fields[3], fields[4], &min_zh, &hour_zh, &dom_zh, &mon_zh, &dow_zh);
    Some(ResultItem {
        id: format!("dev:cron:{normalized}"),
        provider_id: PID.into(),
        title: format!("Cron 释义：{summary}"),
        subtitle: format!("表达式 `{normalized}` · Enter 复制表达式与说明"),
        category: ResultCategory::Plugin,
        score: 9_940,
        icon: IconSpec::Monogram("CRN".into()),
        badge: Some("CRON".into()),
        copy_text: Some(format!("{normalized}  # {summary}")),
        actions: vec![
            ActionSpec {
                id: "copy_cron_note".into(),
                label: "复制 Cron 表达式与中文说明".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText { text: format!("{normalized}  # {summary}"), paste_immediately: false },
            },
            ActionSpec {
                id: "copy_cron".into(),
                label: "仅复制规范化 5 段 Cron".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText { text: normalized.into(), paste_immediately: false },
            },
        ],
        preview: Some(PreviewSpec::RichCard {
            title: "Cron 定时规则解析".into(),
            subtitle: summary,
            badges: vec!["5-Field Cron".into(), "WASM".into()],
            rows: vec![
                ("表达式".into(), normalized.into()),
                ("分钟 (0-59)".into(), format!("{} → {min_zh}", fields[0])),
                ("小时 (0-23)".into(), format!("{} → {hour_zh}", fields[1])),
                ("日期 (1-31)".into(), format!("{} → {dom_zh}", fields[2])),
                ("月份 (1-12)".into(), format!("{} → {mon_zh}", fields[3])),
                ("星期 (0-7)".into(), format!("{} → {dow_zh}", fields[4])),
            ],
            body: "支持标准 5 段 Unix Cron 与 @hourly / @daily / @weekly / @monthly 快捷别名。".into(),
        }),
    })
}

fn is_valid_cron_token(tok: &str) -> bool {
    !tok.is_empty()
        && tok
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'*' | b'/' | b'-' | b','))
}

#[allow(clippy::too_many_arguments)]
fn build_human_summary(
    m: &str, h: &str, dom: &str, mon: &str, dow: &str,
    m_zh: &str, h_zh: &str, dom_zh: &str, mon_zh: &str, dow_zh: &str,
) -> String {
    let mut parts = Vec::new();
    if mon != "*" {
        parts.push(mon_zh.to_string());
    }
    if dom != "*" {
        parts.push(dom_zh.to_string());
    }
    if dow != "*" {
        parts.push(dow_zh.to_string());
    }
    if parts.is_empty() {
        if h == "*" && m.starts_with("*/") {
            return format!("每隔 {} 分钟执行一次", &m[2..]);
        }
        if h == "*" && m == "*" {
            return "每分钟执行一次".into();
        }
        if h == "*" {
            return format!("每小时的 {m_zh} 执行");
        }
        parts.push("每天".into());
    }
    if let (Ok(hv), Ok(mv)) = (h.parse::<u32>(), m.parse::<u32>()) {
        parts.push(format!("{hv:02}:{mv:02}"));
    } else {
        if h != "*" { parts.push(h_zh.to_string()); }
        if m != "*" { parts.push(m_zh.to_string()); }
    }
    format!("{} 执行", parts.join(" "))
}

fn describe_field(
    tok: &str,
    unit: &str,
    _range: std::ops::RangeInclusive<u32>,
    mapper: Option<fn(u32) -> &'static str>,
) -> String {
    if tok == "*" {
        return format!("每{unit}");
    }
    if let Some(step) = tok.strip_prefix("*/") {
        return format!("每隔 {step} {unit}");
    }
    let fmt_atom = |s: &str| -> String {
        if let Some((a, b)) = s.split_once('-') {
            let la = mapper.and_then(|f| a.parse().ok().map(f)).map(String::from).unwrap_or_else(|| format!("{a}{unit}"));
            let lb = mapper.and_then(|f| b.parse().ok().map(f)).map(String::from).unwrap_or_else(|| format!("{b}{unit}"));
            return format!("{la}至{lb}");
        }
        if let Some((base, step)) = s.split_once('/') {
            return format!("从 {base}{unit} 起每 {step} {unit}");
        }
        if let Some(f) = mapper
            && let Ok(n) = s.parse::<u32>()
        {
            return f(n).into();
        }
        format!("{s}{unit}")
    };
    tok.split(',').map(fmt_atom).collect::<Vec<_>>().join("、")
}

fn weekday_name(n: u32) -> &'static str {
    match n {
        0 | 7 => "周日",
        1 => "周一",
        2 => "周二",
        3 => "周三",
        4 => "周四",
        5 => "周五",
        6 => "周六",
        _ => "星期?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_common_cron_expressions() {
        let weekday_9 = explain_cron_item("0 9 * * 1-5").unwrap();
        assert!(weekday_9.title.contains("周一至周五"));
        assert!(weekday_9.title.contains("09:00"));

        let every_15 = explain_cron_item("*/15 * * * *").unwrap();
        assert!(every_15.title.contains("每隔 15 分钟"));

        let alias = explain_cron_item("@daily").unwrap();
        assert!(alias.title.contains("00:00"));
        assert!(explain_cron_item("not a cron").is_none());
    }
}
