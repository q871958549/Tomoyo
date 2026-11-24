//! Comprehensive plugin validation (`tomoyo-cli verify`) and static manifest checks.

use crate::hash::compute_plugin_sha256;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tomoyo_core::ExternalPluginBridge;
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{PluginManifest, QueryContext, RuntimeConfig};

pub(crate) fn resolve_plugin_dir(dir: &Path) -> PathBuf {
    std::fs::canonicalize(dir).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|c| c.join(dir))
            .unwrap_or_else(|_| dir.to_path_buf())
    })
}

pub(crate) fn validate_plugin_dir(dir: &Path) -> Result<(PathBuf, PluginManifest, Vec<String>), String> {
    let canon = resolve_plugin_dir(dir);
    if !canon.is_dir() {
        return Err(format!("Plugin directory not found: {}", canon.display()));
    }
    let manifest_path = canon.join("tomoyo-plugin.toml");
    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Failed to read {}: {e}", manifest_path.display()))?;
    let manifest = PluginManifest::from_toml_str(&raw).map_err(|e| format!("Invalid manifest: {e}"))?;

    let mut warnings = Vec::new();
    if !manifest.id.contains('.') || manifest.id.chars().any(char::is_whitespace) {
        warnings.push(format!(
            "plugin `id` `{}` should follow reverse-DNS style without spaces (e.g. `dev.author.tool`)",
            manifest.id
        ));
    }

    let mut hk_ids = HashSet::new();
    for hk in &manifest.hotkeys {
        if hk.id.trim().is_empty() || !hk_ids.insert(hk.id.as_str()) {
            return Err(format!("Duplicate or empty `[[hotkeys]]` id: `{}`", hk.id));
        }
    }

    let mut set_keys = HashSet::new();
    for field in &manifest.settings {
        if field.key.trim().is_empty() || !set_keys.insert(field.key.as_str()) {
            return Err(format!("Duplicate or empty `[[settings]]` key: `{}`", field.key));
        }
    }

    if let Some(re) = manifest.triggers.regex.as_deref()
        && re.trim().is_empty()
    {
        warnings.push("`triggers.regex` is empty; remove it if unused".into());
    }
    if let Some(cat) = manifest.category.as_deref() {
        let low = cat.trim().to_ascii_lowercase();
        if !low.is_empty() && !matches!(low.as_str(), "productivity" | "docs" | "dev" | "ai" | "media" | "system") {
            warnings.push(format!(
                "unrecognized `category` `{cat}` (recommended: productivity, docs, dev, ai, media, system)"
            ));
        }
    }
    let icon_rel = manifest.icon.as_deref().unwrap_or("icon.png");
    if icon_rel.contains("..") {
        return Err(format!("`icon` path `{icon_rel}` must not escape plugin directory"));
    }
    let icon_path = canon.join(icon_rel);
    if icon_path.is_file() {
        let bytes = std::fs::read(&icon_path).map_err(|e| format!("Failed to read icon `{icon_rel}`: {e}"))?;
        if bytes.len() > 64 * 1024 {
            return Err(format!("Plugin icon `{icon_rel}` is {} bytes (max 64 KB)", bytes.len()));
        }
        if icon_rel.to_ascii_lowercase().ends_with(".png") && bytes.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
            return Err(format!("Plugin icon `{icon_rel}` is not a valid PNG image"));
        }
    } else if manifest.icon.is_some() {
        return Err(format!("Declared plugin icon missing: {}", icon_path.display()));
    } else {
        warnings.push("missing dedicated `icon.png` (recommended 128x128 PNG <= 64 KB)".into());
    }

    match &manifest.runtime {
        RuntimeConfig::Webview { entry, .. } => {
            if !entry.starts_with("http://") && !entry.starts_with("https://") {
                let p = canon.join(entry);
                if !p.is_file() {
                    return Err(format!("WebView entry file missing: {}", p.display()));
                }
            }
        }
        RuntimeConfig::Wasm { module, .. } => {
            let p = canon.join(module);
            if !p.is_file() {
                return Err(format!(
                    "WASM module missing: {} (hint: run `cargo build --target wasm32-unknown-unknown --release`)",
                    p.display()
                ));
            }
            let hdr = std::fs::read(&p).map_err(|e| format!("Failed to read {}: {e}", p.display()))?;
            if hdr.get(..4) != Some(b"\0asm") {
                return Err(format!("`{}` is not a valid WebAssembly binary (missing \\0asm magic)", p.display()));
            }
        }
        RuntimeConfig::StdioJsonrpc { command, args, .. } => {
            check_stdio_entrypoint(&canon, command, args, &mut warnings)?;
        }
    }

    Ok((canon, manifest, warnings))
}

fn check_stdio_entrypoint(
    canon: &Path,
    command: &str,
    args: &[String],
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err("`[runtime].command` must not be empty".into());
    }
    let ext = std::env::consts::EXE_EXTENSION;
    let in_dir = canon.join(trimmed).is_file()
        || (!ext.is_empty() && canon.join(format!("{trimmed}.{ext}")).is_file());
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.starts_with('.') {
        if !in_dir {
            return Err(format!("Local STDIO executable missing: {}", canon.join(trimmed).display()));
        }
    } else if !in_dir && !command_exists_in_path(trimmed) {
        warnings.push(format!("runtime command `{trimmed}` was not found on current PATH or plugin dir"));
    }

    for arg in args {
        if !arg.starts_with('-') && (arg.ends_with(".py") || arg.ends_with(".js") || arg.ends_with(".mjs")) {
            let script = canon.join(arg);
            if !script.is_file() {
                return Err(format!("STDIO script file missing: {}", script.display()));
            }
        }
    }
    Ok(())
}

fn command_exists_in_path(cmd: &str) -> bool {
    let direct = Path::new(cmd);
    if direct.is_file() {
        return true;
    }
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    let ext = std::env::consts::EXE_EXTENSION;
    std::env::split_paths(&path_var).any(|dir| {
        let cand = dir.join(cmd);
        cand.is_file() || (!ext.is_empty() && dir.join(format!("{cmd}.{ext}")).is_file())
    })
}

pub(crate) fn cmd_verify(args: &[String]) -> i32 {
    let Some(dir_str) = args.first() else {
        eprintln!("Usage: tomoyo-cli verify <plugin-dir>");
        return 1;
    };
    let (canon, manifest, warnings) = match validate_plugin_dir(Path::new(dir_str)) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            return 1;
        }
    };
    for w in &warnings {
        println!("[WARN] {w}");
    }
    let sha = match compute_plugin_sha256(&canon) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[FAIL] Failed to hash plugin directory: {e}");
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
        "[OK] Protocol v2: hotkeys={} · events={} · settings={}",
        manifest.hotkeys.len(),
        manifest.events.len(),
        manifest.settings.len()
    );
    println!(
        "[OK] Resource cap: {} MB · idle {}s",
        manifest.resources.max_memory_mb, manifest.resources.idle_timeout_secs
    );
    println!("[OK] Bundle SHA-256: {sha}");

    if let (Some(parent), Some(first_kw)) = (canon.parent(), manifest.triggers.keywords.first()) {
        let host = PluginHost::new(vec![parent.to_path_buf()]);
        host.set_interactive_prompts(false);
        let ctx = QueryContext::new(format!("{first_kw} hello"), 1, 5).with_keyword(first_kw, "hello");
        let items: Vec<_> = host
            .query_keyword(first_kw, &ctx)
            .into_iter()
            .filter(|it| it.provider_id == manifest.id)
            .collect();
        println!("[OK] Live query `{first_kw} hello` returned {} result(s)", items.len());
        if let Some(first) = items.first() {
            println!("     Top hit: `{}` — {}", first.title, first.subtitle);
        }
        host.shutdown_all();
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_and_validate_detects_missing_entry() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_verify_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("tomoyo-plugin.toml"),
            "id=\"dev.test.bad\"\nname=\"Bad\"\nversion=\"0.1.0\"\n[runtime]\nkind=\"webview\"\nentry=\"missing.html\"\n",
        )
        .unwrap();
        let err = validate_plugin_dir(&tmp).unwrap_err();
        assert!(err.contains("missing.html"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
