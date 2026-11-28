//! Protocol v3 `tomoyo/call` RPC backend for the ephemeral `dev.tomoyo.ai` HTML workbench.

use crate::client::{invoke_llm, AiConfig};
use crate::history::{history_path, load_history_from, push_history_to};
use crate::mcp_client::{
    call_server_tool, load_mcp_servers, load_or_refresh_cached_tools, mcp_toml_path, sync_root_dir,
};
use crate::mcp_semantic::parse_semantic_dirs;
use crate::prompts::AiMode;
use crate::semantic::search_local_knowledge;
use serde_json::{json, Value};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tomoyo_sdk::{
    ActionKind, ActionSpec, ExecuteResponse, IconSpec, OpenWebViewRequest, PluginCallRequest,
    ResultCategory, ResultItem,
};

pub fn open_workbench_response(initial_query: String) -> ExecuteResponse {
    ExecuteResponse {
        hide_window: true,
        open_webview: Some(OpenWebViewRequest {
            entry: None,
            title: Some("Tomoyo AI & MCP 工作台".into()),
            initial_query,
            width: Some(900),
            height: Some(640),
        }),
        ..Default::default()
    }
}

pub fn workbench_entry_item(initial_query: &str, title: &str, subtitle: &str, score: i64) -> ResultItem {
    ResultItem {
        id: format!("ai:ui:{initial_query}"),
        provider_id: "dev.tomoyo.ai".into(),
        title: title.into(),
        subtitle: subtitle.into(),
        category: ResultCategory::Plugin,
        score,
        icon: IconSpec::Monogram("UI".into()),
        badge: Some("WORKBENCH".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "open_workbench".into(),
            label: "打开 AI & MCP 工作台".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: "dev.tomoyo.ai".into(),
                action_id: "open_workbench".into(),
                payload: json!({ "initial_query": initial_query }),
            },
        }],
        preview: None,
    }
}

fn json_err(msg: impl Into<String>) -> String {
    msg.into()
}

pub fn handle_rpc(req: &PluginCallRequest) -> Result<Value, String> {
    let cfg = AiConfig::from_maps(&req.plugin_settings, &req.plugin_secrets);
    let sem_dirs = parse_semantic_dirs(&req.plugin_settings);

    match req.method.as_str() {
        "get_init" => {
            let history = load_history_from(&history_path());
            let servers = load_mcp_servers();
            Ok(json!({
                "backend": cfg.backend_hint(),
                "history": history,
                "servers": servers,
                "semantic_dirs": sem_dirs,
                "mcp_path": mcp_toml_path().to_string_lossy(),
            }))
        }
        "ask" => {
            let text = req.params.get("text").and_then(Value::as_str).unwrap_or("").trim();
            if text.is_empty() {
                return Err(json_err("请输入提问或待审校内容"));
            }
            let mode_str = req.params.get("mode").and_then(Value::as_str).unwrap_or("ask");
            let mode = AiMode::from_action_or_str(mode_str);
            let reply = invoke_llm(&cfg, mode, text)?;
            let hp = history_path();
            push_history_to(&hp, mode.as_str(), text, &reply.text, &reply.model);
            let history = load_history_from(&hp);
            Ok(json!({
                "text": reply.text,
                "model": reply.model,
                "elapsed_ms": reply.elapsed_ms,
                "history": history,
            }))
        }
        "semantic_search" => {
            let query = req.params.get("query").and_then(Value::as_str).unwrap_or("").trim();
            let limit = req.params.get("limit").and_then(Value::as_u64).unwrap_or(10).clamp(1, 24) as usize;
            let hits: Vec<Value> = if query.is_empty() {
                Vec::new()
            } else {
                search_local_knowledge(query, &sem_dirs, limit)
                    .into_iter()
                    .map(|h| json!({
                        "path": h.path,
                        "title": h.title,
                        "preview": h.preview,
                        "score": h.score,
                    }))
                    .collect()
            };
            Ok(json!({ "hits": hits }))
        }
        "mcp_list" => {
            let force_refresh = req.params.get("force_refresh").and_then(Value::as_bool).unwrap_or(false);
            let servers = load_mcp_servers();
            let (tools, errors) = load_or_refresh_cached_tools(&servers, force_refresh);
            Ok(json!({
                "servers": servers,
                "tools": tools,
                "errors": errors,
            }))
        }
        "mcp_call" => {
            let server_name = req.params.get("server").and_then(Value::as_str).unwrap_or("").trim();
            let tool_name = req.params.get("tool").and_then(Value::as_str).unwrap_or("").trim();
            if server_name.is_empty() || tool_name.is_empty() {
                return Err(json_err("请先指定 MCP 服务与工具名"));
            }
            let servers = load_mcp_servers();
            let srv = servers
                .iter()
                .find(|s| s.enabled && s.name.eq_ignore_ascii_case(server_name))
                .ok_or_else(|| json_err(format!("未找到已启用的 MCP 服务 `{server_name}`")))?;
            let args = req.params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let started = Instant::now();
            let output = call_server_tool(srv, tool_name, args)?;
            Ok(json!({
                "output": output,
                "elapsed_ms": started.elapsed().as_millis() as u64,
            }))
        }
        "save_note" => {
            let body = req.params.get("body").and_then(Value::as_str).unwrap_or("").trim();
            if body.is_empty() {
                return Err(json_err("空内容无法保存为备忘"));
            }
            let raw_title = req.params.get("title").and_then(Value::as_str).unwrap_or("").trim();
            let title = if raw_title.is_empty() {
                body.lines().find(|l| !l.trim().is_empty()).unwrap_or("AI 工作台摘录").trim().chars().take(40).collect()
            } else {
                raw_title.chars().take(64).collect::<String>()
            };
            let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let file_name = format!("ai_{secs}.md");
            let notes_dir = sync_root_dir().join("notes");
            std::fs::create_dir_all(&notes_dir).map_err(|e| json_err(format!("创建 notes 目录失败: {e}")))?;
            let path = notes_dir.join(&file_name);
            let esc = title.replace('"', "\\\"");
            let md = format!("---\ntitle: \"{esc}\"\ntags:\n  - \"#ai\"\nsource: \"ai_workbench\"\n---\n\n{body}\n");
            std::fs::write(&path, md).map_err(|e| json_err(format!("写入备忘失败: {e}")))?;
            Ok(json!({
                "file_name": file_name,
                "path": path.to_string_lossy(),
            }))
        }
        other => Err(json_err(format!("未知 AI 工作台方法：{other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn rpc_get_init_and_semantic_search_work() {
        let req = PluginCallRequest {
            method: "get_init".into(),
            params: json!({}),
            plugin_settings: BTreeMap::new(),
            plugin_secrets: BTreeMap::new(),
        };
        let init = handle_rpc(&req).unwrap();
        assert!(init["backend"].is_string());
        assert!(init["servers"].is_array());
        assert!(init["history"].is_array());

        let sem_req = PluginCallRequest {
            method: "semantic_search".into(),
            params: json!({ "query": "" }),
            plugin_settings: BTreeMap::new(),
            plugin_secrets: BTreeMap::new(),
        };
        let sem = handle_rpc(&sem_req).unwrap();
        assert_eq!(sem["hits"], json!([]));
    }

    #[test]
    fn rpc_rejects_unknown_method() {
        let req = PluginCallRequest {
            method: "nope".into(),
            params: json!({}),
            plugin_settings: BTreeMap::new(),
            plugin_secrets: BTreeMap::new(),
        };
        assert!(handle_rpc(&req).is_err());
    }
}
