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

    #[cfg(target_os = "linux")]
    probe_wayland_portal();

    probe_local_governance(&mut warnings);

    println!("\nSummary: {} plugin(s), {} warning(s), {} failure(s).", plugins.len(), warnings, failures);
    if failures > 0 { 1 } else { 0 }
}

fn dir_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|ent| {
            let p = ent.path();
            if p.is_dir() {
                dir_bytes(&p)
            } else {
                ent.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

fn probe_local_governance(warnings: &mut usize) {
    println!("\n--- Local Governance & Storage Hygiene ---");
    let data = tomoyo_core::data_dir();
    let img_mb = dir_bytes(&data.join("clipboard-images")) as f64 / (1024.0 * 1024.0);
    let logs_kb = dir_bytes(&data.join("logs")) as f64 / 1024.0;
    if img_mb > 64.0 {
        *warnings += 1;
        println!("  [WARN] clipboard-images: {img_mb:.2} MB (exceeds 64 MB budget)");
    } else {
        println!("  [OK]   clipboard-images: {img_mb:.2} MB (<= 64 MB budget) · logs: {logs_kb:.1} KB");
    }
    if Path::new(".githooks").is_dir() {
        let out = std::process::Command::new("git")
            .args(["config", "--get", "core.hooksPath"])
            .output();
        match out {
            Ok(o) if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == ".githooks" => {
                println!("  [OK]   Git anti-bloat hooks active (core.hooksPath=.githooks)");
            }
            _ => {
                *warnings += 1;
                println!("  [WARN] Git hooks inactive — run `node scripts/setup-governance.mjs --install-timer`");
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn probe_wayland_portal() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"));
    if !wayland {
        return;
    }
    println!("\n--- Wayland GlobalShortcuts Portal ---");
    let out = std::process::Command::new("gdbus")
        .args([
            "call", "--session",
            "--dest", "org.freedesktop.portal.Desktop",
            "--object-path", "/org/freedesktop/portal/desktop",
            "--method", "org.freedesktop.DBus.Properties.Get",
            "org.freedesktop.portal.GlobalShortcuts", "version",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let ver = String::from_utf8_lossy(&o.stdout).trim().to_string();
            println!("  [OK]   org.freedesktop.portal.GlobalShortcuts ready ({ver})");
        }
        _ => {
            println!("  [INFO] GlobalShortcuts portal inactive (Tomoyo falls back to KGlobalAccel / XWayland)");
        }
    }
}
