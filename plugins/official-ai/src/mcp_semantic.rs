//! MCP query/execute handlers & offline BM25+CJK semantic search item builders for `dev.tomoyo.ai`.

use crate::mcp_client::{
    call_server_tool, load_mcp_servers, load_or_refresh_cached_tools, mcp_toml_path,
};
use crate::semantic::search_local_knowledge;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tomoyo_sdk::{
    ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse, IconSpec, PreviewSpec, ResultCategory,
    ResultItem,
};

const PID: &str = "dev.tomoyo.ai";

pub fn parse_semantic_dirs(settings: &BTreeMap<String, Value>) -> Vec<String> {
    settings
        .get("semantic_dirs")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn build_semantic_items(
    query: &str,
    extra_dirs: &[String],
    base_score: i64,
    limit: usize,
) -> Vec<ResultItem> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    search_local_knowledge(query, extra_dirs, limit)
        .into_iter()
        .enumerate()
        .map(|(i, hit)| {
            let short_prev: String = hit.preview.chars().take(88).collect();
            ResultItem {
                id: format!("ai:sem:{i}:{}", hit.title),
                provider_id: PID.into(),
                title: hit.title.clone(),
                subtitle: format!("[BM25 {:.2}] {short_prev}", hit.score),
                category: ResultCategory::Plugin,
                score: base_score - i as i64 * 15,
                icon: IconSpec::Monogram("KB".into()),
                badge: Some("LOCAL KB".into()),
                copy_text: Some(hit.preview.clone()),
                actions: vec![
                    ActionSpec {
                        id: "open_doc".into(),
                        label: "打开本地文档".into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::LaunchPath {
                            path: hit.path,
                            args: None,
                            working_dir: None,
                        },
                    },
                    ActionSpec {
                        id: "copy_snip".into(),
                        label: "复制命中段落".into(),
                        shortcut: Some("Ctrl+Enter".into()),
                        kind: ActionKind::CopyText {
                            text: hit.preview,
                            paste_immediately: false,
                        },
                    },
                ],
                preview: None,
            }
        })
        .collect()
}

pub fn query_mcp(args: &str) -> Vec<ResultItem> {
    let servers = load_mcp_servers();
    if args.eq_ignore_ascii_case(":refresh") {
        return vec![ResultItem {
            id: "mcp:refresh".into(),
            provider_id: PID.into(),
            title: "立即重新探测全部已启用 MCP 服务工具".into(),
            subtitle: format!(
                "已配置 {} 个服务 · 按需拉起握手并刷新 5 分钟缓存",
                servers.len()
            ),
            category: ResultCategory::Plugin,
            score: 10_600,
            icon: IconSpec::Monogram("MCP".into()),
            badge: Some("REFRESH".into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "mcp_refresh".into(),
                label: "刷新 MCP 工具列表".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PID.into(),
                    action_id: "mcp_refresh".into(),
                    payload: json!({}),
                },
            }],
            preview: None,
        }];
    }

    let cfg_path = mcp_toml_path().to_string_lossy().into_owned();
    if args.eq_ignore_ascii_case(":config") || servers.is_empty() {
        return vec![ResultItem {
            id: "mcp:config".into(),
            provider_id: PID.into(),
            title: "配置或打开 MCP 服务清单 (`mcp.toml` / `mcp.json`)".into(),
            subtitle: format!(
                "当前生效服务 {} 个 · 也可用 `tomoyo-cli mcp add` 命令行录入",
                servers.len()
            ),
            category: ResultCategory::Plugin,
            score: 10_550,
            icon: IconSpec::Monogram("MCP".into()),
            badge: Some("CONFIG".into()),
            copy_text: Some(cfg_path.clone()),
            actions: vec![ActionSpec {
                id: "open_mcp".into(),
                label: "编辑 mcp.toml".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::LaunchPath {
                    path: cfg_path,
                    args: None,
                    working_dir: None,
                },
            }],
            preview: None,
        }];
    }

    let (tools, _) = load_or_refresh_cached_tools(&servers, false);
    let (target, arg_str) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let low = target.to_ascii_lowercase();
    let parsed_arg = serde_json::from_str::<Value>(arg_str.trim())
        .unwrap_or_else(|_| json!({ "text": arg_str.trim() }));
    tools
        .into_iter()
        .filter(|t| {
            low.is_empty()
                || format!("{}:{} {}", t.server, t.name, t.description)
                    .to_ascii_lowercase()
                    .contains(&low)
        })
        .enumerate()
        .take(12)
        .map(|(i, t)| ResultItem {
            id: format!("mcp:tool:{}:{}", t.server, t.name),
            provider_id: PID.into(),
            title: format!(
                "mcp {}:{}  ·  {}",
                t.server,
                t.name,
                if arg_str.trim().is_empty() { "(无附加参数)" } else { arg_str.trim() }
            ),
            subtitle: t.description.clone(),
            category: ResultCategory::Plugin,
            score: 10_400 - i as i64 * 10,
            icon: IconSpec::Monogram("MCP".into()),
            badge: Some(t.server.clone()),
            copy_text: Some(format!("mcp {}:{}", t.server, t.name)),
            actions: vec![ActionSpec {
                id: "mcp_call".into(),
                label: "执行 MCP 工具并复制结果".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PID.into(),
                    action_id: "mcp_call".into(),
                    payload: json!({
                        "server": t.server,
                        "tool": t.name,
                        "args": parsed_arg
                    }),
                },
            }],
            preview: t.input_schema.map(|s| PreviewSpec::Text {
                title: format!("MCP Schema: {}", t.name),
                body: serde_json::to_string_pretty(&s).unwrap_or_default(),
            }),
        })
        .collect()
}

pub fn execute_mcp_action(req: &ExecuteRequest) -> Option<ExecuteResponse> {
    if req.action_id == "mcp_refresh" {
        let servers = load_mcp_servers();
        let (tools, errs) = load_or_refresh_cached_tools(&servers, true);
        let msg = if errs.is_empty() {
            format!(
                "已刷新 {} 个 MCP 服务，共发现 {} 个工具",
                servers.iter().filter(|s| s.enabled).count(),
                tools.len()
            )
        } else {
            format!("MCP 发现 {} 个工具（异常：{}）", tools.len(), errs.join("; "))
        };
        return Some(ExecuteResponse {
            hide_window: false,
            toast: Some(msg),
            ..Default::default()
        });
    }
    if req.action_id == "mcp_call" {
        let srv_name = req.payload.get("server").and_then(Value::as_str).unwrap_or("");
        let tool_name = req.payload.get("tool").and_then(Value::as_str).unwrap_or("");
        let args_val = req.payload.get("args").cloned().unwrap_or_else(|| json!({}));
        let servers = load_mcp_servers();
        let Some(srv) = servers.iter().find(|s| s.name.eq_ignore_ascii_case(srv_name)) else {
            return Some(ExecuteResponse {
                hide_window: false,
                toast: Some(format!("未找到 MCP 服务：{srv_name}")),
                ..Default::default()
            });
        };
        return Some(match call_server_tool(srv, tool_name, args_val) {
            Ok(text) => ExecuteResponse {
                hide_window: true,
                copy_text: Some(text),
                toast: Some(format!("MCP {srv_name}:{tool_name} 已完成并复制结果")),
                ..Default::default()
            },
            Err(e) => ExecuteResponse {
                hide_window: false,
                toast: Some(e),
                ..Default::default()
            },
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_semantic_dirs_and_handles_mcp_config_query() {
        let mut s = BTreeMap::new();
        s.insert("semantic_dirs".into(), json!("/tmp/a, /tmp/b;/tmp/c"));
        assert_eq!(parse_semantic_dirs(&s), vec!["/tmp/a", "/tmp/b", "/tmp/c"]);
        let items = query_mcp(":config");
        assert_eq!(items[0].id, "mcp:config");
    }
}
