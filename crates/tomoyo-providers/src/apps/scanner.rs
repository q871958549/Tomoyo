use super::is_noisy_shortcut;
use super::organizers::detect_desktop_organizer_dirs;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tomoyo_core::AppsConfig;

#[derive(Debug, Clone)]
pub(super) struct ScannedApp {
    pub id: String,
    pub name: String,
    pub path: String,
    pub launch_args: Option<String>,
    pub subtitle: String,
    pub is_uwp: bool,
    pub is_folder: bool,
    pub alias_text: Option<String>,
}

struct ScanRoot {
    path: PathBuf,
    max_depth: usize,
    allow_exe: bool,
    allow_scripts: bool,
    include_top_folders: bool,
    filter_noise: bool,
    label_prefix: &'static str,
}

fn collect_dir_roots(apps_cfg: &AppsConfig) -> Vec<ScanRoot> {
    let mut roots: Vec<ScanRoot> = Vec::new();

    if let Some(desktop) = known_desktop_dir() {
        roots.push(ScanRoot {
            path: desktop,
            max_depth: 3,
            allow_exe: true,
            allow_scripts: true,
            include_top_folders: true,
            filter_noise: false,
            label_prefix: "桌面",
        });
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let fallback_desk = PathBuf::from(&userprofile).join("Desktop");
        if !roots.iter().any(|r| r.path == fallback_desk) {
            roots.push(ScanRoot {
                path: fallback_desk,
                max_depth: 3,
                allow_exe: true,
                allow_scripts: true,
                include_top_folders: true,
                filter_noise: false,
                label_prefix: "桌面",
            });
        }
    }
    if let Ok(public_dir) = std::env::var("PUBLIC") {
        roots.push(ScanRoot {
            path: PathBuf::from(public_dir).join("Desktop"),
            max_depth: 3,
            allow_exe: true,
            allow_scripts: true,
            include_top_folders: true,
            filter_noise: false,
            label_prefix: "公共桌面",
        });
    }

    if apps_cfg.auto_detect_desktop_organizers {
        for org_dir in detect_desktop_organizer_dirs() {
            roots.push(ScanRoot {
                path: org_dir,
                max_depth: 3,
                allow_exe: true,
                allow_scripts: true,
                include_top_folders: true,
                filter_noise: false,
                label_prefix: "桌面整理",
            });
        }
    }

    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(ScanRoot {
            path: PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu"),
            max_depth: 5,
            allow_exe: true,
            allow_scripts: false,
            include_top_folders: false,
            filter_noise: true,
            label_prefix: "应用",
        });
    }
    if let Ok(progdata) = std::env::var("ProgramData") {
        roots.push(ScanRoot {
            path: PathBuf::from(progdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu"),
            max_depth: 5,
            allow_exe: true,
            allow_scripts: false,
            include_top_folders: false,
            filter_noise: true,
            label_prefix: "应用",
        });
    }

    let custom_depth = apps_cfg.extra_scan_depth.clamp(1, 4);
    for raw_dir in &apps_cfg.extra_dirs {
        let trimmed = raw_dir.trim();
        if trimmed.is_empty() {
            continue;
        }
        let pb = PathBuf::from(trimmed);
        if pb.is_dir() {
            roots.push(ScanRoot {
                path: pb,
                max_depth: custom_depth,
                allow_exe: true,
                allow_scripts: true,
                include_top_folders: false,
                filter_noise: false,
                label_prefix: "工具",
            });
        }
    }
    roots
}

pub(super) fn app_watch_paths(apps_cfg: &AppsConfig) -> Vec<PathBuf> {
    collect_dir_roots(apps_cfg)
        .into_iter()
        .map(|r| r.path)
        .filter(|p| p.is_dir())
        .collect()
}

pub(super) fn scan_dir_roots(
    apps_cfg: &AppsConfig,
    seen: &mut HashSet<String>,
    seen_targets: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
    let _com = ComApartmentGuard::init();
    let roots = collect_dir_roots(apps_cfg);
    let mut visited_dirs: HashSet<String> = HashSet::new();
    for root in roots {
        scan_dir_recursive(&root, &root.path, 0, &mut visited_dirs, seen, seen_targets, out);
    }
}

fn scan_dir_recursive(
    cfg: &ScanRoot,
    dir: &Path,
    depth: usize,
    visited_dirs: &mut HashSet<String>,
    seen: &mut HashSet<String>,
    seen_targets: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
    if depth > cfg.max_depth || out.len() >= 2500 {
        return;
    }
    let dir_key = dir.to_string_lossy().to_lowercase();
    if !visited_dirs.insert(dir_key) {
        return;
    }

    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let dir_lower = dir_name.to_ascii_lowercase();
            if dir_name.starts_with('.') || dir_name.starts_with('$') || is_skipped_subdir(&dir_lower) {
                continue;
            }
            if cfg.include_top_folders && depth == 0 {
                let full = path.to_string_lossy().into_owned();
                let dedup = format!("dir:{}", full.to_lowercase());
                if seen.insert(dedup) {
                    out.push(ScannedApp {
                        id: format!("dir:{}", full.to_lowercase()),
                        name: dir_name.to_string(),
                        path: full,
                        launch_args: None,
                        subtitle: format!("{} · 文件夹", cfg.label_prefix),
                        is_uwp: false,
                        is_folder: true,
                        alias_text: None,
                    });
                }
            }
            scan_dir_recursive(cfg, &path, depth + 1, visited_dirs, seen, seen_targets, out);
            continue;
        }

        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let is_lnk = ext.eq_ignore_ascii_case("lnk");
        let is_exe = ext.eq_ignore_ascii_case("exe");
        let is_appref = ext.eq_ignore_ascii_case("appref-ms");
        let is_script = matches!(
            ext.to_ascii_lowercase().as_str(),
            "bat" | "cmd" | "ps1" | "msc" | "url"
        );
        if !is_lnk && !is_appref && !(cfg.allow_exe && is_exe) && !(cfg.allow_scripts && is_script) {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let lower_name = stem.to_lowercase();
        if (cfg.filter_noise && is_noisy_shortcut(&lower_name))
            || (is_exe && is_noisy_portable_exe(&lower_name))
        {
            continue;
        }
        if !seen.insert(lower_name.clone()) {
            continue;
        }

        let (alias_text, target_lower) = if is_lnk {
            resolve_lnk_target_info(&path, &lower_name)
        } else {
            (None, is_exe.then(|| path.to_string_lossy().to_lowercase()))
        };
        if let Some(t) = target_lower {
            seen_targets.insert(t);
        }

        let full = path.to_string_lossy().into_owned();
        let subtitle = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|f| format!("{} · {}", cfg.label_prefix, f.to_string_lossy()))
            .unwrap_or_else(|| cfg.label_prefix.to_string());

        out.push(ScannedApp {
            id: format!("app:{}", full.to_lowercase()),
            name: stem.to_string(),
            path: full,
            launch_args: None,
            subtitle,
            is_uwp: false,
            is_folder: false,
            alias_text,
        });
    }
}

fn is_skipped_subdir(lower_name: &str) -> bool {
    matches!(
        lower_name,
        "node_modules"
            | ".git"
            | ".hg"
            | ".svn"
            | "__pycache__"
            | "cache"
            | "temp"
            | "tmp"
            | "logs"
            | "_uninstalled"
            | "locales"
            | "resources"
            | "vcredist"
            | "redist"
    )
}

fn is_noisy_portable_exe(lower: &str) -> bool {
    const PREFIXES: [&str; 10] = [
        "unins",
        "setup",
        "install",
        "update",
        "crash",
        "vc_redist",
        "elevate",
        "helper",
        "reporter",
        "webview2",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p)) || lower.ends_with("_helper")
}

/// Reads the `.lnk` target path header (`SLGP_RAWPATH`, zero disk/network resolve)
/// and returns `(alias_stem, lowercased_full_target_path)`.
#[cfg(windows)]
fn resolve_lnk_target_info(lnk_path: &Path, shortcut_lower: &str) -> (Option<String>, Option<String>) {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

    let wide: Vec<u16> = lnk_path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let Ok(link): windows::core::Result<IShellLinkW> =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
        else {
            return (None, None);
        };
        let Ok(persist): windows::core::Result<IPersistFile> = link.cast() else {
            return (None, None);
        };
        if persist.Load(PCWSTR(wide.as_ptr()), STGM(0)).is_err() {
            return (None, None);
        }

        let mut target_buf = [0u16; 260];
        if link
            .GetPath(&mut target_buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .is_err()
        {
            return (None, None);
        }
        let len = target_buf
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(target_buf.len());
        let target_str = String::from_utf16_lossy(&target_buf[..len]).trim().to_string();
        if target_str.is_empty() {
            return (None, None);
        }
        let stem = Path::new(&target_str)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::trim)
            .unwrap_or("");
        let alias = if stem.is_empty() || stem.to_lowercase() == shortcut_lower {
            None
        } else {
            Some(stem.to_string())
        };
        (alias, Some(target_str.to_lowercase()))
    }
}

#[cfg(not(windows))]
fn resolve_lnk_target_info(_lnk_path: &Path, _shortcut_lower: &str) -> (Option<String>, Option<String>) {
    (None, None)
}

#[cfg(windows)]
fn known_desktop_dir() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    unsafe {
        let pwstr = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).ok()?;
        let s = pwstr.to_string().ok();
        CoTaskMemFree(Some(pwstr.0 as *const std::ffi::c_void));
        s.map(PathBuf::from).filter(|p| p.is_dir())
    }
}

#[cfg(not(windows))]
fn known_desktop_dir() -> Option<PathBuf> {
    None
}

#[cfg(windows)]
struct ComApartmentGuard(bool);

#[cfg(windows)]
impl ComApartmentGuard {
    fn init() -> Self {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self(hr.is_ok())
    }
}

#[cfg(windows)]
impl Drop for ComApartmentGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

#[cfg(not(windows))]
struct ComApartmentGuard;
#[cfg(not(windows))]
impl ComApartmentGuard {
    fn init() -> Self {
        Self
    }
}
