//! On-demand STDIO JSON-RPC 2.0 MCP Client (`mcp.toml` + `mcp.json`) with cached tool catalog.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CACHE_TTL_SECS: u64 = 300;
const MCP_TIMEOUT_MS: u64 = 6_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpToolInfo {
    pub server: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct McpToolsCache {
    updated_at_unix: u64,
    tools: Vec<McpToolInfo>,
}

pub fn tomoyo_data_dir() -> PathBuf {
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Tomoyo");
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") && !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("tomoyo");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".local/share/tomoyo");
        }
    }
    PathBuf::from(".tomoyo-data")
}

pub fn sync_root_dir() -> PathBuf {
    let base = tomoyo_data_dir();
    if let Ok(raw) = std::fs::read_to_string(base.join("sync_dir.txt")) {
        let p = PathBuf::from(raw.trim());
        if p.is_dir() { return p; }
    }
    base
}

pub fn mcp_toml_path() -> PathBuf {
    sync_root_dir().join("mcp.toml")
}

pub fn ensure_mcp_toml_template(path: &Path) {
    if path.is_file() { return; }
    if let Some(p) = path.parent() { let _ = std::fs::create_dir_all(p); }
    let sample = "# Tomoyo MCP Servers Registry (Zero Idle RAM: spawned strictly on demand)\n\
                  # Compatible with both `mcp.toml` ([[servers]]) and `mcp.json` (mcpServers).\n\n\
                  # [[servers]]\n\
                  # name = \"filesystem\"\n\
                  # command = \"npx\"\n\
                  # args = [\"-y\", \"@modelcontextprotocol/server-filesystem\", \"/path/to/docs\"]\n\
                  # enabled = true\n";
    let _ = std::fs::write(path, sample);
}

pub fn load_mcp_servers() -> Vec<McpServerConfig> {
    let toml_p = mcp_toml_path();
    ensure_mcp_toml_template(&toml_p);
    let mut servers = std::fs::read_to_string(&toml_p).map(|s| parse_mcp_toml(&s)).unwrap_or_default();
    let mut seen: std::collections::HashSet<String> = servers.iter().map(|s| s.name.to_ascii_lowercase()).collect();
    for cand in [sync_root_dir().join("mcp.json"), tomoyo_data_dir().join("mcp.json")] {
        if let Ok(raw) = std::fs::read_to_string(&cand) {
            for s in parse_mcp_json(&raw) {
                if seen.insert(s.name.to_ascii_lowercase()) {
                    servers.push(s);
                }
            }
        }
    }
    servers
}

pub fn parse_mcp_toml(raw: &str) -> Vec<McpServerConfig> {
    let mut out = Vec::new();
    let mut cur: Option<McpServerConfig> = None;
    let mut in_env = false;
    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if line == "[[servers]]" {
            if let Some(s) = cur.take() && !s.name.is_empty() && !s.command.is_empty() { out.push(s); }
            cur = Some(McpServerConfig { name: String::new(), command: String::new(), args: vec![], env: BTreeMap::new(), enabled: true });
            in_env = false;
            continue;
        }
        if line == "[servers.env]" { in_env = true; continue; }
        if line.starts_with('[') { in_env = false; continue; }
        let Some((s, (k, v))) = cur.as_mut().zip(line.split_once('=')) else { continue; };
        let key = k.trim();
        let val = v.trim();
        if in_env {
            s.env.insert(key.into(), unquote_str(val));
            continue;
        }
        match key {
            "name" => s.name = unquote_str(val),
            "command" => s.command = unquote_str(val),
            "enabled" => s.enabled = !val.eq_ignore_ascii_case("false"),
            "args" => s.args = serde_json::from_str::<Vec<String>>(val).unwrap_or_default(),
            _ => {}
        }
    }
    if let Some(s) = cur && !s.name.is_empty() && !s.command.is_empty() { out.push(s); }
    out
}

pub fn parse_mcp_json(raw: &str) -> Vec<McpServerConfig> {
    let Ok(val) = serde_json::from_str::<Value>(raw) else { return Vec::new(); };
    let Some(obj) = val.get("mcpServers").and_then(Value::as_object) else { return Vec::new(); };
    obj.iter()
        .filter_map(|(name, cfg)| {
            let command = cfg.get("command")?.as_str()?.trim().to_string();
            if command.is_empty() { return None; }
            let args = cfg.get("args").and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let env = cfg.get("env").and_then(Value::as_object)
                .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
                .unwrap_or_default();
            let enabled = !cfg.get("disabled").and_then(Value::as_bool).unwrap_or(false);
            Some(McpServerConfig { name: name.clone(), command, args, env, enabled })
        })
        .collect()
}

fn unquote_str(s: &str) -> String {
    let t = s.trim();
    serde_json::from_str::<String>(t).unwrap_or_else(|_| t.trim_matches(['"', '\'']).to_string())
}

struct ChildKillGuard(Child);
impl Drop for ChildKillGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct McpSession {
    _guard: Option<ChildKillGuard>,
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<String>,
    next_id: u64,
}

impl McpSession {
    fn start(cfg: &McpServerConfig) -> Result<Self, String> {
        let mut cmd = Command::new(&cfg.command);
        cmd.args(&cfg.args)
            .envs(&cfg.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| format!("启动 MCP 服务 `{}` 失败: {e}", cfg.name))?;
        let stdin = child.stdin.take().ok_or("MCP stdin 不可用")?;
        let stdout = child.stdout.take().ok_or("MCP stdout 不可用")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if !line.trim().is_empty() && tx.send(line).is_err() { break; }
            }
        });
        let mut sess = Self { _guard: Some(ChildKillGuard(child)), writer: Box::new(stdin), rx, next_id: 1 };
        sess.handshake()?;
        Ok(sess)
    }

    fn handshake(&mut self) -> Result<(), String> {
        let _ = self.rpc("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "tomoyo-ai-mcp", "version": env!("CARGO_PKG_VERSION") }
        }))?;
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let _ = writeln!(self.writer, "{note}");
        let _ = self.writer.flush();
        Ok(())
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.writer, "{req}").map_err(|e| format!("MCP 写入失败: {e}"))?;
        self.writer.flush().map_err(|e| format!("MCP flush 失败: {e}"))?;

        let deadline = Duration::from_millis(MCP_TIMEOUT_MS);
        let started = std::time::Instant::now();
        while started.elapsed() < deadline {
            let rem = deadline.saturating_sub(started.elapsed());
            let Ok(line) = self.rx.recv_timeout(rem) else {
                return Err(format!("MCP `{method}` 响应超时"));
            };
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue; };
            if v.get("id").and_then(Value::as_u64) != Some(id) { continue; }
            if let Some(err) = v.get("error") {
                let msg = err.get("message").and_then(Value::as_str).unwrap_or("MCP error");
                return Err(msg.into());
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
        Err(format!("MCP `{method}` 超时"))
    }
}

pub fn list_server_tools(cfg: &McpServerConfig) -> Result<Vec<McpToolInfo>, String> {
    let mut sess = McpSession::start(cfg)?;
    list_tools_on(&mut sess, &cfg.name)
}

fn list_tools_on(sess: &mut McpSession, server_name: &str) -> Result<Vec<McpToolInfo>, String> {
    let res = sess.rpc("tools/list", json!({}))?;
    let tools = res
        .get("tools")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?.to_string();
                    Some(McpToolInfo {
                        server: server_name.into(),
                        name,
                        description: t.get("description").and_then(Value::as_str).unwrap_or("").into(),
                        input_schema: t.get("inputSchema").cloned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(tools)
}

pub fn call_server_tool(cfg: &McpServerConfig, tool: &str, args: Value) -> Result<String, String> {
    let mut sess = McpSession::start(cfg)?;
    call_tool_on(&mut sess, tool, args)
}

fn call_tool_on(sess: &mut McpSession, tool: &str, args: Value) -> Result<String, String> {
    let res = sess.rpc("tools/call", json!({ "name": tool, "arguments": args }))?;
    if let Some(arr) = res.get("content").and_then(Value::as_array) {
        let texts: Vec<&str> = arr
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect();
        if !texts.is_empty() { return Ok(texts.join("\n")); }
    }
    Ok(serde_json::to_string_pretty(&res).unwrap_or_default())
}

pub fn load_or_refresh_cached_tools(servers: &[McpServerConfig], force_refresh: bool) -> (Vec<McpToolInfo>, Vec<String>) {
    let cache_path = tomoyo_data_dir().join("cache").join("mcp_tools.json");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if !force_refresh
        && let Ok(raw) = std::fs::read_to_string(&cache_path)
        && let Ok(cached) = serde_json::from_str::<McpToolsCache>(&raw)
        && now.saturating_sub(cached.updated_at_unix) < CACHE_TTL_SECS
    {
        return (cached.tools, vec![]);
    }
    let mut tools = Vec::new();
    let mut errs = Vec::new();
    for srv in servers.iter().filter(|s| s.enabled) {
        match list_server_tools(srv) {
            Ok(mut list) => tools.append(&mut list),
            Err(e) => errs.push(format!("{}: {e}", srv.name)),
        }
    }
    if let Some(p) = cache_path.parent() { let _ = std::fs::create_dir_all(p); }
    let _ = std::fs::write(&cache_path, serde_json::to_string(&McpToolsCache { updated_at_unix: now, tools: tools.clone() }).unwrap_or_default());
    (tools, errs)
}

pub fn handle_mock_mcp_line(line: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(line.trim()).ok()?;
    let id = v.get("id").cloned()?;
    let method = v.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => json!({ "protocolVersion": "2024-11-05", "capabilities": { "tools": {} }, "serverInfo": { "name": "tomoyo-mock-mcp", "version": "1.0" } }),
        "tools/list" => json!({ "tools": [
            { "name": "echo_upper", "description": "Uppercase text via MCP tool", "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } } } },
            { "name": "word_count", "description": "Count characters & words", "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } } } }
        ]}),
        "tools/call" => {
            let name = v.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let txt = v.pointer("/params/arguments/text").and_then(Value::as_str).unwrap_or("");
            let out_txt = match name {
                "echo_upper" => txt.to_uppercase(),
                "word_count" => format!("chars={}, words={}", txt.chars().count(), txt.split_whitespace().count()),
                _ => format!("unknown tool {name}"),
            };
            json!({ "content": [{ "type": "text", "text": out_txt }] })
        }
        _ => json!({}),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
}

pub fn run_mock_mcp_server() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for line in stdin.lock().lines().map_while(Result::ok) {
        if let Some(reply) = handle_mock_mcp_line(&line) {
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
#[path = "mcp_client_tests.rs"]
mod tests;
