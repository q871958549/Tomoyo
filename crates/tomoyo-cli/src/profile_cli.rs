//! Declarative cross-machine personal profile export/diff/import (`tomoyo-cli profile`).

use crate::flag_val;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use tomoyo_core::{data_dir, format_beijing_now, sync_root_dir, TomoyoConfig, CURRENT_VERSION};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfilePluginRef {
    pub id: String,
    pub version: String,
    pub runtime_badge: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomoyoProfileBundle {
    pub schema_version: u32,
    pub tomoyo_version: String,
    pub exported_at: String,
    pub config: TomoyoConfig,
    #[serde(default)]
    pub snippets_toml: Option<String>,
    #[serde(default)]
    pub flows_toml: Option<String>,
    #[serde(default)]
    pub mcp_toml: Option<String>,
    #[serde(default)]
    pub installed_plugins: Vec<ProfilePluginRef>,
}

fn read_opt_text(path: &Path) -> Option<String> {
    path.is_file().then(|| std::fs::read_to_string(path).ok()).flatten()
}

pub fn collect_profile_bundle(
    sync_root: &Path,
    local_data_dir: &Path,
    cfg: TomoyoConfig,
    installed_plugins: Vec<ProfilePluginRef>,
) -> TomoyoProfileBundle {
    TomoyoProfileBundle {
        schema_version: 1,
        tomoyo_version: CURRENT_VERSION.to_string(),
        exported_at: format_beijing_now(),
        config: cfg,
        snippets_toml: read_opt_text(&sync_root.join("snippets.toml")),
        flows_toml: read_opt_text(&sync_root.join("flows.toml")),
        mcp_toml: read_opt_text(&local_data_dir.join("mcp.toml")),
        installed_plugins,
    }
}

pub fn diff_profile_bundles(local: &TomoyoProfileBundle, incoming: &TomoyoProfileBundle) -> Vec<String> {
    let mut diffs = Vec::new();
    if local.config.theme != incoming.config.theme {
        diffs.push(format!("theme: `{}` -> `{}`", local.config.theme, incoming.config.theme));
    }
    if local.config.language != incoming.config.language {
        diffs.push(format!("language: `{}` -> `{}`", local.config.language, incoming.config.language));
    }
    if local.config.hotkey != incoming.config.hotkey {
        diffs.push(format!("hotkey: `{}` -> `{}`", local.config.hotkey, incoming.config.hotkey));
    }
    if local.snippets_toml != incoming.snippets_toml && incoming.snippets_toml.is_some() {
        diffs.push("snippets.toml: differs from local".into());
    }
    if local.flows_toml != incoming.flows_toml && incoming.flows_toml.is_some() {
        diffs.push("flows.toml: differs from local".into());
    }
    if local.mcp_toml != incoming.mcp_toml && incoming.mcp_toml.is_some() {
        diffs.push("mcp.toml: differs from local".into());
    }
    let local_ids: BTreeSet<&str> = local.installed_plugins.iter().map(|p| p.id.as_str()).collect();
    for p in &incoming.installed_plugins {
        if !local_ids.contains(p.id.as_str()) {
            diffs.push(format!("missing plugin locally: {} (v{})", p.id, p.version));
        }
    }
    diffs
}

pub fn apply_profile_bundle(
    sync_root: &Path,
    local_data_dir: &Path,
    bundle: &TomoyoProfileBundle,
    dry_run: bool,
) -> Result<Vec<String>, String> {
    if bundle.schema_version != 1 {
        return Err(format!("Unsupported profile schema_version={}", bundle.schema_version));
    }
    let mut touched = Vec::new();
    let cfg_path = sync_root.join("config.toml");
    let cfg_toml = toml::to_string_pretty(&bundle.config).map_err(|e| e.to_string())?;
    touched.push(format!("config.toml -> {}", cfg_path.display()));
    if !dry_run {
        std::fs::create_dir_all(sync_root).map_err(|e| e.to_string())?;
        std::fs::write(&cfg_path, cfg_toml).map_err(|e| e.to_string())?;
    }
    for (name, opt, dir) in [
        ("snippets.toml", &bundle.snippets_toml, sync_root),
        ("flows.toml", &bundle.flows_toml, sync_root),
        ("mcp.toml", &bundle.mcp_toml, local_data_dir),
    ] {
        if let Some(raw) = opt {
            let dst = dir.join(name);
            touched.push(format!("{name} -> {}", dst.display()));
            if !dry_run {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                std::fs::write(&dst, raw).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(touched)
}

fn current_local_bundle() -> TomoyoProfileBundle {
    let cfg = TomoyoConfig::load_or_create();
    let disabled: BTreeSet<String> = cfg.disabled_plugins.iter().map(|s| s.to_ascii_lowercase()).collect();
    let host = PluginHost::new(default_plugin_dirs());
    let plugins: Vec<ProfilePluginRef> = host
        .list_plugins()
        .into_iter()
        .map(|p| ProfilePluginRef {
            enabled: !disabled.contains(&p.id.to_ascii_lowercase()),
            id: p.id,
            version: p.version,
            runtime_badge: p.runtime_badge,
        })
        .collect();
    host.shutdown_all();
    collect_profile_bundle(&sync_root_dir(), &data_dir(), cfg, plugins)
}

pub(crate) fn cmd_profile(args: &[String]) -> i32 {
    let Some(sub) = args.first().map(String::as_str) else {
        eprintln!("Usage:\n  tomoyo-cli profile export --out <file.json>\n  tomoyo-cli profile diff --in <file.json>\n  tomoyo-cli profile import --in <file.json> [--dry-run]");
        return 1;
    };
    match sub {
        "export" => {
            let Some(out) = flag_val(&args[1..], "--out") else {
                eprintln!("Missing --out <file.json>");
                return 1;
            };
            let bundle = current_local_bundle();
            let json = serde_json::to_string_pretty(&bundle).unwrap();
            if let Err(e) = std::fs::write(out, format!("{json}\n")) {
                eprintln!("Failed to write `{out}`: {e}");
                return 1;
            }
            println!(
                "[OK] Exported Tomoyo profile -> {out} ({} plugins; run `tomoyo-cli secrets export` separately for encrypted API keys)",
                bundle.installed_plugins.len()
            );
            0
        }
        "diff" | "import" => {
            let Some(inp) = flag_val(&args[1..], "--in") else {
                eprintln!("Missing --in <file.json>");
                return 1;
            };
            let Ok(raw) = std::fs::read_to_string(inp) else {
                eprintln!("Failed to read `{inp}`");
                return 1;
            };
            let incoming: TomoyoProfileBundle = match serde_json::from_str(&raw) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Invalid profile JSON: {e}");
                    return 1;
                }
            };
            let local = current_local_bundle();
            let diffs = diff_profile_bundles(&local, &incoming);
            if sub == "diff" {
                println!("Profile diff ({} change(s)):", diffs.len());
                for d in &diffs {
                    println!("  - {d}");
                }
                return 0;
            }
            let dry_run = args.iter().any(|a| a == "--dry-run");
            match apply_profile_bundle(&sync_root_dir(), &data_dir(), &incoming, dry_run) {
                Ok(applied) => {
                    println!(
                        "[OK] {} {} file(s); {} diff item(s).",
                        if dry_run { "Dry-run would update" } else { "Applied" },
                        applied.len(),
                        diffs.len()
                    );
                    for line in applied {
                        println!("  - {line}");
                    }
                    let local_ids: BTreeSet<&str> = local.installed_plugins.iter().map(|p| p.id.as_str()).collect();
                    for p in &incoming.installed_plugins {
                        if !local_ids.contains(p.id.as_str()) {
                            println!("  [HINT] Install missing plugin via: tomoyo-cli install {}", p.id);
                        }
                    }
                    0
                }
                Err(e) => {
                    eprintln!("[FAIL] profile import failed: {e}");
                    1
                }
            }
        }
        other => {
            eprintln!("Unknown profile subcommand `{other}`");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_export_diff_dry_run_and_apply_roundtrip() {
        let base = std::env::temp_dir().join(format!("tomoyo-prof-test-{}", std::process::id()));
        let sync_a = base.join("sync_a");
        let data_a = base.join("data_a");
        let sync_b = base.join("sync_b");
        let data_b = base.join("data_b");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&sync_a).unwrap();
        std::fs::create_dir_all(&data_a).unwrap();
        std::fs::write(sync_a.join("snippets.toml"), "[[snippets]]\nkeyword=\"hi\"\n").unwrap();
        std::fs::write(data_a.join("mcp.toml"), "[[servers]]\nname=\"demo\"\ncommand=\"echo\"\n").unwrap();

        let cfg_a = TomoyoConfig {
            theme: "midnight".into(),
            ..Default::default()
        };
        let plugins_a = vec![ProfilePluginRef {
            id: "dev.tomoyo.ai".into(),
            version: "3.5.0".into(),
            runtime_badge: "STDIO".into(),
            enabled: true,
        }];
        let bundle_a = collect_profile_bundle(&sync_a, &data_a, cfg_a, plugins_a);
        let json = serde_json::to_string(&bundle_a).unwrap();
        let parsed: TomoyoProfileBundle = serde_json::from_str(&json).unwrap();

        let bundle_b = collect_profile_bundle(&sync_b, &data_b, TomoyoConfig::default(), vec![]);
        let diffs = diff_profile_bundles(&bundle_b, &parsed);
        assert!(diffs.iter().any(|d| d.contains("midnight")));
        assert!(diffs.iter().any(|d| d.contains("dev.tomoyo.ai")));

        let dry = apply_profile_bundle(&sync_b, &data_b, &parsed, true).unwrap();
        assert!(!dry.is_empty());
        assert!(!sync_b.join("config.toml").exists());

        apply_profile_bundle(&sync_b, &data_b, &parsed, false).unwrap();
        assert!(sync_b.join("config.toml").is_file());
        assert!(sync_b.join("snippets.toml").is_file());
        assert!(data_b.join("mcp.toml").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }
}
