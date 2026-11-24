//! Environment, plugin integrity & multi-registry mirror diagnostics (`tomoyo-cli doctor`).

use std::path::Path;
use std::time::Instant;
use tomoyo_core::{TomoyoConfig, CURRENT_VERSION};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};

pub(crate) fn cmd_doctor(_args: &[String]) -> i32 {
    println!("=== Tomoyo Ecosystem Doctor (v{CURRENT_VERSION}) ===");
    let mut warnings = 0usize;
    let mut failures = 0usize;

    let cfg_path = TomoyoConfig::config_path();
    let cfg = TomoyoConfig::load_or_create();
    println!("[OK]   Config: {} (theme={}, lang={})", cfg_path.display(), cfg.theme, cfg.language);

    let dirs = default_plugin_dirs();
    let existing_dirs: Vec<String> = dirs
        .iter()
        .filter(|d| d.is_dir())
        .map(|d| d.display().to_string())
        .collect();
    println!("[OK]   Plugin dirs ({}): {}", existing_dirs.len(), existing_dirs.join(" | "));

    let host = PluginHost::new(dirs);
    let plugins = host.list_plugins();
    println!("\n--- Installed Plugins ({}) ---", plugins.len());
    for p in &plugins {
        let has_icon = p.icon_path.as_deref().is_some_and(|ip| Path::new(ip).is_file());
        let perm_note = if p.declared_permissions.is_empty() {
            "0 perms".to_string()
        } else {
            format!("{}/{} perms granted", p.granted_permissions.len(), p.declared_permissions.len())
        };
        if !has_icon {
            warnings += 1;
            println!("  [WARN] {:<28} v{} [{}] ({perm_note}) · missing dedicated icon", p.id, p.version, p.runtime_badge);
        } else {
            println!("  [OK]   {:<28} v{} [{}] ({perm_note})", p.id, p.version, p.runtime_badge);
        }
    }
    host.shutdown_all();

    let sources: Vec<String> = if cfg.plugin_registries.is_empty() {
        vec!["registry/index.json".into()]
    } else {
        cfg.plugin_registries.clone()
    };
    println!("\n--- Registry Mirrors ({}) ---", sources.len());
    for src in &sources {
        let started = Instant::now();
        if src.starts_with("http://") || src.starts_with("https://") {
            match tomoyo_core::http::http_head_redirect(src, 4000) {
                Ok(r) if r.status_code < 400 => {
                    println!("  [OK]   {src} -> HTTP {} ({} ms)", r.status_code, started.elapsed().as_millis());
                }
                Ok(r) => {
                    warnings += 1;
                    println!("  [WARN] {src} -> HTTP {} ({} ms)", r.status_code, started.elapsed().as_millis());
                }
                Err(e) => {
                    warnings += 1;
                    println!("  [WARN] {src} -> unreachable ({e})");
                }
            }
        } else {
            let ok = Path::new(src).is_file() || tomoyo_core::data_dir().join(src).is_file();
            if ok {
                println!("  [OK]   {src} (local index ready, {} ms)", started.elapsed().as_millis());
            } else {
                failures += 1;
                println!("  [FAIL] {src} (local index file not found)");
            }
        }
    }

    println!("\nSummary: {} plugin(s), {} warning(s), {} failure(s).", plugins.len(), warnings, failures);
    if failures > 0 { 1 } else { 0 }
}
