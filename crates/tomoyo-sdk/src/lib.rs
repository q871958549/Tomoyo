//! Ergonomic Rust SDK for authoring out-of-process STDIO JSON-RPC 2.0 plugins for
//! **Tomoyo Launcher** (`github.com/q871958549/Tomoyo`).
//!
//! Plugins run on demand, stay isolated (Linux process groups + `PR_SET_PDEATHSIG` /
//! Windows Job Objects), and exit after idle timeout for 0 MB idle memory overhead.
//! Supports Protocol v2 declarative hotkeys, events (`TomoyoPlugin::on_event`), and typed settings.
//!
//! Example:
//! ```no_run
//! use tomoyo_sdk::{TomoyoPlugin, PluginMeta, QueryContext, ResultItem, ExecuteRequest, ExecuteResponse};
//!
//! struct MyPlugin;
//! impl TomoyoPlugin for MyPlugin {
//!     fn meta(&self) -> PluginMeta { PluginMeta::new("My Plugin", "0.1.0") }
//!     fn query(&self, _ctx: &QueryContext) -> Vec<ResultItem> { vec![] }
//! }
//! fn main() { tomoyo_sdk::run(MyPlugin); }
//! ```

pub use tomoyo_protocol::*;

use std::io::{self, BufRead, Write};

#[derive(Debug, Clone)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
}

impl PluginMeta {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

pub trait TomoyoPlugin {
    fn meta(&self) -> PluginMeta;
    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem>;
    fn execute(&self, _req: &ExecuteRequest) -> ExecuteResponse {
        ExecuteResponse::default()
    }
    /// Protocol v2 optional event hook (`tomoyo/event`, e.g. clipboard or plugin hotkey).
    fn on_event(&self, _ev: &PluginEventPayload) -> ExecuteResponse {
        ExecuteResponse::default()
    }
}

/// Runs the newline-delimited JSON-RPC 2.0 stdio event loop for a Tomoyo plugin.
pub fn run<P: TomoyoPlugin>(plugin: P) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let meta = plugin.meta();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::err(0, -32700, format!("parse error: {e}"));
                let _ = write_response(&mut out, &resp);
                continue;
            }
        };

        let mut should_exit = false;
        let resp = match req.method.as_str() {
            METHOD_HANDSHAKE => {
                let res = HandshakeResult {
                    protocol_version: PROTOCOL_VERSION,
                    name: meta.name.clone(),
                    version: meta.version.clone(),
                };
                JsonRpcResponse::ok(req.id, serde_json::to_value(res).unwrap())
            }
            METHOD_QUERY => match serde_json::from_value::<QueryContext>(req.params) {
                Ok(ctx) => {
                    let items = plugin.query(&ctx);
                    JsonRpcResponse::ok(
                        req.id,
                        serde_json::to_value(QueryResponse { items }).unwrap(),
                    )
                }
                Err(e) => JsonRpcResponse::err(req.id, -32602, format!("invalid query params: {e}")),
            },
            METHOD_EXECUTE => match serde_json::from_value::<ExecuteRequest>(req.params) {
                Ok(exec_req) => {
                    let res = plugin.execute(&exec_req);
                    JsonRpcResponse::ok(req.id, serde_json::to_value(res).unwrap())
                }
                Err(e) => {
                    JsonRpcResponse::err(req.id, -32602, format!("invalid execute params: {e}"))
                }
            },
            METHOD_EVENT => match serde_json::from_value::<PluginEventPayload>(req.params) {
                Ok(ev) => {
                    let res = plugin.on_event(&ev);
                    JsonRpcResponse::ok(req.id, serde_json::to_value(res).unwrap())
                }
                Err(e) => JsonRpcResponse::err(req.id, -32602, format!("invalid event params: {e}")),
            },
            METHOD_SHUTDOWN => {
                should_exit = true;
                JsonRpcResponse::ok(req.id, serde_json::json!({ "ok": true }))
            }
            other => JsonRpcResponse::err(req.id, -32601, format!("unknown method: {other}")),
        };

        if write_response(&mut out, &resp).is_err() || should_exit {
            break;
        }
    }
}

fn write_response<W: Write>(out: &mut W, resp: &JsonRpcResponse) -> io::Result<()> {
    let json = serde_json::to_string(resp).unwrap_or_else(|_| "{}".to_string());
    out.write_all(json.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}
