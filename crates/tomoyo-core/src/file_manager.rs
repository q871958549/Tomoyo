//! Default third-party file manager detection (OneCommander, Directory Opus, XYplorer,
//! Total Commander, Files App) with clean fallback to native `explorer.exe /select`.

use std::path::Path;

#[cfg(windows)]
fn read_reg_default_sz(root: windows::Win32::System::Registry::HKEY, subkey: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
        REG_VALUE_TYPE,
    };
    let sub_w: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(root, PCWSTR(sub_w.as_ptr()), Some(0), KEY_READ, &mut hkey).is_err() {
            return None;
        }
        let mut kind = REG_VALUE_TYPE::default();
        let mut bytes = 0u32;
        let q1 = RegQueryValueExW(hkey, PCWSTR::null(), None, Some(&mut kind), None, Some(&mut bytes));
        let mut result = None;
        if q1.is_ok() && (kind == REG_SZ || kind == REG_EXPAND_SZ) && (2..=4096).contains(&bytes) {
            let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
            let mut b2 = (buf.len() * 2) as u32;
            if RegQueryValueExW(
                hkey,
                PCWSTR::null(),
                None,
                Some(&mut kind),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut b2),
            )
            .is_ok()
            {
                let u16_len = (b2 as usize / 2).min(buf.len());
                let trimmed = String::from_utf16_lossy(&buf[..u16_len])
                    .trim_end_matches('\0')
                    .trim()
                    .to_string();
                if !trimmed.is_empty() {
                    result = Some(trimmed);
                }
            }
        }
        let _ = RegCloseKey(hkey);
        result
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse_registered_command(raw_cmd: &str) -> Option<(String, String)> {
    let s = raw_cmd.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(after_q) = s.strip_prefix('"') {
        let end = after_q.find('"')?;
        let exe = after_q[..end].trim();
        let rest = after_q[end + 1..].trim();
        return (!exe.is_empty()).then(|| (exe.into(), rest.into()));
    }
    let low = s.to_ascii_lowercase();
    if let Some(idx) = low.find(".exe") {
        let end = idx + 4;
        let exe = s[..end].trim();
        let rest = s[end..].trim();
        return (!exe.is_empty()).then(|| (exe.into(), rest.into()));
    }
    let mut parts = s.splitn(2, char::is_whitespace);
    let exe = parts.next()?.trim();
    let rest = parts.next().unwrap_or("").trim();
    (!exe.is_empty()).then(|| (exe.into(), rest.into()))
}

#[cfg_attr(not(windows), allow(dead_code))]
fn target_dir_for_generic(path: &str) -> String {
    let p = Path::new(path);
    if p.is_dir() {
        path.to_string()
    } else {
        p.parent()
            .map(|d| d.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| path.to_string())
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn build_reveal_from_template(
    exe: &str,
    args_tpl: &str,
    path: &str,
) -> Option<(String, String)> {
    let exe_name = exe
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if exe_name.is_empty() || exe_name == "explorer.exe" {
        return None;
    }
    match exe_name.as_str() {
        "onecommander.exe" => Some((exe.into(), format!("-\"{path}\""))),
        "dopus.exe" => {
            let rt = Path::new(exe).with_file_name("dopusrt.exe");
            let runner = if rt.is_file() { rt.to_string_lossy().into_owned() } else { exe.into() };
            Some((runner, format!("/acmd Go \"{path}\" NEWTAB=findexisting")))
        }
        "dopusrt.exe" => Some((exe.into(), format!("/acmd Go \"{path}\" NEWTAB=findexisting"))),
        "xyplorer.exe" => Some((exe.into(), format!("/select=\"{path}\""))),
        "totalcmd64.exe" | "totalcmd.exe" => Some((exe.into(), format!("/O /T /L=\"{path}\""))),
        "files.exe" => Some((exe.into(), format!("\"{path}\""))),
        _ => {
            let dir = target_dir_for_generic(path);
            let quoted = format!("\"{dir}\"");
            let mut args = args_tpl.trim().to_string();
            let mut replaced = false;
            for pat in ["\"%1\"", "\"%V\"", "\"%L\"", "%1", "%V", "%L"] {
                if args.contains(pat) {
                    args = args.replace(pat, &quoted);
                    replaced = true;
                }
            }
            Some((exe.into(), if replaced { args } else { quoted }))
        }
    }
}

#[cfg(windows)]
fn resolve_custom_windows_file_manager(path: &str) -> Option<(String, String)> {
    use windows::Win32::System::Registry::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
    let probes = [
        (HKEY_CURRENT_USER, "Software\\Classes\\Directory\\shell"),
        (HKEY_CURRENT_USER, "Software\\Classes\\Folder\\shell"),
        (HKEY_CLASSES_ROOT, "Directory\\shell"),
    ];
    for (root, base) in probes {
        let Some(verb) = read_reg_default_sz(root, base) else {
            continue;
        };
        if verb.eq_ignore_ascii_case("none") || verb.eq_ignore_ascii_case("openinexplorer") {
            continue;
        }
        let cmd_key = format!("{base}\\{verb}\\command");
        let Some(raw_cmd) = read_reg_default_sz(root, &cmd_key) else {
            continue;
        };
        let Some((exe, tpl)) = parse_registered_command(&raw_cmd) else {
            continue;
        };
        if !Path::new(&exe).is_file() {
            continue;
        }
        if let Some(pair) = build_reveal_from_template(&exe, &tpl, path) {
            return Some(pair);
        }
    }
    None
}

/// Resolves the active OS file manager executable + arguments to reveal `path`.
pub fn resolve_reveal_file_command(path: &str) -> (String, String) {
    #[cfg(windows)]
    {
        if let Some(custom) = resolve_custom_windows_file_manager(path) {
            return custom;
        }
        ("explorer.exe".into(), format!("/select,\"{path}\""))
    }
    #[cfg(not(windows))]
    {
        ("xdg-open".into(), path.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unquoted_onecommander_with_spaces_and_builds_select_arg() {
        let raw = r#"C:\Program Files\OneCommander\OneCommander.exe -"%1""#;
        let (exe, tpl) = parse_registered_command(raw).unwrap();
        assert_eq!(exe, r"C:\Program Files\OneCommander\OneCommander.exe");
        let (cmd, arg) = build_reveal_from_template(&exe, &tpl, r"D:\docs\report.pdf").unwrap();
        assert_eq!(cmd, exe);
        assert_eq!(arg, "-\"D:\\docs\\report.pdf\"");
    }

    #[test]
    fn supports_xyplorer_totalcmd_generic_and_skips_explorer() {
        let (exe, tpl) = parse_registered_command(r#""E:\Tools\XYplorer.exe" "%1""#).unwrap();
        assert_eq!(
            build_reveal_from_template(&exe, &tpl, r"C:\a\b.txt"),
            Some((r"E:\Tools\XYplorer.exe".into(), "/select=\"C:\\a\\b.txt\"".into()))
        );
        assert_eq!(
            build_reveal_from_template(r"C:\tc\TOTALCMD64.EXE", "\"%1\"", r"C:\a\b.txt"),
            Some((r"C:\tc\TOTALCMD64.EXE".into(), "/O /T /L=\"C:\\a\\b.txt\"".into()))
        );
        assert!(build_reveal_from_template(r"C:\Windows\Explorer.exe", "", r"C:\a\b.txt").is_none());
    }
}
