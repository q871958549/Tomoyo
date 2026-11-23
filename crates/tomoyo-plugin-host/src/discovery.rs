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

/// Resolves a safe local image path (`icon.png` / `icon.ico` / `manifest.icon`) inside `plugin_dir`.
pub fn resolve_plugin_icon_path(manifest: &PluginManifest, dir: &Path) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(raw) = manifest.icon.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        candidates.push(raw.to_string());
    }
    candidates.extend(["icon.png".into(), "logo.png".into(), "icon.ico".into()]);
    for rel in candidates {
        if rel.contains("..") {
            continue;
        }
        let path = dir.join(&rel);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
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
    let icon_path = resolve_plugin_icon_path(manifest, dir);
    let icon = icon_path
        .as_ref()
        .map(|p| IconSpec::ImageFile(p.clone()))
        .unwrap_or_else(|| IconSpec::Monogram("WV".into()));
    let default_mode = manifest.display_mode.as_str().to_string();
    let initial_query = ctx.args.trim().to_string();
    let max_memory_mb = manifest.resources.max_memory_mb.max(256);

    let (primary_mode, alt_mode, alt_label) = match default_mode.as_str() {
        "inline" => (
            Some("inline".to_string()),
            "window",
            format!("在独立窗口中打开 {} (Window)", manifest.name),
        ),
        "window" => (
            Some("window".to_string()),
            "inline",
            format!("在搜索结果预览区嵌入 {} (Inline)", manifest.name),
        ),
        _ => (
            None,
            "inline",
            format!("在搜索结果预览区嵌入 {} (Inline)", manifest.name),
        ),
    };

    ResultItem {
        id: format!("webview:{}", manifest.id),
        provider_id: manifest.id.clone(),
        title: format!(
            "打开 {} {}",
            manifest.name,
            if initial_query.is_empty() {
                String::new()
            } else {
                format!("— {initial_query}")
            }
        )
        .trim()
        .to_string(),
        subtitle: format!(
            "{} · 按需隔离 WebView（退出即释放内存）",
            manifest.description
        ),
        category: ResultCategory::Plugin,
        score: 9_600,
        icon,
        badge: Some(compat_tag.into()),
        copy_text: None,
        actions: vec![
            ActionSpec {
                id: "open_webview".into(),
                label: format!("打开 {} ({})", manifest.name, compat_tag),
                shortcut: Some("Enter".into()),
                kind: ActionKind::OpenWebView {
                    plugin_id: manifest.id.clone(),
                    title: manifest.name.clone(),
                    entry_path: entry_path.clone(),
                    utools_compat,
                    initial_query: initial_query.clone(),
                    width,
                    height,
                    max_memory_mb,
                    icon_path: icon_path.clone(),
                    display_mode: primary_mode,
                },
            },
            ActionSpec {
                id: format!("open_webview_{alt_mode}"),
                label: alt_label,
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::OpenWebView {
                    plugin_id: manifest.id.clone(),
                    title: manifest.name.clone(),
                    entry_path,
                    utools_compat,
                    initial_query,
                    width,
                    height,
                    max_memory_mb,
                    icon_path,
                    display_mode: Some(alt_mode.into()),
                },
            },
        ],
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
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent() {
            let p = dir.join("plugins");
            if p.is_dir() && !dirs.contains(&p) {
                dirs.push(p);
            }
            #[cfg(target_os = "linux")]
            if let Some(prefix) = dir.parent() {
                let share_p = prefix.join("share").join("tomoyo").join("plugins");
                if share_p.is_dir() && !dirs.contains(&share_p) {
                    dirs.push(share_p);
                }
            }
        }
    let user_dir = tomoyo_core::data_dir().join("plugins");
    if !dirs.contains(&user_dir) {
        dirs.push(user_dir);
    }
    #[cfg(target_os = "linux")]
    {
        let sys_dir = PathBuf::from("/usr/share/tomoyo/plugins");
        if sys_dir.is_dir() && !dirs.contains(&sys_dir) {
            dirs.push(sys_dir);
        }
    }
    dirs
}
