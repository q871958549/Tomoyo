# Tomoyo Launcher Plugin SDKs & Protocol v2

Tomoyo Launcher supports four zero-resident-bloat extension models:
1. **Built-in Rust Providers** (`tomoyo-providers`)
2. **STDIO JSON-RPC 2.0 (`stdio_jsonrpc`)** — Rust (`tomoyo-sdk`), Python, Go, Node.js (process-group / Job Object isolated, auto-reaped when idle)
3. **Capability-Sandboxed WASM (`wasm`)** — Rust (`tomoyo-wasm-sdk`) executed via `wasmi` with fuel metering and memory caps
4. **On-Demand WebView (`webview`)** — hosted in `tomoyo-webview-host` (WebKitGTK 4.1 / WebView2) with `window.tomoyo` and baseline `window.utools` compatibility (`tomoyo-cli import-utools`)

## 1. CLI Scaffolding & Local Testing

```bash
tomoyo-cli new my-plugin --lang rust     # or wasm | python | webview
tomoyo-cli dev ./my-plugin --query "hi" --once
tomoyo-cli verify ./my-plugin
tomoyo-cli hash ./my-plugin
tomoyo-cli import-utools ./utools-plugin-dir
```

## 2. Rust STDIO Plugin (`tomoyo-sdk`)

```rust
use tomoyo_sdk::{
    ExecuteRequest, ExecuteResponse, PluginMeta, QueryContext, ResultItem, TomoyoPlugin,
};

struct HelloPlugin;

impl TomoyoPlugin for HelloPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("Hello Plugin", "0.1.0")
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let _ = ctx;
        vec![]
    }

    fn execute(&self, _req: &ExecuteRequest) -> ExecuteResponse {
        ExecuteResponse::default()
    }
}

fn main() {
    tomoyo_sdk::run(HelloPlugin);
}
```

## 3. Zero-Dependency Python STDIO Plugin (`stdio_jsonrpc`)

Plugins read newline-delimited JSON-RPC 2.0 requests (`tomoyo/handshake`, `tomoyo/query`, `tomoyo/execute`, `tomoyo/event`, `tomoyo/shutdown`) from `stdin`, write single-line JSON responses to `stdout`, and log diagnostics to `stderr`.

```python
import json, sys

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    req_id, method, params = req.get("id", 0), req.get("method", ""), req.get("params", {})
    if method == "tomoyo/handshake":
        resp = {"jsonrpc": "2.0", "id": req_id, "result": {"protocol_version": 2, "name": "Py Demo", "version": "0.1.0"}}
    elif method == "tomoyo/query":
        resp = {"jsonrpc": "2.0", "id": req_id, "result": {"items": []}}
    elif method == "tomoyo/shutdown":
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": {"ok": True}}) + "\n")
        sys.stdout.flush()
        break
    else:
        resp = {"jsonrpc": "2.0", "id": req_id, "result": {"hide_window": True}}
    sys.stdout.write(json.dumps(resp) + "\n")
    sys.stdout.flush()
```

## 4. Capability-Sandboxed WASM Plugin (`tomoyo-wasm-sdk`)

```rust
use tomoyo_wasm_sdk::{export_tomoyo_wasm_plugin, QueryContext, ResultItem, TomoyoWasmPlugin};

struct WasmToolkit;

impl TomoyoWasmPlugin for WasmToolkit {
    fn query(_ctx: &QueryContext) -> Vec<ResultItem> {
        vec![]
    }
}

export_tomoyo_wasm_plugin!(WasmToolkit);
```

Compile with:
```bash
cargo build --target wasm32-unknown-unknown --release
```

## 5. Protocol v2 Manifest (`tomoyo-plugin.toml`)

Protocol v2 adds `[[hotkeys]]` (kernel-governed global shortcuts), `events` (`clipboard_text`, `clipboard_image`, `hotkey_triggered`), and `[[settings]]` (`text`, `bool`, `number`, `select` auto-rendered in Settings Center). See the full specification in `docs/PLUGIN_PROTOCOL.md`.
