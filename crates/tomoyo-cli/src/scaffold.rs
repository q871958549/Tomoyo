//! Four-language Tomoyo plugin project templates (`python`, `rust`, `wasm`, `webview`).

use std::path::Path;

pub fn scaffold_python(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
    let manifest = format!(
        r#"id = "dev.community.{slug}"
name = "{name}"
version = "0.1.0"
description = "Custom Tomoyo Python plugin"
author = "You"
license = "MIT"

[runtime]
kind = "stdio_jsonrpc"
command = "python"
args = ["-u", "main.py"]

[triggers]
keywords = ["{slug}"]

[resources]
max_memory_mb = 64
idle_timeout_secs = 45
"#
    );
    let main_py = format!(
        r#"import json, sys

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    rid, method, params = req.get("id", 0), req.get("method", ""), req.get("params", {{}})
    if method == "tomoyo/handshake":
        res = {{"protocol_version": 1, "name": "{name}", "version": "0.1.0"}}
    elif method == "tomoyo/query":
        q = (params.get("args") or "").strip() or "world"
        res = {{"items": [{{"id": "{slug}:1", "provider_id": "dev.community.{slug}", "title": f"Hello, {{q}}!", "subtitle": "From {name} Python plugin", "category": "plugin", "score": 9500, "icon": {{"kind": "monogram", "value": "PY"}}, "copy_text": q, "actions": [{{"id": "copy", "label": "Copy", "kind": {{"type": "copy_text", "text": q}}}}]}}]}}
    elif method == "tomoyo/shutdown":
        sys.stdout.write(json.dumps({{"jsonrpc": "2.0", "id": rid, "result": {{"ok": True}}}}) + "\n")
        sys.stdout.flush()
        break
    else:
        res = {{"hide_window": True}}
    sys.stdout.write(json.dumps({{"jsonrpc": "2.0", "id": rid, "result": res}}) + "\n")
    sys.stdout.flush()
"#
    );
    std::fs::write(dir.join("tomoyo-plugin.toml"), manifest)?;
    std::fs::write(dir.join("main.py"), main_py)?;
    Ok(())
}

pub fn scaffold_rust(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
    let manifest = format!(
        r#"id = "dev.community.{slug}"
name = "{name}"
version = "0.1.0"
description = "Native Rust Tomoyo plugin"
license = "MIT"

[runtime]
kind = "stdio_jsonrpc"
command = "tomoyo-plugin-{slug}"
args = []

[triggers]
keywords = ["{slug}"]

[resources]
max_memory_mb = 32
idle_timeout_secs = 45
"#
    );
    let cargo = format!(
        r#"[package]
name = "tomoyo-plugin-{slug}"
version = "0.1.0"
edition = "2024"

[dependencies]
tomoyo-sdk = {{ path = "../../crates/tomoyo-sdk" }}
"#
    );
    let main_rs = format!(
        r#"use tomoyo_sdk::*;

struct MyPlugin;
impl TomoyoPlugin for MyPlugin {{
    fn meta(&self) -> PluginMeta {{ PluginMeta::new("{name}", "0.1.0") }}
    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {{
        let text = if ctx.args.trim().is_empty() {{ "Tomoyo" }} else {{ ctx.args.trim() }};
        vec![ResultItem {{
            id: "{slug}:1".into(),
            provider_id: "dev.community.{slug}".into(),
            title: format!("Hello {{text}}!"),
            subtitle: "Native Rust STDIO plugin".into(),
            category: ResultCategory::Plugin,
            score: 9500,
            icon: IconSpec::Monogram("RS".into()),
            badge: Some("RUST".into()),
            copy_text: Some(text.into()),
            actions: vec![ActionSpec {{
                id: "copy".into(),
                label: "Copy".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText {{ text: text.into(), paste_immediately: false }},
            }}],
            preview: None,
        }}]
    }}
}}
fn main() {{ tomoyo_sdk::run(MyPlugin); }}
"#
    );
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(dir.join("tomoyo-plugin.toml"), manifest)?;
    std::fs::write(dir.join("Cargo.toml"), cargo)?;
    std::fs::write(dir.join("src/main.rs"), main_rs)?;
    Ok(())
}

pub fn scaffold_wasm(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
    let manifest = format!(
        r#"id = "dev.community.{slug}"
name = "{name}"
version = "0.1.0"
description = "Capability-sandboxed WASM Tomoyo plugin"
license = "MIT"

[runtime]
kind = "wasm"
module = "plugin.wasm"
max_fuel = 5000000

[triggers]
keywords = ["{slug}"]

[resources]
max_memory_mb = 16
idle_timeout_secs = 30
"#
    );
    let cargo = format!(
        r#"[package]
name = "tomoyo-wasm-{slug}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
tomoyo-wasm-sdk = {{ path = "../../crates/tomoyo-wasm-sdk" }}
"#
    );
    let lib_rs = format!(
        r#"use tomoyo_wasm_sdk::*;

struct MyWasmPlugin;
impl TomoyoWasmPlugin for MyWasmPlugin {{
    fn query(ctx: &QueryContext) -> Vec<ResultItem> {{
        let text = if ctx.args.trim().is_empty() {{ "WASM" }} else {{ ctx.args.trim() }};
        vec![ResultItem {{
            id: "{slug}:1".into(),
            provider_id: "dev.community.{slug}".into(),
            title: format!("Sandboxed: {{text}}"),
            subtitle: "Fuel-metered WASM plugin".into(),
            category: ResultCategory::Plugin,
            score: 9500,
            icon: IconSpec::Monogram("WSM".into()),
            badge: Some("WASM".into()),
            copy_text: Some(text.into()),
            actions: vec![ActionSpec {{
                id: "copy".into(),
                label: "Copy".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText {{ text: text.into(), paste_immediately: false }},
            }}],
            preview: None,
        }}]
    }}
}}
export_tomoyo_wasm_plugin!(MyWasmPlugin);
"#
    );
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(dir.join("tomoyo-plugin.toml"), manifest)?;
    std::fs::write(dir.join("Cargo.toml"), cargo)?;
    std::fs::write(dir.join("src/lib.rs"), lib_rs)?;
    Ok(())
}

pub fn scaffold_webview(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
    let manifest = format!(
        r#"id = "dev.community.{slug}"
name = "{name}"
version = "0.1.0"
description = "Ephemeral WebView Tomoyo plugin with window.utools compatibility"
license = "MIT"

[runtime]
kind = "webview"
entry = "index.html"
utools_compat = true
width = 860
height = 580

[triggers]
keywords = ["{slug}"]

[resources]
max_memory_mb = 256
idle_timeout_secs = 10
"#
    );
    let html = format!(
        r#"<!doctype html>
<html>
<head><meta charset="utf-8"><title>{name}</title>
<style>body{{font-family:'Segoe UI',sans-serif;background:#16161e;color:#f3f4f8;padding:24px}}button{{background:#7c6af7;color:#fff;border:0;padding:8px 16px;border-radius:8px;cursor:pointer}}</style>
</head>
<body>
  <h2>{name}</h2>
  <p id="status">Ready (press Esc to close & release 100% WebView memory)</p>
  <button onclick="utools.copyText('Hello from {name}!')">Copy via utools.copyText</button>
  <script>
    if (window.utools) {{
      utools.onPluginEnter((action) => {{
        document.getElementById('status').textContent = 'Entered with payload: ' + (action.payload || '(empty)');
      }});
    }}
  </script>
</body>
</html>
"#
    );
    std::fs::write(dir.join("tomoyo-plugin.toml"), manifest)?;
    std::fs::write(dir.join("index.html"), html)?;
    Ok(())
}
