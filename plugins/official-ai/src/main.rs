//! Official Tomoyo STDIO Plugin: Local-First AI Assistant (`dev.tomoyo.ai`).
//!
//! Runs strictly on-demand via JSON-RPC (`0 MB` idle footprint), connecting to local Ollama
//! or any user-configured OpenAI-compatible endpoint via OS-encrypted `[[settings]]` secrets.

mod client;
mod history;
mod http_curl;
mod mcp_client;
mod mcp_semantic;
mod prompts;
mod semantic;

use client::{invoke_llm, AiConfig};
use history::{history_path, load_history_from, push_history_to, AiHistoryEntry};
use mcp_semantic::{build_semantic_items, execute_mcp_action, parse_semantic_dirs, query_mcp};
use prompts::{extract_first_code_or_line, parse_ai_input, AiMode};
use serde_json::{json, Value};
use tomoyo_sdk::{
    ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse, IconSpec, PluginMeta, PreviewSpec,
    QueryContext, ResultCategory, ResultItem, TomoyoPlugin,
};

const PID: &str = "dev.tomoyo.ai";

struct AiPlugin;

fn clip_preview(s: &str, max_chars: usize) -> String {
    let clean = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = clean.chars().take(max_chars).collect();
    if clean.chars().count() > max_chars {
        out.push('…');
    }
    out
}

fn history_item(entry: &AiHistoryEntry, score: i64, idx: usize) -> ResultItem {
    let mode = AiMode::from_action_or_str(&entry.mode);
    let first_line = extract_first_code_or_line(&entry.reply);
    let mut actions = vec![
        ActionSpec {
            id: "copy".into(),
            label: "复制 AI 回复".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::CopyText {
                text: entry.reply.clone(),
                paste_immediately: false,
            },
        },
        ActionSpec {
            id: "paste".into(),
            label: "直接贴回回复".into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::CopyText {
                text: entry.reply.clone(),
                paste_immediately: true,
            },
        },
    ];
    if mode == AiMode::Regex && !first_line.is_empty() {
        actions.push(ActionSpec {
            id: "test_regex".into(),
            label: "填入 `re` 正则实验室测试".into(),
            shortcut: Some("Alt+Enter".into()),
            kind: ActionKind::FillQuery {
                query: format!("re {first_line}"),
            },
        });
    } else if mode == AiMode::Filter && !first_line.is_empty() {
        actions.push(ActionSpec {
            id: "run_filter".into(),
            label: "填入 `f` 文件检索".into(),
            shortcut: Some("Alt+Enter".into()),
            kind: ActionKind::FillQuery {
                query: format!("f {first_line}"),
            },
        });
    }

    ResultItem {
        id: format!("ai:hist:{idx}:{}", entry.id),
        provider_id: PID.into(),
        title: format!("[{}] {}", mode.badge(), clip_preview(&entry.query, 44)),
        subtitle: format!(
            "{} · {} (Enter 复制 · Shift+Enter 直贴)",
            entry.model,
            clip_preview(&entry.reply, 64)
        ),
        category: ResultCategory::Plugin,
        score,
        icon: IconSpec::Monogram(mode.badge().into()),
        badge: Some(entry.model.clone()),
        copy_text: Some(entry.reply.clone()),
        actions,
        preview: Some(PreviewSpec::RichCard {
            title: format!("{} · {}", mode.title_zh(), entry.model),
            subtitle: clip_preview(&entry.query, 72),
            badges: vec![mode.badge().into(), entry.model.clone()],
            rows: vec![],
            body: entry.reply.clone(),
        }),
    }
}

fn quick_mode_items(backend: &str) -> Vec<ResultItem> {
    let presets = [
        ("ai:m:sum", "SUM", "ai sum ", "AI 提炼要点 (`ai sum <文本>` 或省略直接读剪贴板)", 9_400),
        ("ai:m:fix", "FIX", "ai fix ", "AI 诊断报错 (`ai fix <堆栈>` 或省略直接诊断剪贴板)", 9_390),
        ("ai:m:polish", "EDIT", "ai polish ", "AI 润色改写 (`ai polish <文本>`)", 9_380),
        ("ai:m:regex", "REGEX", "ai regex ", "自然语言生成正则 (`ai regex 匹配邮箱` → 可联动 `re`)", 9_370),
        ("ai:m:cmd", "CMD", "ai cmd ", "自然语言生成终端命令 (`ai cmd 查找并清理 7 天前日志`)", 9_360),
        ("ai:m:filter", "FILTER", "ai filter ", "自然语言生成文件筛选 (`ai filter 本周超过 10MB 的 PDF`)", 9_350),
        ("ai:m:find", "KB", "ai find ", "离线 BM25+CJK 语义检索本地备忘录/短语/文档 (`ai find <问题>`)", 9_340),
        ("ai:m:mcp", "MCP", "mcp ", "按需调用外部 STDIO MCP Server 工具 (`mcp :config` / `mcp :refresh`)", 9_330),
    ];
    presets
        .into_iter()
        .map(|(id, badge, fill, title, score)| ResultItem {
            id: id.into(),
            provider_id: PID.into(),
            title: title.into(),
            subtitle: format!("当前后端：{backend} · Enter 填入前缀后输入需求"),
            category: ResultCategory::Plugin,
            score,
            icon: IconSpec::Monogram(badge.into()),
            badge: Some(badge.into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "fill".into(),
                label: "填入指令前缀".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::FillQuery { query: fill.into() },
            }],
            preview: None,
        })
        .collect()
}

impl TomoyoPlugin for AiPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("本地 AI 助手 (dev.tomoyo.ai)", env!("CARGO_PKG_VERSION"))
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        if ctx.keyword.as_deref().is_some_and(|k| k.eq_ignore_ascii_case("mcp")) {
            return query_mcp(ctx.args.trim());
        }
        let semantic_dirs = parse_semantic_dirs(&ctx.plugin_settings);
        let clip_text = ctx.clipboard.as_ref().map(|c| c.text.as_str());
        let trimmed = ctx.args.trim();
        let (head, tail) = trimmed.split_once(char::is_whitespace).unwrap_or((trimmed, ""));
        if matches!(head.to_ascii_lowercase().as_str(), "find" | "mem" | "kb" | "搜笔记") {
            let q = if tail.trim().is_empty() { clip_text.unwrap_or("") } else { tail.trim() };
            let hits = build_semantic_items(q, &semantic_dirs, 10_500, 10);
            if !hits.is_empty() || !q.trim().is_empty() {
                return if hits.is_empty() {
                    vec![ResultItem {
                        id: "ai:sem:none".into(), provider_id: PID.into(),
                        title: format!("本地知识库暂无匹配「{q}」的记录"),
                        subtitle: "已扫描 notes/、snippets.toml 及配置的 semantic_dirs".into(),
                        category: ResultCategory::Plugin, score: 9_800,
                        icon: IconSpec::Monogram("KB".into()), badge: Some("LOCAL KB".into()),
                        copy_text: None, actions: vec![], preview: None,
                    }]
                } else { hits };
            }
        }

        let cfg = AiConfig::from_maps(&ctx.plugin_settings, &ctx.plugin_secrets);
        let backend = cfg.backend_hint();
        let parsed = parse_ai_input(&ctx.args, clip_text);
        let history = load_history_from(&history_path());
        let mut out = Vec::new();

        if !parsed.text.is_empty() {
            let src_tag = if parsed.from_clipboard { " · 来自剪贴板" } else { "" };
            let snip = clip_preview(&parsed.text, 48);
            out.push(ResultItem {
                id: "ai:exec".into(),
                provider_id: PID.into(),
                title: format!("{}：「{snip}」", parsed.mode.title_zh()),
                subtitle: format!(
                    "使用 {backend}{src_tag} · Enter 执行并在预览窗查看回复 · Ctrl+Enter 直贴结果"
                ),
                category: ResultCategory::Plugin,
                score: 10_600,
                icon: IconSpec::Monogram(parsed.mode.badge().into()),
                badge: Some(parsed.mode.badge().into()),
                copy_text: Some(parsed.text.clone()),
                actions: vec![
                    ActionSpec {
                        id: "run_ai".into(),
                        label: "调用 AI 生成并复制结果".into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::PluginExecute {
                            plugin_id: PID.into(),
                            action_id: "run_ai".into(),
                            payload: json!({
                                "mode": parsed.mode.as_str(),
                                "text": parsed.text,
                                "paste": false,
                            }),
                        },
                    },
                    ActionSpec {
                        id: "run_ai_paste".into(),
                        label: "调用 AI 并直接贴回结果".into(),
                        shortcut: Some("Ctrl+Enter".into()),
                        kind: ActionKind::PluginExecute {
                            plugin_id: PID.into(),
                            action_id: "run_ai_paste".into(),
                            payload: json!({
                                "mode": parsed.mode.as_str(),
                                "text": parsed.text,
                                "paste": true,
                            }),
                        },
                    },
                ],
                preview: Some(PreviewSpec::Text {
                    title: format!("{} ({backend})", parsed.mode.title_zh()),
                    body: parsed.text.clone(),
                }),
            });

            let low = parsed.text.to_lowercase();
            for (i, h) in history
                .iter()
                .filter(|h| {
                    h.query.to_lowercase().contains(&low) || h.reply.to_lowercase().contains(&low)
                })
                .take(4)
                .enumerate()
            {
                out.push(history_item(h, 9_500 - i as i64, i));
            }
            out.extend(build_semantic_items(&parsed.text, &semantic_dirs, 9_100, 3));
            return out;
        }

        out.extend(quick_mode_items(&backend));
        for (i, h) in history.iter().take(6).enumerate() {
            out.push(history_item(h, 9_200 - i as i64, i));
        }
        out
    }

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        if let Some(mcp_resp) = execute_mcp_action(req) {
            return mcp_resp;
        }
        let mode = match req.action_id.as_str() {
            "ai_summarize" => AiMode::Summarize,
            "ai_diagnose" => AiMode::Diagnose,
            "ai_polish" => AiMode::Polish,
            _ => AiMode::from_action_or_str(
                req.payload.get("mode").and_then(Value::as_str).unwrap_or("ask"),
            ),
        };
        let text = req.payload.get("text").and_then(Value::as_str).unwrap_or("");
        let from_super_panel = req
            .payload
            .get("source")
            .and_then(Value::as_str)
            .is_some_and(|s| s == "super_panel");
        let cfg = AiConfig::from_maps(&req.plugin_settings, &req.plugin_secrets);

        match invoke_llm(&cfg, mode, text) {
            Ok(reply) => {
                push_history_to(&history_path(), mode.as_str(), text, &reply.text, &reply.model);
                let toast_snip = clip_preview(&reply.text, 56);
                let toast = Some(format!(
                    "[{} · {}ms] {toast_snip} (已复制)",
                    reply.model, reply.elapsed_ms
                ));
                if from_super_panel || req.action_id == "run_ai_paste" {
                    ExecuteResponse {
                        hide_window: true,
                        refresh_query: false,
                        toast,
                        copy_text: Some(reply.text),
                        ..Default::default()
                    }
                } else {
                    ExecuteResponse {
                        hide_window: false,
                        refresh_query: true,
                        toast,
                        copy_text: Some(reply.text),
                        ..Default::default()
                    }
                }
            }
            Err(err) => ExecuteResponse {
                hide_window: false,
                refresh_query: false,
                toast: Some(format!("AI 调用失败：{err}")),
                ..Default::default()
            },
        }
    }
}

fn main() {
    if std::env::args().any(|a| a == "--mock-mcp-server") {
        mcp_client::run_mock_mcp_server();
        return;
    }
    if std::env::args().any(|a| a == "--self-test") {
        let mut ctx = QueryContext::new("ai sum hello world", 1, 10);
        ctx.keyword = Some("ai".into());
        ctx.args = "sum hello world".into();
        let hits = AiPlugin.query(&ctx);
        assert!(hits.iter().any(|r| r.id == "ai:exec"));
        let empty = AiPlugin.query(&QueryContext::new("ai", 2, 10).with_keyword("ai", ""));
        assert!(empty.len() >= 8);
        assert!(!semantic::tokenize("Wayland 窗口").is_empty());
        let exe = std::env::current_exe().unwrap();
        let srv = mcp_client::McpServerConfig {
            name: "self".into(), command: exe.to_string_lossy().into_owned(),
            args: vec!["--mock-mcp-server".into()], env: std::collections::BTreeMap::new(), enabled: true,
        };
        assert_eq!(mcp_client::call_server_tool(&srv, "echo_upper", json!({ "text": "ok" })).unwrap(), "OK");
        println!("[OK] tomoyo-plugin-ai v{} self-test passed (LLM + BM25/CJK Semantic + STDIO MCP handshake)", env!("CARGO_PKG_VERSION"));
        return;
    }
    tomoyo_sdk::run(AiPlugin);
}
