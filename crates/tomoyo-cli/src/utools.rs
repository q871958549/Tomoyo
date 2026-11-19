//! One-click uTools `plugin.json` -> `tomoyo-plugin.toml` migration importer.

use crate::hash::compute_plugin_sha256;
use std::path::Path;
use tomoyo_protocol::PluginManifest;

pub fn cmd_import_utools(args: &[String]) -> i32 {
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
                    let cand = cmd
                        .as_str()
                        .or_else(|| cmd.get("label").and_then(|v| v.as_str()));
                    if let Some(s) = cand {
                        let clean = s.trim().to_string();
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
