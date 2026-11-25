//! Breadth-first deep custom-directory indexer (up to 12 levels) for apps, folders & documents.
//!
//! Ensures deeply nested files & portable tools inside user-configured `extra_dirs` are always
//! searchable (with Pinyin, relative-path alias matching, file-type filters, and preview) even
//! when nested 5–12 folders deep or when Everything is unavailable, while pruning build/cache noise.

use super::scanner::ScannedApp;
use super::AppEntry;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use tomoyo_core::{AppsConfig, Lang, MAX_SCAN_DEPTH};
use tomoyo_protocol::{
    extension_short_tag, ActionKind, ActionSpec, IconSpec, PreviewSpec, ResultCategory, ResultItem,
    SearchFilter,
};

const MAX_CUSTOM_ENTRIES: usize = 6_000;
const MAX_VISITED_DIRS: usize = 4_000;

/// Breadth-first scans user `apps.extra_dirs` up to `extra_scan_depth` (`1..=12`), indexing
/// portable executables/scripts, nested folders, and regular user documents/files cleanly.
pub(super) fn scan_custom_extra_dirs(
    apps_cfg: &AppsConfig,
    seen_targets: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
    let max_depth = apps_cfg.extra_scan_depth.clamp(1, MAX_SCAN_DEPTH);
    let mut visited_dirs: HashSet<String> = HashSet::new();
    let mut added = 0usize;

    for raw_dir in &apps_cfg.extra_dirs {
        let trimmed = raw_dir.trim();
        if trimmed.is_empty() {
            continue;
        }
        let root = PathBuf::from(trimmed);
        if !root.is_dir() {
            continue;
        }
        let root_label = root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(trimmed)
            .to_string();

        let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
        queue.push_back((root.clone(), 0));

        while let Some((dir, depth)) = queue.pop_front() {
            if depth > max_depth || added >= MAX_CUSTOM_ENTRIES || visited_dirs.len() >= MAX_VISITED_DIRS {
                break;
            }
            let dir_key = dir.to_string_lossy().to_lowercase();
            if !visited_dirs.insert(dir_key) {
                continue;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };

            let rel_parent = dir
                .strip_prefix(&root)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();

            for entry in rd.flatten() {
                if added >= MAX_CUSTOM_ENTRIES {
                    break;
                }
                let path = entry.path();
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_symlink() {
                    continue;
                }
                let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if file_name.starts_with('.') || file_name.starts_with('$') || file_name.starts_with("~$") {
                    continue;
                }

                if ft.is_dir() {
                    let lower = file_name.to_ascii_lowercase();
                    if is_noisy_dir(&lower) {
                        continue;
                    }
                    let full = path.to_string_lossy().into_owned();
                    let full_low = full.to_lowercase();
                    if depth < max_depth && seen_targets.insert(format!("dir:{full_low}")) {
                        let sub = if rel_parent.is_empty() {
                            format!("目录 · {root_label}")
                        } else {
                            format!("目录 · {root_label}/{rel_parent}")
                        };
                        out.push(ScannedApp {
                            id: format!("dir:{full_low}"),
                            name: file_name.to_string(),
                            path: full,
                            launch_args: None,
                            subtitle: sub,
                            is_uwp: false,
                            is_folder: true,
                            is_file: false,
                            alias_text: (!rel_parent.is_empty()).then(|| rel_parent.clone()),
                        });
                        added += 1;
                    }
                    if depth < max_depth {
                        queue.push_back((path, depth + 1));
                    }
                    continue;
                }

                if !ft.is_file() {
                    continue;
                }
                let ext_low = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if is_noisy_file_ext(&ext_low) {
                    continue;
                }

                let full = path.to_string_lossy().into_owned();
                let full_low = full.to_lowercase();
                if !seen_targets.insert(full_low.clone()) {
                    continue;
                }

                let is_exec = matches!(
                    ext_low.as_str(),
                    "exe" | "lnk" | "appref-ms" | "bat" | "cmd" | "ps1" | "msc" | "url"
                );
                if is_exec {
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
                    if ext_low == "exe" && is_noisy_helper_exe(&stem.to_ascii_lowercase()) {
                        continue;
                    }
                    let sub = if rel_parent.is_empty() {
                        format!("自定义目录 · {root_label}")
                    } else {
                        format!("自定义目录 · {root_label}/{rel_parent}")
                    };
                    out.push(ScannedApp {
                        id: format!("app:{full_low}"),
                        name: stem.to_string(),
                        path: full,
                        launch_args: None,
                        subtitle: sub,
                        is_uwp: false,
                        is_folder: false,
                        is_file: false,
                        alias_text: (!rel_parent.is_empty()).then(|| rel_parent.clone()),
                    });
                } else {
                    let sub = if rel_parent.is_empty() {
                        format!("{root_label} · {full}")
                    } else {
                        format!("{root_label}/{rel_parent} · {full}")
                    };
                    out.push(ScannedApp {
                        id: format!("file:{full_low}"),
                        name: file_name.to_string(),
                        path: full,
                        launch_args: None,
                        subtitle: sub,
                        is_uwp: false,
                        is_folder: false,
                        is_file: true,
                        alias_text: (!rel_parent.is_empty()).then(|| rel_parent.clone()),
                    });
                }
                added += 1;
            }
        }
    }
}

/// Checks whether an `AppEntry` (app, folder, or deep custom file) matches the active search filter.
pub(super) fn entry_matches_filter(
    app: &AppEntry,
    filter: SearchFilter,
    custom_exts: Option<&[String]>,
) -> bool {
    match filter {
        SearchFilter::All => true,
        SearchFilter::App => !app.is_folder && !app.is_file,
        SearchFilter::Folder => app.is_folder,
        SearchFilter::Custom(_) => {
            if !app.is_file {
                return false;
            }
            let ext = file_ext_lower(&app.path);
            custom_exts.is_some_and(|exts| exts.iter().any(|e| e.eq_ignore_ascii_case(&ext)))
        }
        other => {
            if !app.is_file {
                return false;
            }
            let ext = file_ext_lower(&app.path);
            other
                .builtin_extensions()
                .iter()
                .any(|e| e.eq_ignore_ascii_case(&ext))
        }
    }
}

/// Builds a first-class `ResultCategory::File` result item for a deep custom-directory file.
pub(super) fn custom_file_to_result_item(app: &AppEntry, score: i64, lang: Lang) -> ResultItem {
    let ext = file_ext_lower(&app.path);
    let tag = extension_short_tag(&ext);
    let badge = if tag != "FILE" {
        Some(tag.to_string())
    } else if !ext.is_empty() && ext.len() <= 5 {
        Some(ext.to_ascii_uppercase())
    } else {
        Some("FILE".to_string())
    };
    let parent = Path::new(&app.path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let preview = Some(PreviewSpec::FilePreview { path: app.path.clone() });
    let mut actions = vec![
        ActionSpec {
            id: "open".into(),
            label: lang.pick("打开文件", "Open File").into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::LaunchPath {
                path: app.path.clone(),
                args: None,
                working_dir: (!parent.is_empty()).then(|| parent.clone()),
            },
        },
        ActionSpec {
            id: "reveal".into(),
            label: lang.pick("在资源管理器中定位", "Reveal in Explorer").into(),
            shortcut: Some("Ctrl+Enter".into()),
            kind: ActionKind::RevealInExplorer { path: app.path.clone() },
        },
        ActionSpec {
            id: "copy_path".into(),
            label: lang.pick("复制完整路径", "Copy Full Path").into(),
            shortcut: Some("Ctrl+C".into()),
            kind: ActionKind::CopyText { text: app.path.clone(), paste_immediately: false },
        },
    ];
    if !parent.is_empty() {
        actions.push(ActionSpec {
            id: "terminal".into(),
            label: lang.pick("在所在目录打开终端", "Open Terminal Here").into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::OpenTerminalAt { dir: parent },
        });
    }
    ResultItem {
        id: app.id.clone(),
        provider_id: "builtin.apps".into(),
        title: app.name.clone(),
        subtitle: app.subtitle.clone(),
        category: ResultCategory::File,
        score,
        icon: IconSpec::ShellPath(app.path.clone()),
        badge,
        copy_text: Some(app.path.clone()),
        actions,
        preview,
    }
}

fn file_ext_lower(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_noisy_dir(lower: &str) -> bool {
    matches!(
        lower,
        "node_modules"
            | ".git"
            | ".hg"
            | ".svn"
            | "__pycache__"
            | ".venv"
            | "venv"
            | "target"
            | ".next"
            | ".nuxt"
            | "dist"
            | "build"
            | "obj"
            | ".idea"
            | ".vscode"
            | ".cache"
            | "cache"
            | "temp"
            | "tmp"
            | "logs"
            | "$recycle.bin"
            | "system volume information"
            | "_uninstalled"
            | "locales"
            | "vcredist"
            | "redist"
    )
}

fn is_noisy_file_ext(ext: &str) -> bool {
    matches!(
        ext,
        "dll" | "pdb" | "obj" | "o" | "lib" | "a" | "pyc" | "pyo" | "class" | "tmp" | "bak"
            | "log" | "lock" | "dat" | "pak" | "dmp" | "etl" | "idx" | "pack" | "map" | "ilk"
            | "exp" | "tlog" | "pch" | "suo" | "user" | "sys" | "mui" | "cat"
    )
}

fn is_noisy_helper_exe(lower: &str) -> bool {
    const PREFIXES: [&str; 10] = [
        "unins", "setup", "install", "update", "crash", "vc_redist", "elevate", "helper",
        "reporter", "webview2",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p)) || lower.ends_with("_helper")
}

/// Word-boundary aware Start Menu noise filter: skips app uninstallers/docs/website links
/// without falsely dropping real utilities like `HiBit Uninstaller`, `Geek Uninstaller`,
/// `HelpDesk`, or `License Manager`.
pub(crate) fn is_noisy_shortcut(lower: &str) -> bool {
    let s = lower.trim();
    if s.contains("卸载") || s.contains("访问主页") || s.contains("release notes") || s.contains("readme") {
        return true;
    }
    let words: Vec<&str> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.iter().any(|&w| matches!(w, "uninstall" | "documentation")) {
        return true;
    }
    matches!(
        words.last().copied(),
        Some("help" | "manual" | "manuals" | "documentation" | "website" | "license")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn noise_filter_keeps_real_uninstaller_apps_and_drops_uninstall_links() {
        assert!(!is_noisy_shortcut("hibit uninstaller"));
        assert!(!is_noisy_shortcut("geek uninstaller"));
        assert!(!is_noisy_shortcut("revo uninstaller pro"));
        assert!(!is_noisy_shortcut("bcuninstaller"));
        assert!(!is_noisy_shortcut("helpdesk client"));
        assert!(!is_noisy_shortcut("license manager"));

        assert!(is_noisy_shortcut("uninstall node.js"));
        assert!(is_noisy_shortcut("xyplorer uninstall"));
        assert!(is_noisy_shortcut("卸载hibit uninstaller"));
        assert!(is_noisy_shortcut("卸载微信"));
        assert!(is_noisy_shortcut("7-zip help"));
        assert!(is_noisy_shortcut("git release notes"));
        assert!(is_noisy_shortcut("node.js documentation"));
    }

    #[test]
    fn bfs_scans_deep_nested_files_up_to_depth_ten_and_skips_noise_dirs() {
        let base = std::env::temp_dir().join(format!("tomoyo_deep_scan_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let deep = base.join("a").join("b").join("c").join("d").join("e").join("f").join("g");
        fs::create_dir_all(&deep).unwrap();
        let target_md = deep.join("禾晗IT运维手册.md");
        fs::write(&target_md, "# 运维手册").unwrap();
        fs::write(deep.join("ignored.dll"), b"bin").unwrap();

        let nm = base.join("node_modules").join("pkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("noise.md"), "# noise").unwrap();

        let cfg = AppsConfig {
            extra_dirs: vec![base.to_string_lossy().into_owned()],
            auto_detect_desktop_organizers: false,
            extra_scan_depth: 10,
        };
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        scan_custom_extra_dirs(&cfg, &mut seen, &mut out);

        assert!(out.iter().any(|e| e.name == "禾晗IT运维手册.md" && e.is_file));
        assert!(!out.iter().any(|e| e.name == "ignored.dll"));
        assert!(!out.iter().any(|e| e.name == "noise.md"));
        let _ = fs::remove_dir_all(&base);
    }
}
