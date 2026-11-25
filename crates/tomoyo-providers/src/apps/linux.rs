//! Native Linux XDG `.desktop` application indexer, Desktop folder scanner, and localized aliases.

use super::is_noisy_shortcut;
use super::scanner::ScannedApp;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tomoyo_core::AppsConfig;

pub(super) fn linux_app_watch_paths(apps_cfg: &AppsConfig) -> Vec<PathBuf> {
    let mut dirs = xdg_application_dirs();
    if let Some(d) = detect_xdg_desktop_dir() {
        dirs.push(d);
    }
    for raw in &apps_cfg.extra_dirs {
        let p = PathBuf::from(raw.trim());
        if p.is_dir() {
            dirs.push(p);
        }
    }
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

pub(super) fn scan_linux_apps(
    _apps_cfg: &AppsConfig,
    seen: &mut HashSet<String>,
    seen_targets: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
    let mut visited = HashSet::new();
    for app_dir in xdg_application_dirs() {
        scan_desktop_entries_dir(&app_dir, 0, &mut visited, seen, seen_targets, out);
    }
    if let Some(desktop) = detect_xdg_desktop_dir() {
        scan_user_folder(&desktop, 0, 2, "桌面", true, &mut visited, seen, out);
    }
}

fn xdg_application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let xdg_data = std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".local").join("share"));
        dirs.push(xdg_data.join("applications"));
        dirs.push(
            PathBuf::from(&home)
                .join(".local/share/flatpak/exports/share/applications"),
        );
    }
    if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
        for part in data_dirs.split(':').filter(|s| !s.trim().is_empty()) {
            dirs.push(PathBuf::from(part.trim()).join("applications"));
        }
    } else {
        dirs.push(PathBuf::from("/usr/local/share/applications"));
        dirs.push(PathBuf::from("/usr/share/applications"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop/applications"));

    let mut dedup = Vec::new();
    let mut seen = HashSet::new();
    for d in dirs {
        if seen.insert(d.to_string_lossy().into_owned()) {
            dedup.push(d);
        }
    }
    dedup
}

fn detect_xdg_desktop_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let cfg_dir = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join(".config"));
    if let Ok(raw) = std::fs::read_to_string(cfg_dir.join("user-dirs.dirs")) {
        for line in raw.lines() {
            let t = line.trim();
            if let Some(val) = t.strip_prefix("XDG_DESKTOP_DIR=") {
                let unquoted = val.trim().trim_matches('"').replace("$HOME", &home);
                let pb = PathBuf::from(unquoted);
                if pb.is_dir() {
                    return Some(pb);
                }
            }
        }
    }
    for sub in ["Desktop", "桌面"] {
        let p = PathBuf::from(&home).join(sub);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

fn scan_desktop_entries_dir(
    dir: &Path,
    depth: usize,
    visited: &mut HashSet<String>,
    seen: &mut HashSet<String>,
    seen_targets: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
    if depth > 2 || out.len() >= 2500 {
        return;
    }
    let key = dir.to_string_lossy().into_owned();
    if !visited.insert(key) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            scan_desktop_entries_dir(&p, depth + 1, visited, seen, seen_targets, out);
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&p) else {
            continue;
        };
        if let Some(app) = parse_desktop_file(&p, &raw) {
            let name_key = app.name.to_lowercase();
            let path_key = app.path.to_lowercase();
            if seen.contains(&name_key) || !seen_targets.insert(path_key) {
                continue;
            }
            seen.insert(name_key);
            out.push(app);
        }
    }
}

fn parse_desktop_file(path: &Path, raw: &str) -> Option<ScannedApp> {
    let mut in_main = false;
    let mut name = None;
    let mut name_zh = None;
    let mut generic = None;
    let mut exec = None;
    let mut try_exec = None;
    let mut icon = None;
    let mut no_display = false;
    let mut hidden = false;
    let mut entry_type = String::new();

    for line in raw.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "Type" => entry_type = v.to_string(),
            "Name" if name.is_none() => name = Some(v.to_string()),
            "Name[zh_CN]" | "Name[zh]" if name_zh.is_none() => name_zh = Some(v.to_string()),
            "GenericName" | "GenericName[zh_CN]" if generic.is_none() => generic = Some(v.to_string()),
            "Exec" => exec = Some(v.to_string()),
            "TryExec" => try_exec = Some(v.to_string()),
            "Icon" => icon = Some(v.to_string()),
            "NoDisplay" => no_display = v.eq_ignore_ascii_case("true"),
            "Hidden" => hidden = v.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }

    if no_display || hidden || (!entry_type.is_empty() && entry_type != "Application") {
        return None;
    }
    if let Some(te) = try_exec.as_deref()
        && !command_exists(te)
    {
        return None;
    }
    let raw_exec = exec?;
    let bin_hint = raw_exec
        .split_whitespace()
        .find(|tok| !tok.starts_with('%') && !tok.contains('='))
        .unwrap_or("")
        .trim_matches('"')
        .to_string();
    let title = name_zh.clone().or(name.clone())?;
    if is_noisy_shortcut(&title.to_lowercase()) {
        return None;
    }

    let mut aliases = Vec::new();
    if let Some(n) = name
        && n != title
    {
        aliases.push(n);
    }
    if !bin_hint.is_empty()
        && let Some(b) = Path::new(&bin_hint).file_name().and_then(|s| s.to_str())
    {
        aliases.push(b.to_string());
    }
    if let Some(g) = generic {
        aliases.push(g);
    }
    let full_path = path.to_string_lossy().into_owned();
    let subtitle = icon
        .filter(|s| !s.is_empty())
        .map(|ic| format!("应用 · {bin_hint} [{ic}]"))
        .unwrap_or_else(|| format!("应用 · {bin_hint}"));

    Some(ScannedApp {
        id: format!("app:xdg:{}", full_path.to_lowercase()),
        name: title,
        path: full_path,
        launch_args: None,
        subtitle,
        is_uwp: false,
        is_folder: false,
        is_file: false,
        alias_text: (!aliases.is_empty()).then(|| aliases.join(" ")),
    })
}

fn command_exists(cmd: &str) -> bool {
    let p = Path::new(cmd);
    if p.is_absolute() {
        return p.is_file();
    }
    let Ok(path_var) = std::env::var("PATH") else {
        return true;
    };
    std::env::split_paths(&path_var).any(|d| d.join(cmd).is_file())
}

#[allow(clippy::too_many_arguments)]
fn scan_user_folder(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    label_prefix: &'static str,
    include_top_folders: bool,
    visited: &mut HashSet<String>,
    seen: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
    if depth > max_depth || out.len() >= 2500 {
        return;
    }
    if !visited.insert(dir.to_string_lossy().into_owned()) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        let Some(fname) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if fname.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            if include_top_folders && depth == 0 {
                let full = p.to_string_lossy().into_owned();
                let dedup = format!("dir:{}", full.to_lowercase());
                if seen.insert(dedup.clone()) {
                    out.push(ScannedApp {
                        id: dedup,
                        name: fname.to_string(),
                        path: full,
                        launch_args: None,
                        subtitle: format!("{label_prefix} · 文件夹"),
                        is_uwp: false,
                        is_folder: true,
                        is_file: false,
                        alias_text: None,
                    });
                }
            }
            scan_user_folder(&p, depth + 1, max_depth, label_prefix, false, visited, seen, out);
            continue;
        }
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_exec_ext = matches!(ext.as_str(), "desktop" | "appimage" | "sh" | "py");
        let is_exec_perm = is_unix_executable(&p);
        if !is_exec_ext && !is_exec_perm {
            continue;
        }
        if ext == "desktop"
            && let Ok(raw) = std::fs::read_to_string(&p)
            && let Some(app) = parse_desktop_file(&p, &raw)
        {
            if seen.insert(app.name.to_lowercase()) {
                out.push(app);
            }
            continue;
        }
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(fname);
        if seen.insert(stem.to_lowercase()) {
            let full = p.to_string_lossy().into_owned();
            out.push(ScannedApp {
                id: format!("app:{}", full.to_lowercase()),
                name: stem.to_string(),
                path: full,
                launch_args: None,
                subtitle: format!("{label_prefix} · {fname}"),
                is_uwp: false,
                is_folder: false,
                is_file: false,
                alias_text: None,
            });
        }
    }
}

fn is_unix_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}
