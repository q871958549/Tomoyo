//! Official Zero-Permission Pure-WASM Developer Swiss-Army Knife (`dev.tomoyo.devtools`).
//!
//! Runs inside Tomoyo's `wasmi` fuel-metered sandbox (16 MB cap, 0 ambient OS permissions).
//! Keywords: `dev`, `ts`, `b64`, `url`, `cron`, `uuid`.

mod codec;
mod cron;
mod time_uuid;

use codec::{build_base64_items, build_url_items, decode_jwt_items};
use cron::explain_cron_item;
use time_uuid::{build_timestamp_items, build_uuid_items};
use tomoyo_wasm_sdk::{
    export_tomoyo_wasm_plugin, host, ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse,
    IconSpec, PreviewSpec, QueryContext, ResultCategory, ResultItem, TomoyoWasmPlugin,
};

const PID: &str = "dev.tomoyo.devtools";

struct OfficialDevTools;

impl TomoyoWasmPlugin for OfficialDevTools {
    fn query(ctx: &QueryContext) -> Vec<ResultItem> {
        let mut kw = ctx.keyword.as_deref().unwrap_or("dev").to_ascii_lowercase();
        let mut args = ctx.args.trim();
        let now_ms = host::now_ms();
        let now_unix = if now_ms > 0 { now_ms / 1000 } else { host::now_unix() };

        if kw == "dev" && !args.is_empty() {
            let (first, rest) = match args.split_once(char::is_whitespace) {
                Some((a, b)) => (a, b.trim()),
                None => (args, ""),
            };
            let fl = first.to_ascii_lowercase();
            if matches!(fl.as_str(), "ts" | "uuid" | "b64" | "url" | "cron" | "jwt") {
                kw = fl;
                args = rest;
            }
        }

        match kw.as_str() {
            "ts" => {
                let mut items = build_timestamp_items(args, now_ms);
                if items.is_empty() {
                    items.push(hint_card("ts <时间戳或 YYYY-MM-DD HH:MM:SS>", "示例：ts 1700000000 · ts 2026-11-25 12:00:00 · ts now"));
                }
                return items;
            }
            "uuid" => {
                let counter = next_kv_counter();
                return build_uuid_items(now_ms, counter);
            }
            "b64" => {
                if args.is_empty() {
                    return vec![hint_card("b64 <文本或 Base64 密文>", "自动同时展示 Base64 编码与 UTF-8 解码结果")];
                }
                return build_base64_items(args);
            }
            "url" => {
                if args.is_empty() {
                    return vec![hint_card("url <网址或文本>", "支持 URL 百分号编解码与 ?k=v 查询参数 JSON 拆解")];
                }
                return build_url_items(args);
            }
            "cron" => {
                if let Some(item) = explain_cron_item(args) {
                    return vec![item];
                }
                return vec![hint_card("cron <5 段表达式>", "示例：cron 0 9 * * 1-5 · cron */15 * * * * · cron @daily")];
            }
            "jwt" => {
                if let Some(items) = decode_jwt_items(args, now_unix) {
                    return items;
                }
                return vec![hint_card("jwt <header.payload.sig>", "粘贴三段式 JWT 令牌，零网络离线解析并校验过期时间")];
            }
            _ => {}
        }

        if args.is_empty() {
            let mut out = build_timestamp_items("", now_ms);
            let counter = next_kv_counter();
            if let Some(first_uuid) = build_uuid_items(now_ms, counter).into_iter().next() {
                out.push(first_uuid);
            }
            out.push(overview_card());
            return out;
        }

        // Smart auto-detection under `dev <anything>`
        let mut items = Vec::new();
        if let Some(jwt) = decode_jwt_items(args, now_unix) {
            items.extend(jwt);
        }
        if let Some(cr) = explain_cron_item(args) {
            items.push(cr);
        }
        if looks_like_timestamp_or_date(args) {
            items.extend(build_timestamp_items(args, now_ms));
        }
        if args.contains('%') || args.contains('?') || args.starts_with("http://") || args.starts_with("https://") {
            items.extend(build_url_items(args));
        }
        items.extend(build_base64_items(args));
        items.sort_by_key(|a| std::cmp::Reverse(a.score));
        items.truncate(8);
        items
    }

    fn execute(req: &ExecuteRequest) -> ExecuteResponse {
        if req.action_id == "inspect_dev" {
            let text = req.payload.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
            if !text.is_empty() {
                return ExecuteResponse {
                    hide_window: false,
                    fill_query: Some(format!("dev {text}")),
                    ..Default::default()
                };
            }
        }
        ExecuteResponse::default()
    }
}

fn next_kv_counter() -> u64 {
    let cur = host::kv_get("uuid_seq").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let next = cur.wrapping_add(1);
    let _ = host::kv_set("uuid_seq", &next.to_string());
    next
}

fn looks_like_timestamp_or_date(s: &str) -> bool {
    if s.parse::<i64>().is_ok() && (9..=13).contains(&s.trim_start_matches('-').len()) {
        return true;
    }
    s.len() >= 10 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-')
}

fn hint_card(title: &str, subtitle: &str) -> ResultItem {
    ResultItem {
        id: format!("dev:hint:{title}"),
        provider_id: PID.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        category: ResultCategory::Plugin,
        score: 9_600,
        icon: IconSpec::Monogram("DEV".into()),
        badge: Some("WASM".into()),
        copy_text: None,
        actions: vec![],
        preview: None,
    }
}

fn overview_card() -> ResultItem {
    ResultItem {
        id: "dev:overview".into(),
        provider_id: PID.into(),
        title: "开发者工具箱 DevTools — 零权限纯 WASM 沙箱".into(),
        subtitle: "支持 ts / uuid / b64 / url / cron / dev <JWT|时间戳|密文|Cron> 自动识别".into(),
        category: ResultCategory::Plugin,
        score: 9_500,
        icon: IconSpec::Monogram("DEV".into()),
        badge: Some("WASM".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "try_cron".into(),
            label: "示例：解析工作日早 9 点 Cron".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::FillQuery { query: "cron 0 9 * * 1-5".into() },
        }],
        preview: Some(PreviewSpec::RichCard {
            title: "Tomoyo Official WASM DevTools".into(),
            subtitle: "16 MB 线性内存硬顶 · 零网络/零磁盘/零进程驻留".into(),
            badges: vec!["WASM ABI v2".into(), "0 Permissions".into(), "Offline".into()],
            rows: vec![
                ("ts <数值/日期>".into(), "Unix 秒/毫秒 ↔ UTC+8 / ISO-8601 互转".into()),
                ("uuid".into(), "生成 UUID v4、紧凑 UUID 与 128/256-bit 密钥".into()),
                ("b64 <内容>".into(), "Base64 双向编解码（兼容 Standard / URL-Safe）".into()),
                ("url <链接>".into(), "百分号编解码 + Query 参数 JSON 拆解".into()),
                ("cron <5段>".into(), "Unix Cron 与 @daily/@hourly 中文释义".into()),
                ("dev <任意>".into(), "自动识别 JWT 过期校验 / 时间戳 / Cron / URL / Base64".into()),
            ],
            body: "选中任意 Token/时间戳/Cron 文本呼出超级面板，点击「开发者解码/解析」亦可一键直达。".into(),
        }),
    }
}

export_tomoyo_wasm_plugin!(OfficialDevTools);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_smart_dev_queries_and_super_panel_action() {
        let mut ctx = QueryContext::new("dev 0 9 * * 1-5", 1, 10);
        ctx.keyword = Some("dev".into());
        ctx.args = "0 9 * * 1-5".into();
        let hits = OfficialDevTools::query(&ctx);
        assert!(hits.iter().any(|i| i.id.starts_with("dev:cron:")));

        ctx.args = "cron 0 9 * * 1-5".into();
        assert!(OfficialDevTools::query(&ctx)[0].title.contains("周一至周五"));
        ctx.args = "uuid".into();
        assert_eq!(OfficialDevTools::query(&ctx).len(), 4);

        let resp = OfficialDevTools::execute(&ExecuteRequest::new(
            "inspect_dev",
            "sel",
            serde_json::json!({ "text": "1700000000" }),
        ));
        assert_eq!(resp.fill_query.as_deref(), Some("dev 1700000000"));
        assert!(!resp.hide_window);
    }
}
