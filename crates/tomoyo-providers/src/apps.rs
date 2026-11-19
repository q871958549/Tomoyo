use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tomoyo_core::{score_match, IndexedText, Lang, Provider, TomoyoConfig};
use tomoyo_protocol::{ActionKind, ActionSpec, IconSpec, QueryContext, ResultCategory, ResultItem};

#[derive(Debug, Clone)]
struct AppEntry {
    id: String,
    name: String,
    path: String,
    launch_args: Option<String>,
    subtitle: String,
    is_uwp: bool,
    indexed: IndexedText,
}

/// Fast in-memory installed application index supporting Start Menu shortcuts,
/// Registry App Paths, AND Windows Store UWP/MSIX apps (`shell:AppsFolder`),
/// with full Chinese Pinyin & acronym matching.
pub struct AppsProvider {
    entries: RwLock<Vec<AppEntry>>,
    config: Arc<RwLock<TomoyoConfig>>,
}

impl AppsProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        let provider = Self {
            entries: RwLock::new(Vec::new()),
            config,
        };
        provider.reload();
        provider
    }

    pub fn app_count(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }
}

impl Provider for AppsProvider {
    fn id(&self) -> &'static str {
        "builtin.apps"
    }

    fn name(&self) -> &'static str {
        "Installed Applications"
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let q = ctx.trimmed.as_str();
        let lang = self.lang();
        let Ok(entries) = self.entries.read() else {
            return Vec::new();
        };

        if q.is_empty() {
            return entries
                .iter()
                .take(12)
                .map(|app| to_result_item(app, 500, lang))
                .collect();
        }

        let mut matched: Vec<ResultItem> = entries
            .iter()
            .filter_map(|app| {
                let s = score_match(q, &app.indexed)?;
                Some(to_result_item(app, s, lang))
            })
            .collect();

        matched.sort_by(|a, b| b.score.cmp(&a.score));
        matched.truncate(ctx.limit.min(12));
        matched
    }

    fn reload(&self) {
        let scanned = scan_installed_apps();
        tracing::info!("Indexed {} installed applications (StartMenu + Registry + UWP)", scanned.len());
        if let Ok(mut guard) = self.entries.write() {
            *guard = scanned;
            guard.shrink_to_fit();
        }
    }

    fn trim_memory(&self) {
        if let Ok(mut guard) = self.entries.write() {
            guard.shrink_to_fit();
        }
    }
}

fn to_result_item(app: &AppEntry, score: i64, lang: Lang) -> ResultItem {
    let parent = if app.is_uwp {
        String::new()
    } else {
        Path::new(&app.path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };

    let mut actions = vec![ActionSpec {
        id: "launch".into(),
        label: lang.pick("启动应用", "Launch Application").into(),
        shortcut: Some("Enter".into()),
        kind: ActionKind::LaunchPath {
            path: app.path.clone(),
            args: app.launch_args.clone(),
            working_dir: if parent.is_empty() { None } else { Some(parent) },
        },
    }];

    if !app.is_uwp {
        actions.push(ActionSpec {
            id: "reveal".into(),
            label: lang.pick("打开文件位置", "Open File Location").into(),
            shortcut: Some("Ctrl+Enter".into()),
            kind: ActionKind::RevealInExplorer {
                path: app.path.clone(),
            },
        });
        actions.push(ActionSpec {
            id: "copy_path".into(),
            label: lang.pick("复制路径", "Copy Path").into(),
            shortcut: Some("Ctrl+C".into()),
            kind: ActionKind::CopyText {
                text: app.path.clone(),
                paste_immediately: false,
            },
        });
    }

    let icon = if app.is_uwp {
        let mono: String = app
            .name
            .chars()
            .filter(|c| !c.is_whitespace())
            .take(2)
            .collect();
        IconSpec::Monogram(if mono.is_empty() { "APP".into() } else { mono })
    } else {
        IconSpec::ShellPath(app.path.clone())
    };

    ResultItem {
        id: app.id.clone(),
        provider_id: "builtin.apps".into(),
        title: app.name.clone(),
        subtitle: app.subtitle.clone(),
        category: ResultCategory::App,
        score,
        icon,
        badge: if app.is_uwp { Some("UWP".into()) } else { None },
        copy_text: Some(app.path.clone()),
        actions,
        preview: None,
    }
}

fn scan_installed_apps() -> Vec<AppEntry> {
    let mut entries = Vec::with_capacity(320);
    let mut seen_names = HashSet::with_capacity(320);

    let mut roots = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Ok(progdata) = std::env::var("ProgramData") {
        roots.push(
            PathBuf::from(progdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(userprofile).join("Desktop"));
    }

    for root in roots {
        scan_dir_recursive(&root, 0, &mut seen_names, &mut entries);
    }

    scan_registry_app_paths(&mut seen_names, &mut entries);
    scan_uwp_apps(&mut seen_names, &mut entries);

    for (name, exe, desc) in [
        ("记事本 Notepad", "notepad.exe", "Windows 记事本"),
        ("计算器 Calculator", "calc.exe", "Windows 计算器"),
        ("命令提示符 CMD", "cmd.exe", "Windows 命令提示符"),
        ("PowerShell 终端", "powershell.exe", "Windows PowerShell"),
        ("画图 Paint", "mspaint.exe", "Windows 画图"),
        ("控制面板 Control Panel", "control.exe", "Windows 控制面板"),
        ("资源管理器 Explorer", "explorer.exe", "Windows 文件资源管理器"),
        ("注册表编辑器 Regedit", "regedit.exe", "Windows 注册表编辑器"),
    ] {
        let key = name.to_lowercase();
        if seen_names.insert(key) {
            entries.push(AppEntry {
                id: format!("app:sys:{exe}"),
                name: name.to_string(),
                path: exe.to_string(),
                launch_args: None,
                subtitle: desc.to_string(),
                is_uwp: false,
                indexed: IndexedText::new(name),
            });
        }
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn scan_dir_recursive(
    dir: &Path,
    depth: usize,
    seen: &mut HashSet<String>,
    out: &mut Vec<AppEntry>,
) {
    if depth > 4 || out.len() >= 1800 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(&path, depth + 1, seen, out);
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("lnk") && !ext.eq_ignore_ascii_case("exe") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let lower_name = stem.to_lowercase();
        if is_noisy_shortcut(&lower_name) {
            continue;
        }
        if !seen.insert(lower_name) {
            continue;
        }
        let full = path.to_string_lossy().into_owned();
        let subtitle = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|f| format!("应用 · {}", f.to_string_lossy()))
            .unwrap_or_else(|| "本地应用".into());

        out.push(AppEntry {
            id: format!("app:{}", full.to_lowercase()),
            name: stem.to_string(),
            path: full,
            launch_args: None,
            subtitle,
            is_uwp: false,
            indexed: IndexedText::new(stem),
        });
    }
}

fn is_noisy_shortcut(lower: &str) -> bool {
    const NOISE: [&str; 10] = [
        "uninstall",
        "卸载",
        "readme",
        "help",
        "manual",
        "license",
        "documentation",
        "release notes",
        "website",
        "访问主页",
    ];
    NOISE.iter().any(|n| lower.contains(n))
}

/// Native COM enumeration of `FOLDERID_AppsFolder` indexing Windows Store UWP / MSIX apps!
#[cfg(windows)]
fn scan_uwp_apps(seen: &mut HashSet<String>, out: &mut Vec<AppEntry>) {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem,
        KF_FLAG_DEFAULT, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
    };

    unsafe {
        let Ok(folder): windows::core::Result<IShellItem> =
            SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)
        else {
            return;
        };
        let Ok(enum_items): windows::core::Result<IEnumShellItems> =
            folder.BindToHandler(None, &BHID_EnumItems)
        else {
            return;
        };

        let mut count = 0usize;
        loop {
            let mut batch: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            if enum_items.Next(&mut batch, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = batch[0].take() else {
                break;
            };
            count += 1;
            if count > 800 {
                break;
            }

            let Ok(name_pw) = item.GetDisplayName(SIGDN_NORMALDISPLAY) else {
                continue;
            };
            let name = name_pw.to_string().unwrap_or_default();
            CoTaskMemFree(Some(name_pw.0 as *const _));

            let lower = name.trim().to_lowercase();
            if lower.is_empty() || is_noisy_shortcut(&lower) || seen.contains(&lower) {
                continue;
            }

            let Ok(parse_pw) = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING) else {
                continue;
            };
            let parse_id = parse_pw.to_string().unwrap_or_default();
            CoTaskMemFree(Some(parse_pw.0 as *const _));

            // Keep UWP/MSIX AUMIDs (contain `!`) that aren't already indexed via Start Menu `.lnk`.
            if parse_id.contains('!') && seen.insert(lower) {
                out.push(AppEntry {
                    id: format!("app:uwp:{}", parse_id.to_lowercase()),
                    name: name.clone(),
                    path: "explorer.exe".into(),
                    launch_args: Some(format!("shell:AppsFolder\\{parse_id}")),
                    subtitle: format!("UWP / Store 应用 · {parse_id}"),
                    is_uwp: true,
                    indexed: IndexedText::new(&name),
                });
            }
        }
    }
}

#[cfg(not(windows))]
fn scan_uwp_apps(_seen: &mut HashSet<String>, _out: &mut Vec<AppEntry>) {}

#[cfg(windows)]
fn scan_registry_app_paths(seen: &mut HashSet<String>, out: &mut Vec<AppEntry>) {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, REG_VALUE_TYPE,
    };

    let subkey: Vec<u16> = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        unsafe {
            let mut hkey = HKEY::default();
            if RegOpenKeyExW(root, PCWSTR(subkey.as_ptr()), Some(0), KEY_READ, &mut hkey).is_err() {
                continue;
            }
            let mut index = 0u32;
            let mut name_buf = [0u16; 260];
            loop {
                let mut name_len = name_buf.len() as u32;
                let res = RegEnumKeyExW(
                    hkey,
                    index,
                    Some(windows::core::PWSTR(name_buf.as_mut_ptr())),
                    &mut name_len,
                    None,
                    None,
                    None,
                    None,
                );
                if res.is_err() {
                    break;
                }
                index += 1;
                let key_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                let stem = key_name.strip_suffix(".exe").unwrap_or(&key_name);
                let lower = stem.to_lowercase();
                if lower.is_empty() || is_noisy_shortcut(&lower) || seen.contains(&lower) {
                    continue;
                }

                let mut app_key = HKEY::default();
                let key_w: Vec<u16> = key_name.encode_utf16().chain(std::iter::once(0)).collect();
                if RegOpenKeyExW(hkey, PCWSTR(key_w.as_ptr()), Some(0), KEY_READ, &mut app_key)
                    .is_ok()
                {
                    let mut data = [0u8; 1024];
                    let mut data_len = data.len() as u32;
                    let mut kind = REG_VALUE_TYPE::default();
                    if RegQueryValueExW(
                        app_key,
                        PCWSTR::null(),
                        None,
                        Some(&mut kind),
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    )
                    .is_ok()
                        && data_len >= 2
                    {
                        let u16s: Vec<u16> = data[..data_len as usize]
                            .chunks_exact(2)
                            .map(|c| u16::from_le_bytes([c[0], c[1]]))
                            .take_while(|&c| c != 0)
                            .collect();
                        let raw_path = String::from_utf16_lossy(&u16s)
                            .trim_matches('"')
                            .trim()
                            .to_string();
                        if !raw_path.is_empty()
                            && Path::new(&raw_path).exists()
                            && seen.insert(lower)
                        {
                            out.push(AppEntry {
                                id: format!("app:reg:{}", raw_path.to_lowercase()),
                                name: stem.to_string(),
                                path: raw_path.clone(),
                                launch_args: None,
                                subtitle: format!("系统注册应用 · {raw_path}"),
                                is_uwp: false,
                                indexed: IndexedText::new(stem),
                            });
                        }
                    }
                    let _ = RegCloseKey(app_key);
                }
            }
            let _ = RegCloseKey(hkey);
        }
    }
}

#[cfg(not(windows))]
fn scan_registry_app_paths(_seen: &mut HashSet<String>, _out: &mut Vec<AppEntry>) {}
