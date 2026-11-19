//! Plugin manifest discovery, directory scanning, and zero-RAM WebView trigger builder.

use std::path::{Path, PathBuf};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, PluginManifest, PreviewSpec, QueryContext, ResultCategory,
    ResultItem,
};

pub struct DiscoveredManifest {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
}

/// Scans configured plugin roots for valid `tomoyo-plugin.toml` manifests.
pub fn scan_plugin_dirs(search_dirs: &[PathBuf]) -> Vec<DiscoveredManifest> {
    let mut found = Vec::new();
    for root in search_dirs {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("tomoyo-plugin.toml");
            if !manifest_path.is_file() {
                continue;
            }
            match std::fs::read_to_string(&manifest_path)
                .map_err(|e| e.to_string())
                .and_then(|raw| PluginManifest::from_toml_str(&raw).map_err(|e| e.to_string()))
            {
                Ok(manifest) => {
                    tracing::info!(
                        "Discovered [{}] plugin `{}` ({}) from {}",
                        manifest.runtime.kind_badge(),
                        manifest.name,
                        manifest.id,
                        path.display()
                    );
                    found.push(DiscoveredManifest {
                        manifest,
                        dir: path,
                    });
                }
                Err(err) => {
                    tracing::warn!(
                        "Skipping invalid plugin manifest at {}: {err}",
                        manifest_path.display()
                    );
                }
            }
        }
    }
    found
}

/// Builds a lightweight trigger result for a WebView plugin (0 bytes of WebView2 RAM until Enter).
pub fn build_webview_trigger_item(
    manifest: &PluginManifest,
    dir: &Path,
    entry: &str,
    utools_compat: bool,
    width: u32,
    height: u32,
    ctx: &QueryContext,
) -> ResultItem {
    let entry_path = dir.join(entry).to_string_lossy().into_owned();
    let compat_tag = if utools_compat {
        "uTools Bridge"
    } else {
        "WebView"
    };
    ResultItem {
        id: format!("webview:{}", manifest.id),
        provider_id: manifest.id.clone(),
        title: format!(
            "Open {} {}",
            manifest.name,
            if ctx.args.trim().is_empty() {
                String::new()
            } else {
                format!("— {}", ctx.args.trim())
            }
        )
        .trim()
        .to_string(),
        subtitle: format!(
            "{} · Ephemeral WebView Host (0 idle RAM, killed on close)",
            manifest.description
        ),
        category: ResultCategory::Plugin,
        score: 9_600,
        icon: IconSpec::Monogram("WV".into()),
        badge: Some(compat_tag.into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "open_webview".into(),
            label: format!("Launch {} (Isolated WebView)", manifest.name),
            shortcut: Some("Enter".into()),
            kind: ActionKind::OpenWebView {
                plugin_id: manifest.id.clone(),
                title: manifest.name.clone(),
                entry_path,
                utools_compat,
                initial_query: ctx.args.trim().to_string(),
                width,
                height,
                max_memory_mb: manifest.resources.max_memory_mb.max(256),
            },
        }],
        preview: Some(PreviewSpec::KeyValue {
            title: manifest.name.clone(),
            entries: vec![
                ("Runtime".into(), "Ephemeral WebView2 Host".into()),
                ("Background RAM".into(), "0 MB (killed immediately on close)".into()),
                (
                    "uTools Compat".into(),
                    if utools_compat {
                        "Enabled (window.utools)"
                    } else {
                        "Disabled"
                    }
                    .into(),
                ),
                ("Window Size".into(), format!("{width}x{height}")),
                ("Description".into(), manifest.description.clone()),
            ],
        }),
    }
}

/// Default plugin search roots: `./plugins`, `<exe_dir>/plugins`, and `%LOCALAPPDATA%\Tomoyo\plugins`.
pub fn default_plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        let p = cwd.join("plugins");
        if p.is_dir() {
            dirs.push(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("plugins");
            if p.is_dir() && !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    }
    let user_dir = tomoyo_core::data_dir().join("plugins");
    if !dirs.contains(&user_dir) {
        dirs.push(user_dir);
    }
    dirs
}
