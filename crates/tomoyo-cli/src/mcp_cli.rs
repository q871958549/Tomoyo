//! Portable `mcp.toml` CRUD & live STDIO JSON-RPC 2.0 handshake inspector (`tomoyo-cli mcp`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tomoyo_core::sync_root_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpDoc {
    #[serde(default)]
    servers: Vec<McpServerRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpServerRow {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "def_true")]
    enabled: bool,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn def_true() -> bool { true }

pub fn cmd_mcp(args: &[String]) -> i32 {
    let path = sync_root_dir().join("mcp.toml");
    let Some(sub) = args.first().map(String::as_str) else {
        eprintln!(
            "Usage:\n  tomoyo-cli mcp list\n  tomoyo-cli mcp add <name> --cmd <bin> [--args \"a,b\"] [--env K=V] [--disabled]\n  tomoyo-cli mcp remove <name>\n  tomoyo-cli mcp inspect <name>\n  tomoyo-cli mcp call <name> <tool> [--json '{{\"k\":\"v\"}}']"
        );
        return 1;
    };
    match sub {
        "list" => {
            let doc = load_mcp_at(&path);
            if doc.servers.is_empty() {
                println!("No MCP servers configured at {}", path.display());
            } else {
                println!("MCP Servers ({} in {}):", doc.servers.len(), path.display());
                for s in &doc.servers {
                    println!(
                        "  - {:<14} | enabled={} | {} {}",
                        s.name,
                        s.enabled,
                        s.command,
                        s.args.join(" ")
                    );
                }
            }
            0
        }
        "add" => {
            let Some(name) = args.get(1).filter(|s| !s.starts_with('-')) else {
                eprintln!("Usage: tomoyo-cli mcp add <name> --cmd <bin> [--args \"a,b\"] [--env K=V] [--disabled]");
                return 1;
            };
            let Some(command) = crate::flag_val(&args[2..], "--cmd").map(str::trim).filter(|s| !s.is_empty()) else {
                eprintln!("Missing `--cmd <bin>`");
                return 1;
            };
            let arg_list: Vec<String> = crate::flag_val(&args[2..], "--args")
                .map(|r| r.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
                .unwrap_or_default();
            let mut env = BTreeMap::new();
            for win in args[2..].windows(2) {
                if win[0] == "--env"
                    && let Some((k, v)) = win[1].split_once('=')
                {
                    env.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
            let enabled = !args[2..].iter().any(|a| a == "--disabled");
            match upsert_mcp_at(
                &path,
                McpServerRow {
                    name: name.into(),
                    command: command.into(),
                    args: arg_list,
                    enabled,
                    env,
                },
            ) {
                Ok(()) => {
                    println!("[OK] Saved MCP server `{name}` -> {}", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("[ERROR] {e}");
                    1
                }
            }
        }
        "remove" | "rm" => {
            let Some(name) = args.get(1) else {
                eprintln!("Usage: tomoyo-cli mcp remove <name>");
                return 1;
            };
            match remove_mcp_at(&path, name) {
                Ok(true) => {
                    println!("[OK] Removed MCP server `{name}` from {}", path.display());
                    0
                }
                Ok(false) => {
                    eprintln!("MCP server `{name}` not found");
                    1
                }
                Err(e) => {
                    eprintln!("[ERROR] {e}");
                    1
                }
            }
        }
        "inspect" => {
            let Some(name) = args.get(1) else {
                eprintln!("Usage: tomoyo-cli mcp inspect <name>");
                return 1;
            };
            let doc = load_mcp_at(&path);
            let Some(srv) = doc.servers.iter().find(|s| s.name.eq_ignore_ascii_case(name)) else {
                eprintln!("MCP server `{name}` not found in {}", path.display());
                return 1;
            };
            match live_rpc(srv, "tools/list", json!({})) {
                Ok(res) => {
                    let tools = res.get("tools").and_then(Value::as_array).cloned().unwrap_or_default();
                    println!("MCP `{}` Tools ({}):", srv.name, tools.len());
                    for t in tools {
                        let tn = t.get("name").and_then(Value::as_str).unwrap_or("");
                        let desc = t.get("description").and_then(Value::as_str).unwrap_or("");
                        println!("  - {tn:<22} | {desc}");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("[ERROR] {e}");
                    1
                }
            }
        }
        "call" => {
            let (Some(name), Some(tool)) = (args.get(1), args.get(2)) else {
                eprintln!("Usage: tomoyo-cli mcp call <name> <tool> [--json '{{\"k\":\"v\"}}']");
                return 1;
            };
            let doc = load_mcp_at(&path);
            let Some(srv) = doc.servers.iter().find(|s| s.name.eq_ignore_ascii_case(name)) else {
                eprintln!("MCP server `{name}` not found");
                return 1;
            };
            let arg_val = crate::flag_val(&args[3..], "--json")
                .and_then(|r| serde_json::from_str::<Value>(r).ok())
                .unwrap_or_else(|| json!({}));
            match live_rpc(srv, "tools/call", json!({ "name": tool, "arguments": arg_val })) {
                Ok(res) => {
                    if let Some(arr) = res.get("content").and_then(Value::as_array) {
                        for item in arr {
                            if let Some(t) = item.get("text").and_then(Value::as_str) {
                                println!("{t}");
                            }
                        }
                    } else {
                        println!("{}", serde_json::to_string_pretty(&res).unwrap_or_default());
                    }
                    0
                }
                Err(e) => {
                    eprintln!("[ERROR] {e}");
                    1
                }
            }
        }
        other => {
            eprintln!("Unknown mcp subcommand `{other}`");
            1
        }
    }
}

fn load_mcp_at(path: &Path) -> McpDoc {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return McpDoc { servers: Vec::new() };
    };
    toml::from_str::<McpDoc>(&raw).unwrap_or(McpDoc { servers: Vec::new() })
}

fn save_mcp_at(path: &Path, doc: &McpDoc) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("create dir: {e}"))?;
    }
    let body = toml::to_string_pretty(doc).map_err(|e| format!("serialize mcp.toml: {e}"))?;
    std::fs::write(path, body).map_err(|e| format!("write {}: {e}", path.display()))
}

fn upsert_mcp_at(path: &Path, row: McpServerRow) -> Result<(), String> {
    let mut doc = load_mcp_at(path);
    if let Some(existing) = doc.servers.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&row.name)) {
        *existing = row;
    } else {
        doc.servers.push(row);
    }
    save_mcp_at(path, &doc)
}

fn remove_mcp_at(path: &Path, name: &str) -> Result<bool, String> {
    let mut doc = load_mcp_at(path);
    let before = doc.servers.len();
    doc.servers.retain(|s| !s.name.eq_ignore_ascii_case(name));
    let removed = doc.servers.len() != before;
    if removed {
        save_mcp_at(path, &doc)?;
    }
    Ok(removed)
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn live_rpc(srv: &McpServerRow, method: &str, params: Value) -> Result<Value, String> {
    let mut cmd = Command::new(&srv.command);
    cmd.args(&srv.args)
        .envs(&srv.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("spawn `{}`: {e}", srv.command))?;
    let mut stdin = child.stdin.take().ok_or("mcp stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("mcp stdout unavailable")?;
    let _guard = ChildGuard(child);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if !line.trim().is_empty() && tx.send(line).is_err() {
                break;
            }
        }
    });

    let send_recv = |stdin: &mut std::process::ChildStdin, id: u64, m: &str, p: Value| -> Result<Value, String> {
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": m, "params": p });
        writeln!(stdin, "{req}").map_err(|e| format!("write {m}: {e}"))?;
        stdin.flush().map_err(|e| format!("flush {m}: {e}"))?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(6) {
            let Ok(line) = rx.recv_timeout(Duration::from_secs(6).saturating_sub(started.elapsed())) else {
                return Err(format!("timeout waiting for `{m}`"));
            };
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue; };
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(err.to_string());
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
        Err(format!("timeout waiting for `{m}`"))
    };

    let _ = send_recv(&mut stdin, 1, "initialize", json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": { "name": "tomoyo-cli", "version": env!("CARGO_PKG_VERSION") }
    }))?;
    let _ = writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let _ = stdin.flush();
    send_recv(&mut stdin, 2, method, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_toml_crud_roundtrips() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_cli_mcp_{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        upsert_mcp_at(
            &tmp,
            McpServerRow {
                name: "demo".into(),
                command: "echo".into(),
                args: vec!["--flag".into()],
                enabled: true,
                env: BTreeMap::from([("TOKEN".into(), "1".into())]),
            },
        )
        .unwrap();
        let doc = load_mcp_at(&tmp);
        assert_eq!(doc.servers.len(), 1);
        assert_eq!(doc.servers[0].env.get("TOKEN").map(String::as_str), Some("1"));
        assert!(remove_mcp_at(&tmp, "demo").unwrap());
        let _ = std::fs::remove_file(&tmp);
    }
}
