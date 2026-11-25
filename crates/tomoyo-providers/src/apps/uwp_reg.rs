#[cfg(windows)]
use super::is_noisy_shortcut;
use super::scanner::ScannedApp;
use std::collections::HashSet;
#[cfg(windows)]
use std::path::Path;

/// Native COM enumeration of `FOLDERID_AppsFolder` indexing Windows Store UWP/MSIX apps
/// AND desktop Win32 entries registered in Shell AppsFolder.
#[cfg(windows)]
pub(super) fn scan_apps_folder(
    seen: &mut HashSet<String>,
    seen_targets: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
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

            if parse_id.is_empty() {
                continue;
            }

            // Direct Win32 executable paths in AppsFolder (not UWP AUMIDs)
            if !parse_id.contains('!') && Path::new(&parse_id).is_file() {
                let ext = Path::new(&parse_id)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if !ext.eq_ignore_ascii_case("exe") && !ext.eq_ignore_ascii_case("lnk") {
                    continue;
                }
                let target_low = parse_id.to_lowercase();
                if seen.insert(lower) {
                    seen_targets.insert(target_low.clone());
                    out.push(ScannedApp {
                        id: format!("app:af:{target_low}"),
                        name,
                        path: parse_id.clone(),
                        launch_args: None,
                        subtitle: format!("应用 · {parse_id}"),
                        is_uwp: false,
                        is_folder: false,
                        is_file: false,
                        alias_text: None,
                    });
                }
                continue;
            }

            // UWP / MSIX AUMIDs or KnownFolder GUID shell paths
            if (parse_id.contains('!') || parse_id.starts_with('{')) && seen.insert(lower) {
                let is_uwp = parse_id.contains('!');
                out.push(ScannedApp {
                    id: format!("app:uwp:{}", parse_id.to_lowercase()),
                    name,
                    path: "explorer.exe".into(),
                    launch_args: Some(format!("shell:AppsFolder\\{parse_id}")),
                    subtitle: if is_uwp {
                        format!("UWP / Store 应用 · {parse_id}")
                    } else {
                        format!("系统应用 · {parse_id}")
                    },
                    is_uwp: true,
                    is_folder: false,
                    is_file: false,
                    alias_text: None,
                });
            }
        }
    }
}

#[cfg(not(windows))]
pub(super) fn scan_apps_folder(
    _seen: &mut HashSet<String>,
    _seen_targets: &mut HashSet<String>,
    _out: &mut Vec<ScannedApp>,
) {
}

#[cfg(windows)]
pub(super) fn scan_registry_app_paths(
    seen: &mut HashSet<String>,
    seen_targets: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
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
                            .as_chunks::<2>().0
                            .iter()
                            .map(|&c| u16::from_le_bytes(c))
                            .take_while(|&c| c != 0)
                            .collect();
                        let raw_path = String::from_utf16_lossy(&u16s)
                            .trim()
                            .trim_matches('"')
                            .trim()
                            .to_string();
                        let target_low = raw_path.to_lowercase();
                        if !raw_path.is_empty()
                            && Path::new(&raw_path).exists()
                            && seen.insert(lower)
                        {
                            seen_targets.insert(target_low.clone());
                            out.push(ScannedApp {
                                id: format!("app:reg:{target_low}"),
                                name: stem.to_string(),
                                path: raw_path.clone(),
                                launch_args: None,
                                subtitle: format!("系统注册应用 · {raw_path}"),
                                is_uwp: false,
                                is_folder: false,
                                is_file: false,
                                alias_text: None,
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
pub(super) fn scan_registry_app_paths(
    _seen: &mut HashSet<String>,
    _seen_targets: &mut HashSet<String>,
    _out: &mut Vec<ScannedApp>,
) {
}

/// Fallback scan of installed applications in Windows `Uninstall` registry keys whose executables
/// were not already indexed via Desktop, Start Menu, App Paths, or AppsFolder.
#[cfg(windows)]
pub(super) fn scan_registry_uninstall(
    seen: &mut HashSet<String>,
    seen_targets: &mut HashSet<String>,
    out: &mut Vec<ScannedApp>,
) {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ,
    };

    let hives: [(HKEY, &str); 3] = [
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_CURRENT_USER, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
    ];
    for (root, path_str) in hives {
        let subkey: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let mut hkey = HKEY::default();
            if RegOpenKeyExW(root, PCWSTR(subkey.as_ptr()), Some(0), KEY_READ, &mut hkey).is_err() {
                continue;
            }
            let mut idx = 0u32;
            let mut name_buf = [0u16; 260];
            loop {
                let mut name_len = name_buf.len() as u32;
                if RegEnumKeyExW(
                    hkey,
                    idx,
                    Some(windows::core::PWSTR(name_buf.as_mut_ptr())),
                    &mut name_len,
                    None,
                    None,
                    None,
                    None,
                )
                .is_err()
                {
                    break;
                }
                idx += 1;
                let sub = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                let sub_w: Vec<u16> = sub.encode_utf16().chain(std::iter::once(0)).collect();
                let mut item_key = HKEY::default();
                if RegOpenKeyExW(hkey, PCWSTR(sub_w.as_ptr()), Some(0), KEY_READ, &mut item_key).is_err() {
                    continue;
                }
                if reg_read_dword(item_key, "SystemComponent") == Some(1)
                    || reg_read_string(item_key, "ParentKeyName").is_some()
                {
                    let _ = RegCloseKey(item_key);
                    continue;
                }
                let disp = reg_read_string(item_key, "DisplayName").unwrap_or_default();
                let icon = reg_read_string(item_key, "DisplayIcon").unwrap_or_default();
                let _ = RegCloseKey(item_key);
                if let Some((clean_name, exe_path)) = parse_uninstall_candidate(&disp, &icon) {
                    let name_low = clean_name.to_lowercase();
                    let exe_low = exe_path.to_lowercase();
                    if !seen_targets.contains(&exe_low) && !seen.contains(&name_low) && Path::new(&exe_path).is_file() {
                        seen.insert(name_low);
                        seen_targets.insert(exe_low.clone());
                        let stem = Path::new(&exe_path).file_stem().and_then(|s| s.to_str()).map(String::from);
                        out.push(ScannedApp {
                            id: format!("app:uninst:{exe_low}"),
                            name: clean_name,
                            path: exe_path.clone(),
                            launch_args: None,
                            subtitle: format!("已安装应用 · {exe_path}"),
                            is_uwp: false,
                            is_folder: false,
                            is_file: false,
                            alias_text: stem,
                        });
                    }
                }
            }
            let _ = RegCloseKey(hkey);
        }
    }
}

#[cfg(not(windows))]
pub(super) fn scan_registry_uninstall(
    _seen: &mut HashSet<String>,
    _seen_targets: &mut HashSet<String>,
    _out: &mut Vec<ScannedApp>,
) {
}

#[cfg(windows)]
fn parse_uninstall_candidate(display_name: &str, display_icon: &str) -> Option<(String, String)> {
    let name = display_name.trim();
    if name.is_empty() {
        return None;
    }
    let name_low = name.to_lowercase();
    for skip in ["runtime", "redistributable", "sdk", "driver", "update for", "hotfix", "语言包"] {
        if name_low.contains(skip) {
            return None;
        }
    }
    let raw_icon = display_icon.trim();
    let no_idx = raw_icon.rsplit_once(',').map(|(l, _)| l).unwrap_or(raw_icon);
    let exe = no_idx.trim().trim_matches('"').trim();
    let exe_low = exe.to_lowercase();
    if !exe_low.ends_with(".exe")
        || exe_low.contains("package cache")
        || exe_low.contains("\\windows\\installer\\")
        || exe_low.contains("msiexec")
    {
        return None;
    }
    let stem = Path::new(exe).file_stem().and_then(|s| s.to_str())?.to_lowercase();
    for bad in ["unins", "setup", "install", "update", "crash", "helper", "redist"] {
        if stem.starts_with(bad) || stem.ends_with("installer") && !name_low.contains("uninstaller") {
            return None;
        }
    }
    Some((name.to_string(), exe.to_string()))
}

#[cfg(windows)]
unsafe fn reg_read_string(hkey: windows::Win32::System::Registry::HKEY, name: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegQueryValueExW, REG_VALUE_TYPE};
    let name_w: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut data = [0u8; 1024];
    let mut len = data.len() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    unsafe {
        RegQueryValueExW(hkey, PCWSTR(name_w.as_ptr()), None, Some(&mut kind), Some(data.as_mut_ptr()), Some(&mut len)).ok().ok()?;
    }
    if len < 2 {
        return None;
    }
    let u16s: Vec<u16> = data[..len as usize]
        .as_chunks::<2>().0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .take_while(|&c| c != 0)
        .collect();
    let s = String::from_utf16_lossy(&u16s).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(windows)]
unsafe fn reg_read_dword(hkey: windows::Win32::System::Registry::HKEY, name: &str) -> Option<u32> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegQueryValueExW, REG_DWORD, REG_VALUE_TYPE};
    let name_w: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut val = 0u32;
    let mut len = 4u32;
    let mut kind = REG_VALUE_TYPE::default();
    unsafe {
        RegQueryValueExW(hkey, PCWSTR(name_w.as_ptr()), None, Some(&mut kind), Some(&mut val as *mut u32 as *mut u8), Some(&mut len)).ok().ok()?;
    }
    (kind == REG_DWORD).then_some(val)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn push_windows_builtin_apps(seen_names: &mut HashSet<String>, raw_entries: &mut Vec<ScannedApp>) {
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
        if seen_names.insert(name.to_lowercase()) {
            raw_entries.push(ScannedApp {
                id: format!("app:sys:{exe}"),
                name: name.to_string(),
                path: exe.to_string(),
                launch_args: None,
                subtitle: desc.to_string(),
                is_uwp: false,
                is_folder: false,
                is_file: false,
                alias_text: None,
            });
        }
    }
}
