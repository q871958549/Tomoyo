//! Tomoyo Open Ecosystem CLI (`tomoyo-cli`):
//! - `tomoyo-cli new <name> --lang <rust|wasm|python|webview>`
//! - `tomoyo-cli dev <plugin-dir> [--query "<args>"] [--once]`
//! - `tomoyo-cli verify <plugin-dir>`
//! - `tomoyo-cli hash <plugin-dir>`
//! - `tomoyo-cli import-utools <utools-plugin-dir>`
//! - `tomoyo-cli registry list|check [--index <path-or-https-url>]`
//! - `tomoyo-cli install <plugin-id> [--index <path-or-https-url>] [--dest <plugins-dir>]`

mod dev;
mod hash;
mod scaffold;
mod utools;

use hash::{compute_plugin_sha256, copy_plugin_files};
use scaffold::{scaffold_python, scaffold_rust, scaffold_wasm, scaffold_webview};
use std::path::{Path, PathBuf};
use tomoyo_core::ExternalPluginBridge;
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{PluginManifest, PluginRegistryIndex, QueryContext};
use utools::cmd_import_utools;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "-h" | "--help" | "help") {
        print_usage();
        return;
    }

    let code = match args[0].as_str() {
        "new" => cmd_new(&args[1..]),
        "dev" => dev::cmd_dev(&args[1..]),
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
         \t tomoyo-cli dev <plugin-dir> [--query \"<args>\"] [--once]\n\
         \t tomoyo-cli verify <plugin-dir>\n\
         \t tomoyo-cli hash <plugin-dir>\n\
         \t tomoyo-cli import-utools <utools-plugin-dir>\n\
         \t tomoyo-cli registry list|check [--index <path-or-https-url>]\n\
         \t tomoyo-cli install <plugin-id> [--index <path-or-https-url>] [--dest <dir>]\n",
        env!("CARGO_PKG_VERSION")
    );
}

pub(crate) fn flag_val<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].as_str())
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
    println!(
        "[OK] Resource cap: {} MB · idle {}s",
        manifest.resources.max_memory_mb, manifest.resources.idle_timeout_secs
    );
    println!("[OK] Bundle SHA-256: {sha}");

    if let (Some(parent), Some(first_kw)) = (dir.parent(), manifest.triggers.keywords.first()) {
        let host = PluginHost::new(vec![parent.to_path_buf()]);
        let ctx = QueryContext::new(format!("{first_kw} hello"), 1, 5).with_keyword(first_kw, "hello");
        let items = host.query_keyword(first_kw, &ctx);
        println!("[OK] Live query `{first_kw} hello` returned {} result(s)", items.len());
        if let Some(first) = items.first() {
            println!("     Top hit: `{}` — {}", first.title, first.subtitle);
        }
        host.shutdown_all();
    }
    0
}

fn cmd_registry(args: &[String]) -> i32 {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("list");
    if sub == "check" {
        return dev::cmd_registry_check(&args[1..]);
    }
    if sub != "list" {
        eprintln!("Usage: tomoyo-cli registry list|check [--index <path-or-https-url>]");
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
    let Some(target) = args.first().filter(|s| !s.starts_with('-')) else {
        eprintln!("Usage: tomoyo-cli install <plugin-id | https-zip-url | local.zip> [--sha256 <hex>] [--index <path-or-https-url>] [--dest <dir>]");
        return 1;
    };
    let dest_root = flag_val(args, "--dest")
        .map(PathBuf::from)
        .unwrap_or_else(|| tomoyo_core::data_dir().join("plugins"));

    if target.starts_with("https://")
        || target.starts_with("http://")
        || target.to_ascii_lowercase().ends_with(".zip")
    {
        return install_direct_zip(target, flag_val(args, "--sha256"), &dest_root);
    }

    let index_source = flag_val(args, "--index").unwrap_or("registry/index.json");
    let index_path = PathBuf::from(index_source);
    let index = match load_registry(index_source) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Failed to load registry `{index_source}`: {e}");
            return 1;
        }
    };
    let Some(entry) = index.plugins.iter().find(|p| p.id == *target) else {
        eprintln!("Plugin `{target}` not found in registry.");
        return 1;
    };

    let target_dir = dest_root.join(entry.id.replace('.', "-"));
    if entry.source.starts_with("https://") || entry.source.starts_with("http://") {
        match tomoyo_core::download_and_install_remote_zip(
            &entry.source,
            &entry.sha256,
            true,
            &target_dir,
        ) {
            Ok(actual_sha) => {
                println!(
                    "[VERIFIED] Remote ZIP SHA-256 matched ({})\n[INSTALLED] `{}` v{} -> {}",
                    &actual_sha[..16],
                    entry.name,
                    entry.version,
                    target_dir.display()
                );
                println!("Run `:reload` inside Tomoyo to activate immediately.");
                return 0;
            }
            Err(e) => {
                eprintln!("SECURITY/INSTALL ERROR: {e}");
                return 2;
            }
        }
    }

    let base_dir = index_path
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| Path::new("."));
    let src_dir = if Path::new(&entry.source).is_absolute() || Path::new(&entry.source).exists() {
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
            "SECURITY ERROR: SHA-256 mismatch for `{target}`!\n  Expected: {}\n  Actual:   {}",
            entry.sha256, actual_sha
        );
        return 2;
    }

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

fn install_direct_zip(source: &str, sha256_opt: Option<&str>, dest_root: &Path) -> i32 {
    let stage = std::env::temp_dir().join(format!("tomoyo-cli-stage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let res = if source.starts_with("https://") || source.starts_with("http://") {
        let Some(sha) = sha256_opt else {
            eprintln!("SECURITY ERROR: Remote ZIP URL installation requires `--sha256 <64-hex>`");
            return 2;
        };
        tomoyo_core::download_and_install_remote_zip(source, sha, true, &stage)
    } else {
        tomoyo_core::install_zip_file_verified(Path::new(source), sha256_opt.unwrap_or(""), &stage)
    };
    let sha = match res {
        Ok(s) => s,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            eprintln!("Install failed: {e}");
            return 2;
        }
    };
    let manifest_raw = match std::fs::read_to_string(stage.join("tomoyo-plugin.toml")) {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            eprintln!("Missing tomoyo-plugin.toml: {e}");
            return 1;
        }
    };
    let manifest = match PluginManifest::from_toml_str(&manifest_raw) {
        Ok(m) => m,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            eprintln!("Invalid tomoyo-plugin.toml: {e}");
            return 1;
        }
    };
    let target_dir = dest_root.join(manifest.id.replace('.', "-"));
    if let Err(e) = copy_plugin_files(&stage, &target_dir) {
        let _ = std::fs::remove_dir_all(&stage);
        eprintln!("Copy failed: {e}");
        return 1;
    }
    let _ = std::fs::remove_dir_all(&stage);
    println!(
        "[VERIFIED] ZIP SHA-256 ({})\n[INSTALLED] `{}` ({}) v{} -> {}",
        &sha[..16],
        manifest.name,
        manifest.id,
        manifest.version,
        target_dir.display()
    );
    println!("Run `:reload` inside Tomoyo to activate immediately.");
    0
}

pub(crate) fn load_registry(source: &str) -> Result<PluginRegistryIndex, String> {
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
