//! Protocol v2 + TypeScript-typed project templates (`python`, `rust`, `wasm`, `webview`).

use std::path::{Component, Path};

const TOMOYO_DTS: &str = include_str!("../../../docs/tomoyo.d.ts");

fn relative_path(from: &Path, to: &Path) -> Option<String> {
    let f_comps: Vec<_> = from.components().collect();
    let t_comps: Vec<_> = to.components().collect();
    let mut common = 0usize;
    while common < f_comps.len() && common < t_comps.len() && f_comps[common] == t_comps[common] {
        common += 1;
    }
    if common == 0 && matches!(f_comps.first(), Some(Component::Prefix(_))) {
        return None;
    }
    let mut parts = Vec::new();
    for c in &f_comps[common..] {
        if matches!(c, Component::Normal(_)) {
            parts.push("..".to_string());
        }
    }
    for c in &t_comps[common..] {
        if let Component::Normal(s) = c {
            parts.push(s.to_string_lossy().into_owned());
        }
    }
    Some(if parts.is_empty() { ".".into() } else { parts.join("/") })
}

pub(crate) fn resolve_sdk_dep(target_dir: &Path, crate_name: &str, sdk_override: Option<&Path>) -> String {
    let abs_target = std::fs::canonicalize(target_dir).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|c| c.join(target_dir))
            .unwrap_or_else(|_| target_dir.to_path_buf())
    });

    if let Some(root) = sdk_override {
        let abs_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let crate_dir = abs_root.join(crate_name);
        let rel = relative_path(&abs_target, &crate_dir)
            .unwrap_or_else(|| crate_dir.to_string_lossy().replace('\\', "/"));
        return format!("{crate_name} = {{ path = \"{rel}\" }}");
    }

    let mut cur = Some(abs_target.as_path());
    while let Some(anc) = cur {
        let candidate = anc.join("crates").join(crate_name);
        if candidate.join("Cargo.toml").is_file() {
            let rel = relative_path(&abs_target, &candidate)
                .unwrap_or_else(|| candidate.to_string_lossy().replace('\\', "/"));
            return format!("{crate_name} = {{ path = \"{rel}\" }}");
        }
        cur = anc.parent();
    }

    format!("{crate_name} = {{ git = \"https://github.com/q871958549/Tomoyo\", branch = \"main\" }}")
}

pub fn scaffold_python(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
    let manifest = format!(
        r#"id = "dev.community.{slug}"
name = "{name}"
version = "0.1.0"
description = "Custom Tomoyo Protocol v2 Python plugin"
author = "You"
license = "MIT"

[runtime]
kind = "stdio_jsonrpc"
command = "python3"
args = ["-u", "main.py"]

[triggers]
keywords = ["{slug}"]

[resources]
max_memory_mb = 64
idle_timeout_secs = 45

# Optional Protocol v2 declarative settings (rendered automatically in Settings Center):
[[settings]]
key = "prefix"
label = "Greeting Prefix"
description = "Shown before query text"
type = "text"
default = "Hello"
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
        res = {{"protocol_version": 2, "name": "{name}", "version": "0.1.0"}}
    elif method == "tomoyo/query":
        q = (params.get("args") or "").strip() or "world"
        prefix = (params.get("plugin_settings") or {{}}).get("prefix") or "Hello"
        title = f"{{prefix}}, {{q}}!"
        res = {{"items": [{{
            "id": "{slug}:1",
            "provider_id": "dev.community.{slug}",
            "title": title,
            "subtitle": "From {name} Python plugin (Protocol v2)",
            "category": "plugin",
            "score": 9500,
            "icon": {{"kind": "monogram", "value": "PY"}},
            "badge": "PY",
            "copy_text": title,
            "actions": [
                {{"id": "copy", "label": "Copy", "shortcut": "Enter", "kind": {{"type": "copy_text", "text": title}}}},
                {{"id": "ping", "label": "Run tomoyo/execute", "kind": {{"type": "plugin_execute", "plugin_id": "dev.community.{slug}", "action_id": "ping", "payload": {{"text": title}}}}}}
            ]
        }}]}}
    elif method == "tomoyo/execute":
        txt = (params.get("payload") or {{}}).get("text") or "ok"
        res = {{"hide_window": False, "toast": f"Executed {slug}: {{txt}}"}}
    elif method == "tomoyo/event":
        ev = params.get("event", "")
        res = {{"hide_window": False, "toast": f"Event received: {{ev}}"}}
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
    write_plugin_readme(dir, name, slug, "python")
}

pub fn scaffold_rust(dir: &Path, name: &str, slug: &str, sdk_dir: Option<&Path>) -> std::io::Result<()> {
    let manifest = format!(
        r#"id = "dev.community.{slug}"
name = "{name}"
version = "0.1.0"
description = "Native Rust Protocol v2 Tomoyo plugin"
author = "You"
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

[[settings]]
key = "uppercase"
label = "Uppercase Output"
type = "bool"
default = false
"#
    );
    let sdk_dep = resolve_sdk_dep(dir, "tomoyo-sdk", sdk_dir);
    let cargo = format!(
        r#"[package]
name = "tomoyo-plugin-{slug}"
version = "0.1.0"
edition = "2024"

[dependencies]
{sdk_dep}
"#
    );
    let main_rs = format!(
        r#"use tomoyo_sdk::*;

struct MyPlugin;
impl TomoyoPlugin for MyPlugin {{
    fn meta(&self) -> PluginMeta {{
        PluginMeta::new("{name}", "0.1.0")
    }}

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {{
        let raw = ctx.input_or("Tomoyo");
        let text = if ctx.setting_bool("uppercase", false) {{ raw.to_uppercase() }} else {{ raw.to_string() }};
        vec![
            ResultItem::plugin("{slug}:1", "dev.community.{slug}", format!("Hello {{text}}!"), "Native Rust STDIO plugin")
                .with_monogram("RS")
                .with_badge("RUST")
                .with_copy(&text)
                .with_action(ActionSpec::copy(&text))
                .with_action(ActionSpec::plugin_exec("dev.community.{slug}", "greet", "Execute Hook", serde_json::json!({{ "text": text }})))
                .with_text_preview("{name}", format!("Input: {{raw}}\nOutput: {{text}}")),
        ]
    }}

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {{
        let msg = req.payload.get("text").and_then(|v| v.as_str()).unwrap_or(&req.action_id);
        ExecuteResponse::toast(format!("Executed {{}}: {{msg}}", req.action_id))
    }}
}}

fn main() {{
    tomoyo_sdk::run(MyPlugin);
}}
"#
    );
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(dir.join("tomoyo-plugin.toml"), manifest)?;
    std::fs::write(dir.join("Cargo.toml"), cargo)?;
    std::fs::write(dir.join("src/main.rs"), main_rs)?;
    write_plugin_readme(dir, name, slug, "rust")
}

pub fn scaffold_wasm(dir: &Path, name: &str, slug: &str, sdk_dir: Option<&Path>) -> std::io::Result<()> {
    let manifest = format!(
        r#"id = "dev.community.{slug}"
name = "{name}"
version = "0.1.0"
description = "Capability-sandboxed WASM Tomoyo plugin"
author = "You"
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
    let sdk_dep = resolve_sdk_dep(dir, "tomoyo-wasm-sdk", sdk_dir);
    let cargo = format!(
        r#"[package]
name = "tomoyo-wasm-{slug}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
{sdk_dep}
serde_json = "1.0"
"#
    );
    let lib_rs = format!(
        r#"use tomoyo_wasm_sdk::*;

struct MyWasmPlugin;
impl TomoyoWasmPlugin for MyWasmPlugin {{
    fn query(ctx: &QueryContext) -> Vec<ResultItem> {{
        let text = ctx.input_or("WASM");
        vec![
            ResultItem::plugin("{slug}:1", "dev.community.{slug}", format!("Sandboxed: {{text}}"), "Fuel-metered WASM plugin")
                .with_monogram("WSM")
                .with_badge("WASM")
                .with_copy(text)
                .with_action(ActionSpec::copy(text))
                .with_kv_preview("{name}", vec![("Sandbox".into(), "wasmi".into()), ("Input".into(), text.into())]),
        ]
    }}
}}
export_tomoyo_wasm_plugin!(MyWasmPlugin);
"#
    );
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(dir.join("tomoyo-plugin.toml"), manifest)?;
    std::fs::write(dir.join("Cargo.toml"), cargo)?;
    std::fs::write(dir.join("src/lib.rs"), lib_rs)?;
    write_plugin_readme(dir, name, slug, "wasm")
}

pub fn scaffold_webview(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
    let manifest = format!(
        r#"id = "dev.community.{slug}"
name = "{name}"
version = "0.1.0"
description = "Ephemeral WebView Tomoyo plugin with window.tomoyo + window.utools API"
author = "You"
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
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>{name}</title>
  <style>
    :root {{ --bg:#f7f8fb; --card:#ffffff; --fg:#141722; --muted:#636b82; --border:#dde2ee; --accent:#6b5ce7; }}
    :root[data-theme="dark"] {{ --bg:#14151e; --card:#1b1d29; --fg:#f3f4f8; --muted:#9aa3bd; --border:#2a2e42; --accent:#7c6af7; }}
    body {{ margin:0; font-family:system-ui,-apple-system,sans-serif; background:var(--bg); color:var(--fg); padding:24px; }}
    .card {{ background:var(--card); border:1px solid var(--border); border-radius:12px; padding:18px; max-width:680px; }}
    .muted {{ color:var(--muted); font-size:13px; }}
    .row {{ display:flex; gap:10px; margin-top:14px; flex-wrap:wrap; }}
    input {{ flex:1; padding:8px 10px; border-radius:8px; border:1px solid var(--border); background:var(--bg); color:var(--fg); }}
    button {{ background:var(--accent); color:#fff; border:0; padding:8px 14px; border-radius:8px; cursor:pointer; font-weight:600; }}
    button.sec {{ background:transparent; color:var(--fg); border:1px solid var(--border); }}
  </style>
</head>
<body>
  <div class="card">
    <h2>{name}</h2>
    <p id="status" class="muted">Ready (0 MB idle RAM — Esc closes window immediately)</p>
    <div class="row">
      <input id="note" type="text" placeholder="Type text saved via tomoyo.dbStorage..." />
      <button onclick="saveAndCopy()">Save &amp; Copy</button>
      <button class="sec" onclick="window.tomoyo && tomoyo.togglePinTopmost()">Pin Window</button>
    </div>
  </div>
  <script>
    // For TypeScript / VS Code autocomplete, see ./tomoyo.d.ts in this folder.
    function applyTheme() {{
      const dark = window.tomoyo ? tomoyo.isDarkColors() : true;
      document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    }}
    function saveAndCopy() {{
      const val = document.getElementById('note').value || 'Hello from {name}!';
      if (window.tomoyo) {{
        tomoyo.dbStorage.setItem('last_value', val);
        tomoyo.copyText(val);
        document.getElementById('status').textContent = 'Copied & persisted in tomoyo.dbStorage!';
      }}
    }}
    applyTheme();
    if (window.tomoyo) {{
      const prev = tomoyo.dbStorage.getItem('last_value');
      if (prev) document.getElementById('note').value = String(prev);
      tomoyo.onEnter((action) => {{
        document.getElementById('status').textContent = 'Query: ' + (action.payload || '(empty)') + ' · OS: ' + tomoyo.platform;
      }});
    }}
  </script>
</body>
</html>
"#
    );
    std::fs::write(dir.join("tomoyo-plugin.toml"), manifest)?;
    std::fs::write(dir.join("index.html"), html)?;
    std::fs::write(dir.join("tomoyo.d.ts"), TOMOYO_DTS)?;
    write_plugin_readme(dir, name, slug, "webview")
}

fn write_plugin_readme(dir: &Path, name: &str, slug: &str, lang: &str) -> std::io::Result<()> {
    let build_step = match lang {
        "wasm" => "1. Build WASM: `cargo build --target wasm32-unknown-unknown --release && cp target/wasm32-unknown-unknown/release/*.wasm plugin.wasm`\n2. Dev test:  `tomoyo-cli dev . --build --once`\n",
        "rust" => "1. Build binary: `cargo build --release` (place binary on PATH or set `[runtime].command`)\n2. Verify:       `tomoyo-cli verify .`\n",
        _ => "1. Verify: `tomoyo-cli verify .`\n",
    };
    let doc = format!(
        "# {name}\n\nTomoyo Launcher `{lang}` plugin (`dev.community.{slug}`).\n\n## Quickstart\n\n{build_step}3. Live watch: `tomoyo-cli dev . --query \"sample\"`\n4. Package ZIP: `tomoyo-cli pack .`\n"
    );
    std::fs::write(dir.join("README.md"), doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomoyo_protocol::PluginManifest;

    #[test]
    fn external_rust_and_wasm_scaffolds_use_git_fallback_and_valid_manifests() {
        let base = std::env::temp_dir().join(format!("tomoyo_scaf_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let py_dir = base.join("py");
        let rs_dir = base.join("rs");
        let ws_dir = base.join("ws");
        let wv_dir = base.join("wv");
        for d in [&py_dir, &rs_dir, &ws_dir, &wv_dir] {
            std::fs::create_dir_all(d).unwrap();
        }
        scaffold_python(&py_dir, "Py Demo", "py-demo").unwrap();
        scaffold_rust(&rs_dir, "Rs Demo", "rs-demo", None).unwrap();
        scaffold_wasm(&ws_dir, "Ws Demo", "ws-demo", None).unwrap();
        scaffold_webview(&wv_dir, "Wv Demo", "wv-demo").unwrap();

        for d in [&py_dir, &rs_dir, &ws_dir, &wv_dir] {
            let raw = std::fs::read_to_string(d.join("tomoyo-plugin.toml")).unwrap();
            PluginManifest::from_toml_str(&raw).unwrap();
        }
        let rs_cargo = std::fs::read_to_string(rs_dir.join("Cargo.toml")).unwrap();
        assert!(rs_cargo.contains("https://github.com/q871958549/Tomoyo"));
        assert!(wv_dir.join("tomoyo.d.ts").is_file());

        let _ = std::fs::remove_dir_all(&base);
    }
}
