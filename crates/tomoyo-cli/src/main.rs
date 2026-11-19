//! Tomoyo Open Ecosystem CLI (`tomoyo-cli`):
//! - `tomoyo-cli new <name> --lang <rust|wasm|python|webview>`
//! - `tomoyo-cli verify <plugin-dir>`
//! - `tomoyo-cli hash <plugin-dir>`
//! - `tomoyo-cli import-utools <utools-plugin-dir>`
//! - `tomoyo-cli registry list [--index <path-or-https-url>]`
//! - `tomoyo-cli install <plugin-id> [--index <path-or-https-url>] [--dest <plugins-dir>]`

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tomoyo_core::ExternalPluginBridge;
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{PluginManifest, PluginRegistryIndex, QueryContext};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "-h" | "--help" | "help") {
        print_usage();
        return;
    }

    let code = match args[0].as_str() {
        "new" => cmd_new(&args[1..]),
        "verify" => cmd_verify(&args[1..]),
        "hash" => cmd_hash(&args[1..]),
        "import-utools" => cmd_import_utools(&args[1..]),
        "registry" => cmd_registry(&args[1..]),
        "install" => cmd_install(&args[1..]),
        other => {
            eprintln!("Unknown command `{other}`.\n");
            print_usage();
            1
        }
    };

    if code != 0 {
        std::process::exit(code);
    }
}

fn print_usage() {
    println!(
        "tomoyo-cli {} — Open Plugin SDK, uTools Importer & Decentralized Registry Tool\n\
         \n\
         USAGE:\n\
         \t tomoyo-cli new <name> [--lang rust|wasm|python|webview] [--dir <path>]\n\
         \t tomoyo-cli verify <plugin-dir>\n\
         \t tomoyo-cli hash <plugin-dir>\n\
         \t tomoyo-cli import-utools <utools-plugin-dir>\n\
         \t tomoyo-cli registry list [--index <path-or-https-url>]\n\
         \t tomoyo-cli install <plugin-id> [--index <path-or-https-url>] [--dest <dir>]\n",
        env!("CARGO_PKG_VERSION")
    );
}

fn flag_val<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

fn cmd_new(args: &[String]) -> i32 {
    let Some(name) = args.first().filter(|s| !s.starts_with('-')) else {
        eprintln!("Usage: tomoyo-cli new <name> [--lang rust|wasm|python|webview]");
        return 1;
    };
    let lang = flag_val(args, "--lang").unwrap_or("python");
    let root = flag_val(args, "--dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("plugins"));
    let slug = name
        .to_ascii_lowercase()
        .replace(|c: char| !c.is_ascii_alphanumeric(), "-");
    let target_dir = root.join(&slug);

    if target_dir.exists() {
        eprintln!("Error: target directory already exists: {}", target_dir.display());
        return 1;
    }
    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        eprintln!("Failed to create directory: {e}");
        return 1;
    }

    let result = match lang {
        "python" => scaffold_python(&target_dir, name, &slug),
        "rust" => scaffold_rust(&target_dir, name, &slug),
        "wasm" => scaffold_wasm(&target_dir, name, &slug),
        "webview" => scaffold_webview(&target_dir, name, &slug),
        other => {
            eprintln!("Unsupported --lang `{other}`. Choose: rust, wasm, python, webview");
            return 1;
        }
    };

    match result {
        Ok(()) => {
            println!("Created {lang} plugin `{name}` at {}", target_dir.display());
            println!("Next: run `tomoyo-cli verify {}`", target_dir.display());
            0
        }
        Err(e) => {
            eprintln!("Scaffold failed: {e}");
            1
        }
    }
}

fn cmd_hash(args: &[String]) -> i32 {
    let Some(dir) = args.first() else {
        eprintln!("Usage: tomoyo-cli hash <plugin-dir>");
        return 1;
    };
    match compute_plugin_sha256(Path::new(dir)) {
        Ok(digest) => {
            println!("{digest}");
            0
        }
        Err(e) => {
            eprintln!("Error hashing {}: {e}", dir);
            1
        }
    }
}

fn cmd_verify(args: &[String]) -> i32 {
    let Some(dir_str) = args.first() else {
        eprintln!("Usage: tomoyo-cli verify <plugin-dir>");
        return 1;
    };
    let dir = Path::new(dir_str);
    let manifest_path = dir.join("tomoyo-plugin.toml");
    let raw = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {e}", manifest_path.display());
            return 1;
        }
    };
    let manifest = match PluginManifest::from_toml_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Invalid manifest: {e}");
            return 1;
        }
    };
    let sha = match compute_plugin_sha256(dir) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to hash plugin directory: {e}");
            return 1;
        }
    };

    println!(
        "[OK] Manifest valid: `{}` ({}) v{} [{}]",
        manifest.name,
        manifest.id,
        manifest.version,
        manifest.runtime.kind_badge()
    );
    println!("[OK] Triggers: {:?}", manifest.triggers.keywords);
    println!("[OK] Resource cap: {} MB · idle {}s", manifest.resources.max_memory_mb, manifest.resources.idle_timeout_secs);
    println!("[OK] Bundle SHA-256: {sha}");

    // Live smoke-test query through real PluginHost!
    if let (Some(parent), Some(first_kw)) = (dir.parent(), manifest.triggers.keywords.first()) {
        let host = PluginHost::new(vec![parent.to_path_buf()]);
        let ctx = QueryContext::new(format!("{first_kw} hello"), 1, 5).with_keyword(first_kw, "hello");
        let items = host.query_keyword(first_kw, &ctx);
        println!(
            "[OK] Live query `{first_kw} hello` returned {} result(s)",
            items.len()
        );
        if let Some(first) = items.first() {
            println!("     Top hit: `{}` — {}", first.title, first.subtitle);
        }
        host.shutdown_all();
    }
    0
}

fn cmd_import_utools(args: &[String]) -> i32 {
    let Some(dir_str) = args.first().filter(|s| !s.starts_with('-')) else {
        eprintln!("Usage: tomoyo-cli import-utools <utools-plugin-dir>");
        return 1;
    };
    let dir = Path::new(dir_str);
    let pj_path = dir.join("plugin.json");
    let raw = match std::fs::read_to_string(&pj_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {e}", pj_path.display());
            return 1;
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Invalid uTools plugin.json: {e}");
            return 1;
        }
    };

    let plugin_name = json
        .get("pluginName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Imported uTools Plugin");
    let description = json
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("Imported from uTools plugin.json");
    let version = json
        .get("version")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("0.1.0");
    let author = json
        .get("author")
        .and_then(|v| v.as_str())
        .unwrap_or("Community");
    let main_entry = json
        .get("main")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("index.html");

    let mut keywords: Vec<String> = Vec::new();
    if let Some(features) = json.get("features").and_then(|v| v.as_array()) {
        for feat in features {
            if let Some(cmds) = feat.get("cmds").and_then(|v| v.as_array()) {
                for cmd in cmds {
                    if let Some(s) = cmd.as_str() {
                        let clean = s.trim().to_string();
                        if !clean.is_empty() && !keywords.contains(&clean) {
                            keywords.push(clean);
                        }
                    } else if let Some(label) = cmd.get("label").and_then(|v| v.as_str()) {
                        let clean = label.trim().to_string();
                        if !clean.is_empty() && !keywords.contains(&clean) {
                            keywords.push(clean);
                        }
                    }
                }
            }
            if keywords.is_empty() {
                if let Some(code) = feat.get("code").and_then(|v| v.as_str()) {
                    let clean = code.trim().to_string();
                    if !clean.is_empty() && !keywords.contains(&clean) {
                        keywords.push(clean);
                    }
                }
            }
        }
    }

    let slug: String = plugin_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let slug = if slug.is_empty() {
        "utools-plugin".to_string()
    } else {
        slug
    };
    if keywords.is_empty() {
        keywords.push(slug.clone());
    }

    let kw_toml = keywords
        .iter()
        .map(|k| serde_json::to_string(k).unwrap_or_else(|_| "\"plugin\"".into()))
        .collect::<Vec<_>>()
        .join(", ");

    let manifest_toml = format!(
        r#"id = "dev.utools.imported.{slug}"
name = {name}
version = {version}
description = {desc}
author = {author}
license = "MIT"

[runtime]
kind = "webview"
entry = {entry}
utools_compat = true
width = 900
height = 620

[triggers]
keywords = [{kw_toml}]

[resources]
max_memory_mb = 256
idle_timeout_secs = 15
"#,
        name = serde_json::to_string(plugin_name).unwrap_or_else(|_| "\"Plugin\"".into()),
        version = serde_json::to_string(version).unwrap_or_else(|_| "\"0.1.0\"".into()),
        desc = serde_json::to_string(description).unwrap_or_else(|_| "\"\"".into()),
        author = serde_json::to_string(author).unwrap_or_else(|_| "\"Community\"".into()),
        entry = serde_json::to_string(main_entry).unwrap_or_else(|_| "\"index.html\"".into()),
    );

    if let Err(e) = PluginManifest::from_toml_str(&manifest_toml) {
        eprintln!("Generated manifest validation failed: {e}");
        return 1;
    }

    let out_path = dir.join("tomoyo-plugin.toml");
    if let Err(e) = std::fs::write(&out_path, &manifest_toml) {
        eprintln!("Failed to write {}: {e}", out_path.display());
        return 1;
    }

    let sha = compute_plugin_sha256(dir).unwrap_or_default();
    println!("[OK] Imported uTools plugin `{plugin_name}` -> {}", out_path.display());
    println!("[OK] Entry: `{main_entry}` · uTools Shim: enabled · Triggers: {:?}", keywords);
    println!("[OK] Bundle SHA-256: {sha}");
    println!("Copy this folder into `%LOCALAPPDATA%\\Tomoyo\\plugins` and run `:reload` inside Tomoyo!");
    0
}

fn cmd_registry(args: &[String]) -> i32 {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("list");
    if sub != "list" {
        eprintln!("Usage: tomoyo-cli registry list [--index <path-or-https-url>]");
        return 1;
    }
    let index_source = flag_val(args, "--index").unwrap_or("registry/index.json");
    let index = match load_registry(index_source) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Failed to load registry `{index_source}`: {e}");
            return 1;
        }
    };
    println!("Registry: {} ({} plugins)\n", index.name, index.plugins.len());
    for p in &index.plugins {
        println!(
            "  {:<28} [{}] v{}  triggers={:?}\n    {}\n    sha256: {}\n",
            p.id, p.runtime_kind, p.version, p.keywords, p.description, p.sha256
        );
    }
    0
}

fn cmd_install(args: &[String]) -> i32 {
    let Some(plugin_id) = args.first().filter(|s| !s.starts_with('-')) else {
        eprintln!("Usage: tomoyo-cli install <plugin-id> [--index <path-or-https-url>] [--dest <dir>]");
        return 1;
    };
    let index_source = flag_val(args, "--index").unwrap_or("registry/index.json");
    let index_path = PathBuf::from(index_source);
    let dest_root = flag_val(args, "--dest")
        .map(PathBuf::from)
        .unwrap_or_else(|| tomoyo_core::data_dir().join("plugins"));

    let index = match load_registry(index_source) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Failed to load registry `{index_source}`: {e}");
            return 1;
        }
    };

    let Some(entry) = index.plugins.iter().find(|p| p.id == *plugin_id) else {
        eprintln!("Plugin `{plugin_id}` not found in registry.");
        return 1;
    };

    let base_dir = index_path
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| Path::new("."));
    let src_dir = if Path::new(&entry.source).is_absolute() {
        PathBuf::from(&entry.source)
    } else if Path::new(&entry.source).exists() {
        PathBuf::from(&entry.source)
    } else {
        base_dir.join(&entry.source)
    };

    if !src_dir.is_dir() {
        eprintln!("Source directory not found: {}", src_dir.display());
        return 1;
    }

    let actual_sha = match compute_plugin_sha256(&src_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to compute SHA-256: {e}");
            return 1;
        }
    };

    if !entry.sha256.eq_ignore_ascii_case(&actual_sha) {
        eprintln!(
            "SECURITY ERROR: SHA-256 mismatch for `{plugin_id}`!\n  Expected: {}\n  Actual:   {}",
            entry.sha256, actual_sha
        );
        return 2;
    }

    let slug = entry.id.replace('.', "-");
    let target_dir = dest_root.join(slug);
    if let Err(e) = copy_plugin_files(&src_dir, &target_dir) {
        eprintln!("Installation copy failed: {e}");
        return 1;
    }

    println!(
        "[VERIFIED] SHA-256 matched ({})\n[INSTALLED] `{}` v{} -> {}",
        &actual_sha[..16],
        entry.name,
        entry.version,
        target_dir.display()
    );
    println!("Run `:reload` inside Tomoyo to activate immediately.");
    0
}

fn load_registry(source: &str) -> Result<PluginRegistryIndex, String> {
    if source.starts_with("https://") || source.starts_with("http://") {
        let resp = tomoyo_core::http::http_get(source, 8000, 4 * 1024 * 1024)?;
        if resp.status_code != 200 {
            return Err(format!("HTTP {}", resp.status_code));
        }
        return serde_json::from_slice(&resp.body).map_err(|e| e.to_string());
    }
    let raw = std::fs::read_to_string(source).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

/// Deterministic SHA-256 over all non-build plugin files (sorted relative paths + bytes).
pub fn compute_plugin_sha256(dir: &Path) -> Result<String, std::io::Error> {
    let mut files = BTreeMap::<String, PathBuf>::new();
    collect_plugin_files(dir, dir, &mut files)?;

    let mut hasher = Sha256::new();
    for (rel, path) in files {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        let data = std::fs::read(&path)?;
        hasher.update((data.len() as u64).to_le_bytes());
        hasher.update(&data);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn collect_plugin_files(
    root: &Path,
    cur: &Path,
    out: &mut BTreeMap<String, PathBuf>,
) -> Result<(), std::io::Error> {
    for ent in std::fs::read_dir(cur)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name();
        let name_str = name.to_string_lossy();
        if matches!(
            name_str.as_ref(),
            "target" | "node_modules" | "__pycache__" | ".git" | "Cargo.lock"
        ) {
            continue;
        }
        if path.is_dir() {
            collect_plugin_files(root, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(rel, path);
        }
    }
    Ok(())
}

fn copy_plugin_files(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    let mut files = BTreeMap::new();
    collect_plugin_files(src, src, &mut files)?;
    std::fs::create_dir_all(dst)?;
    for (rel, path) in files {
        let target = dst.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&path, &target)?;
    }
    Ok(())
}

fn scaffold_python(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
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

fn scaffold_rust(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
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

fn scaffold_wasm(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
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

fn scaffold_webview(dir: &Path, name: &str, slug: &str) -> std::io::Result<()> {
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
