//! Settings Center visual CRUD for `mcp.toml` and on-demand STDIO `tools/list` connectivity probe.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tomoyo_core::{data_dir, sync_root_dir};

const PROBE_TIMEOUT_MS: u64 = 4_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerRow {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct McpDoc {
    #[serde(default)]
    servers: Vec<McpServerRow>,
}

pub fn mcp_file_path() -> PathBuf {
    sync_root_dir().join("mcp.toml")
}

pub fn load_mcp_servers_list() -> Vec<McpServerRow> {
    load_mcp_servers_at(&mcp_file_path())
}

fn load_mcp_servers_at(path: &Path) -> Vec<McpServerRow> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    toml::from_str::<McpDoc>(&raw)
        .map(|d| d.servers)
        .unwrap_or_default()
}

fn save_mcp_servers_at(path: &Path, rows: &[McpServerRow]) -> Result<Vec<McpServerRow>, String> {
    let mut clean: Vec<McpServerRow> = rows
        .iter()
        .filter_map(|r| {
            let name = r.name.trim().to_string();
            let command = r.command.trim().to_string();
            if name.is_empty() || command.is_empty() {
                return None;
            }
            let args = r
                .args
                .iter()
                .map(|a| a.trim())
                .filter(|a| !a.is_empty())
                .map(str::to_string)
                .collect();
            let env = r
                .env
                .iter()
                .filter(|(k, _)| !k.trim().is_empty())
                .map(|(k, v)| (k.trim().to_string(), v.clone()))
                .collect();
            Some(McpServerRow {
                name,
                command,
                args,
                env,
                enabled: r.enabled,
            })
        })
        .take(64)
        .collect();
    let mut seen = HashSet::new();
    clean.retain(|r| seen.insert(r.name.to_ascii_lowercase()));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let body = toml::to_string_pretty(&McpDoc {
        servers: clean.clone(),
    })
    .map_err(|e| format!("序列化 mcp.toml 失败: {e}"))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("写入失败: {e}"))?;
    if std::fs::rename(&tmp, path).is_err() {
        std::fs::copy(&tmp, path).map_err(|e| format!("覆盖失败: {e}"))?;
        let _ = std::fs::remove_file(&tmp);
    }
    let _ = std::fs::remove_file(data_dir().join("cache").join("mcp_tools.json"));
    Ok(clean)
}

pub fn save_mcp_servers_reply(rows: &[McpServerRow]) -> (String, bool) {
    match save_mcp_servers_at(&mcp_file_path(), rows) {
        Ok(clean) => (
            json!({
                "event": "mcp_servers_saved",
                "ok": true,
                "servers": clean,
                "message": format!("已保存 {} 个 MCP 服务并刷新工具缓存", clean.len()),
            })
            .to_string(),
            true,
        ),
        Err(e) => (
            json!({
                "event": "mcp_servers_saved",
                "ok": false,
                "message": e,
            })
            .to_string(),
            false,
        ),
    }
}

struct ChildKillGuard(Child);
impl Drop for ChildKillGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read_rpc_reply(
    rx: &mpsc::Receiver<String>,
    expected_id: u64,
    deadline: Instant,
    method: &str,
) -> Result<Value, String> {
    while Instant::now() < deadline {
        let rem = deadline.saturating_duration_since(Instant::now());
        let Ok(line) = rx.recv_timeout(rem) else {
            return Err(format!("MCP `{method}` 响应超时"));
        };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v.get("id").and_then(Value::as_u64) != Some(expected_id) {
            continue;
        }
        if let Some(err) = v.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("MCP error");
            return Err(msg.into());
        }
        return Ok(v.get("result").cloned().unwrap_or(Value::Null));
    }
    Err(format!("MCP `{method}` 超时"))
}

fn probe_tools_once(row: &McpServerRow) -> Result<Vec<Value>, String> {
    let cmd_s = row.command.trim();
    if cmd_s.is_empty() {
        return Err("MCP 启动命令不能为空".into());
    }
    let mut cmd = Command::new(cmd_s);
    cmd.args(&row.args)
        .envs(&row.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 `{cmd_s}` 失败: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("MCP stdin 不可用")?;
    let stdout = child.stdout.take().ok_or("MCP stdout 不可用")?;
    let _guard = ChildKillGuard(child);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if !line.trim().is_empty() && tx.send(line).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_millis(PROBE_TIMEOUT_MS);
    let init_req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "tomoyo-settings-probe", "version": env!("CARGO_PKG_VERSION") }
        }
    });
    writeln!(stdin, "{init_req}").map_err(|e| format!("写入失败: {e}"))?;
    let _ = stdin.flush();
    let _ = read_rpc_reply(&rx, 1, deadline, "initialize")?;

    let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let _ = writeln!(stdin, "{note}");
    let list_req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
    writeln!(stdin, "{list_req}").map_err(|e| format!("写入失败: {e}"))?;
    let _ = stdin.flush();

    let res = read_rpc_reply(&rx, 2, deadline, "tools/list")?;
    let tools = res
        .get("tools")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .take(32)
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?;
                    let desc = t.get("description").and_then(Value::as_str).unwrap_or("");
                    Some(json!({ "name": name, "description": desc }))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(tools)
}

pub fn probe_mcp_server_reply(row: &McpServerRow) -> String {
    let started = Instant::now();
    let name = row.name.trim();
    match probe_tools_once(row) {
        Ok(tools) => json!({
            "event": "mcp_probe_result",
            "ok": true,
            "name": name,
            "elapsed_ms": started.elapsed().as_millis() as u64,
            "tools": tools,
            "message": format!("握手成功 · 发现 {} 个工具", tools.len()),
        })
        .to_string(),
        Err(err) => json!({
            "event": "mcp_probe_result",
            "ok": false,
            "name": name,
            "elapsed_ms": started.elapsed().as_millis() as u64,
            "tools": [],
            "message": err,
        })
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_reload_mcp_toml_roundtrip() {
        let p = std::env::temp_dir().join(format!("tomoyo_mcp_test_{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let rows = vec![
            McpServerRow {
                name: "fs".into(),
                command: "npx".into(),
                args: vec!["-y".into(), "server-filesystem".into()],
                env: BTreeMap::from([("MODE".into(), "ro".into())]),
                enabled: true,
            },
            McpServerRow {
                name: "FS".into(),
                command: "dup".into(),
                args: vec![],
                env: BTreeMap::new(),
                enabled: false,
            },
        ];
        let saved = save_mcp_servers_at(&p, &rows).unwrap();
        assert_eq!(saved.len(), 1);
        let loaded = load_mcp_servers_at(&p);
        assert_eq!(loaded, saved);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn probe_nonexistent_command_returns_structured_error() {
        let row = McpServerRow {
            name: "missing".into(),
            command: "tomoyo_nonexistent_mcp_cmd_12345".into(),
            args: vec![],
            env: BTreeMap::new(),
            enabled: true,
        };
        let v: Value = serde_json::from_str(&probe_mcp_server_reply(&row)).unwrap();
        assert_eq!(v["event"], "mcp_probe_result");
        assert_eq!(v["ok"], false);
        assert_eq!(v["name"], "missing");
    }
}
