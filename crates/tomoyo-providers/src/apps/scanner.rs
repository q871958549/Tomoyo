use super::is_noisy_shortcut;
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
    pub alias_text: Option<String>,
}

struct ScanRoot {
    path: PathBuf,
    max_depth: usize,
    allow_exe: bool,
    label_prefix: &'static str,
}

pub(super) fn scan_dir_roots(
    apps_cfg: &AppsConfig,
    seen: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
    let _com = ComApartmentGuard::init();
    let mut roots: Vec<ScanRoot> = Vec::new();

    // 1. User Desktop & Public Desktop (direct user shortcuts take highest priority)
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        roots.push(ScanRoot {
            path: PathBuf::from(&userprofile).join("Desktop"),
            max_depth: 3,
            allow_exe: true,
            label_prefix: "桌面",
        });
    }
    if let Ok(public_dir) = std::env::var("PUBLIC") {
        roots.push(ScanRoot {
            path: PathBuf::from(public_dir).join("Desktop"),
            max_depth: 3,
            allow_exe: true,
            label_prefix: "公共桌面",
        });
    }

    // 2. Auto-detected Desktop Organizers (DeskBox, Tencent DeskGo, etc.)
    if apps_cfg.auto_detect_desktop_organizers {
        for org_dir in detect_desktop_organizer_dirs() {
            roots.push(ScanRoot {
                path: org_dir,
                max_depth: 3,
                allow_exe: false,
                label_prefix: "桌面整理",
            });
        }
    }

    // 3. Full Start Menu roots (includes top-level shortcuts outside `\Programs`)
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(ScanRoot {
            path: PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu"),
            max_depth: 5,
            allow_exe: true,
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
            label_prefix: "应用",
        });
    }

    // 4. User-configured custom application / portable tools directories
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
                label_prefix: "工具",
            });
        }
    }

    let mut visited_dirs: HashSet<String> = HashSet::new();
    for root in roots {
        scan_dir_recursive(&root, &root.path, 0, &mut visited_dirs, seen, out);
    }
}

fn scan_dir_recursive(
    cfg: &ScanRoot,
    dir: &Path,
    depth: usize,
    visited_dirs: &mut HashSet<String>,
    seen: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
    if depth > cfg.max_depth || out.len() >= 2200 {
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
            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if is_skipped_subdir(&dir_name) {
                continue;
            }
            scan_dir_recursive(cfg, &path, depth + 1, visited_dirs, seen, out);
            continue;
        }

        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let is_lnk = ext.eq_ignore_ascii_case("lnk");
        let is_exe = ext.eq_ignore_ascii_case("exe");
        if !is_lnk && !(cfg.allow_exe && is_exe) {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let lower_name = stem.to_lowercase();
        if is_noisy_shortcut(&lower_name) || (is_exe && is_noisy_portable_exe(&lower_name)) {
            continue;
        }
        if !seen.insert(lower_name.clone()) {
            continue;
        }

        let alias_text = if is_lnk {
            resolve_lnk_target_stem(&path, &lower_name)
        } else {
            None
        };

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

/// Auto-discovers managed folders of popular Windows desktop organizers (e.g. DeskBox).
fn detect_desktop_organizer_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Ok(local_app) = std::env::var("LOCALAPPDATA") else {
        return dirs;
    };
    let deskbox_data = PathBuf::from(&local_app).join("DeskBox").join("data");

    if let Ok(raw) = std::fs::read_to_string(deskbox_data.join("settings.json")) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(root) = json
                .get("defaultManagedStorageRootPath")
                .and_then(|v| v.as_str())
            {
                push_existing_dir(&mut dirs, PathBuf::from(root));
            }
        }
    }

    if let Ok(raw) = std::fs::read_to_string(deskbox_data.join("widget-layout.json")) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(widgets) = json
                .get("layout")
                .and_then(|l| l.get("widgets"))
                .and_then(|w| w.as_array())
            {
                for w in widgets {
                    if let Some(mapped) = w.get("mappedFolderPath").and_then(|v| v.as_str()) {
                        push_existing_dir(&mut dirs, PathBuf::from(mapped));
                    }
                }
            }
        }
    }

    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let fallback = PathBuf::from(userprofile).join("DeskBox");
        push_existing_dir(&mut dirs, fallback);
    }

    dirs
}

fn push_existing_dir(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if !dir.is_dir() {
        return;
    }
    let key = dir.to_string_lossy().to_lowercase();
    if !dirs.iter().any(|d| d.to_string_lossy().to_lowercase() == key) {
        dirs.push(dir);
    }
}

/// Reads the `.lnk` target path header (`SLGP_RAWPATH`, zero disk/network resolve)
/// and returns its executable stem if different from the shortcut display name.
#[cfg(windows)]
fn resolve_lnk_target_stem(lnk_path: &Path, shortcut_lower: &str) -> Option<String> {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

    let wide: Vec<u16> = lnk_path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        persist.Load(PCWSTR(wide.as_ptr()), STGM(0)).ok()?; // STGM_READ = 0

        let mut target_buf = [0u16; 260];
        if link
            .GetPath(&mut target_buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .is_err()
        {
            return None;
        }
        let len = target_buf
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(target_buf.len());
        let target_str = String::from_utf16_lossy(&target_buf[..len]);
        let stem = Path::new(target_str.trim())
            .file_stem()
            .and_then(|s| s.to_str())?
            .trim();
        if stem.is_empty() || stem.to_lowercase() == shortcut_lower {
            None
        } else {
            Some(stem.to_string())
        }
    }
}

#[cfg(not(windows))]
fn resolve_lnk_target_stem(_lnk_path: &Path, _shortcut_lower: &str) -> Option<String> {
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
