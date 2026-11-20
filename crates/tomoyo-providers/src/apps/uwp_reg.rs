use super::is_noisy_shortcut;
use super::scanner::ScannedApp;
use std::collections::HashSet;
use std::path::Path;

/// Native COM enumeration of `FOLDERID_AppsFolder` indexing Windows Store UWP/MSIX apps
/// AND desktop Win32 entries registered in Shell AppsFolder.
#[cfg(windows)]
pub(super) fn scan_apps_folder(seen: &mut HashSet<String>, out: &mut Vec<ScannedApp>) {
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
                if seen.insert(lower) {
                    out.push(ScannedApp {
                        id: format!("app:af:{}", parse_id.to_lowercase()),
                        name,
                        path: parse_id.clone(),
                        launch_args: None,
                        subtitle: format!("应用 · {parse_id}"),
                        is_uwp: false,
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
                    alias_text: None,
                });
            }
        }
    }
}

#[cfg(not(windows))]
pub(super) fn scan_apps_folder(_seen: &mut HashSet<String>, _out: &mut Vec<ScannedApp>) {}

#[cfg(windows)]
pub(super) fn scan_registry_app_paths(seen: &mut HashSet<String>, out: &mut Vec<ScannedApp>) {
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
                        // Trim outer whitespace BEFORE stripping quotes (e.g. `"C:\...\app.exe" `)
                        let raw_path = String::from_utf16_lossy(&u16s)
                            .trim()
                            .trim_matches('"')
                            .trim()
                            .to_string();
                        if !raw_path.is_empty()
                            && Path::new(&raw_path).exists()
                            && seen.insert(lower)
                        {
                            out.push(ScannedApp {
                                id: format!("app:reg:{}", raw_path.to_lowercase()),
                                name: stem.to_string(),
                                path: raw_path.clone(),
                                launch_args: None,
                                subtitle: format!("系统注册应用 · {raw_path}"),
                                is_uwp: false,
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
pub(super) fn scan_registry_app_paths(_seen: &mut HashSet<String>, _out: &mut Vec<ScannedApp>) {}
