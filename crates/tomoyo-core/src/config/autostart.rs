//! Cross-platform login autostart management with Windows Task Scheduler elevation support.

use std::path::{Path, PathBuf};

/// Resolves the `tomoyo` / `tomoyo.exe` launcher path beside the current executable.
fn resolve_tomoyo_exe() -> Result<PathBuf, String> {
    let cur = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = if cfg!(windows) { "tomoyo.exe" } else { "tomoyo" };
    let sibling = cur.with_file_name(target);
    Ok(if sibling.is_file() { sibling } else { cur })
}

#[cfg(windows)]
mod win_impl {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_BINARY, REG_EXPAND_SZ,
        REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    pub const AUTOSTART_TASK_NAME: &str = "TomoyoAutostart";
    const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    const APPROVED_KEY: PCWSTR =
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\StartupApproved\\Run");
    const LAYERS_KEY: PCWSTR =
        w!("Software\\Microsoft\\Windows NT\\CurrentVersion\\AppCompatFlags\\Layers");
    const TASK_CACHE_KEY: PCWSTR = w!(
        "Software\\Microsoft\\Windows NT\\CurrentVersion\\Schedule\\TaskCache\\Tree\\TomoyoAutostart"
    );

    fn to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open_reg(root: HKEY, subkey: PCWSTR, access: REG_SAM_FLAGS) -> Result<HKEY, String> {
        unsafe {
            let mut hkey = HKEY::default();
            RegOpenKeyExW(root, subkey, Some(0), access, &mut hkey)
                .ok()
                .map_err(|e| format!("RegOpenKeyExW failed: {e}"))?;
            Ok(hkey)
        }
    }

    fn read_reg_string(root: HKEY, subkey: PCWSTR, value_name: PCWSTR) -> Option<String> {
        let hkey = open_reg(root, subkey, KEY_READ).ok()?;
        unsafe {
            let mut ty = REG_VALUE_TYPE::default();
            let mut len = 0u32;
            let ok = RegQueryValueExW(hkey, value_name, None, Some(&mut ty), None, Some(&mut len))
                .is_ok()
                && len >= 2
                && (ty == REG_SZ || ty == REG_EXPAND_SZ);
            let mut res = None;
            if ok {
                let mut buf = vec![0u8; len as usize];
                if RegQueryValueExW(hkey, value_name, None, None, Some(buf.as_mut_ptr()), Some(&mut len))
                    .is_ok()
                {
                    let u16s: Vec<u16> = buf[..len as usize]
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .take_while(|&ch| ch != 0)
                        .collect();
                    res = Some(String::from_utf16_lossy(&u16s));
                }
            }
            let _ = RegCloseKey(hkey);
            res
        }
    }

    /// Fast registry check (~0.05ms, zero subprocesses) for Scheduled Task `TomoyoAutostart`.
    fn has_scheduled_task_fast() -> bool {
        let Ok(hkey) = open_reg(HKEY_LOCAL_MACHINE, TASK_CACHE_KEY, KEY_READ) else {
            return false;
        };
        unsafe {
            let exists = RegQueryValueExW(hkey, w!("Id"), None, None, None, None).is_ok();
            let _ = RegCloseKey(hkey);
            exists
        }
    }

    /// Returns `true` if Task Manager / Startup Apps marked `Run\Tomoyo` as disabled (odd first byte).
    fn is_run_disabled_in_startup_approved() -> bool {
        let Ok(hkey) = open_reg(HKEY_CURRENT_USER, APPROVED_KEY, KEY_READ) else {
            return false;
        };
        unsafe {
            let mut ty = REG_VALUE_TYPE::default();
            let mut len = 64u32;
            let mut buf = vec![0u8; len as usize];
            let disabled = RegQueryValueExW(
                hkey,
                w!("Tomoyo"),
                None,
                Some(&mut ty),
                Some(buf.as_mut_ptr()),
                Some(&mut len),
            )
            .is_ok()
                && ty == REG_BINARY
                && len >= 1
                && (buf[0] & 1) != 0;
            let _ = RegCloseKey(hkey);
            disabled
        }
    }

    fn has_run_entry() -> bool {
        read_reg_string(HKEY_CURRENT_USER, RUN_KEY, w!("Tomoyo")).is_some()
    }

    fn delete_run_entry() {
        if let Ok(hkey) = open_reg(HKEY_CURRENT_USER, RUN_KEY, KEY_SET_VALUE) {
            unsafe {
                let _ = RegDeleteValueW(hkey, w!("Tomoyo"));
                let _ = RegCloseKey(hkey);
            }
        }
    }

    fn write_run_entry(exe: &Path) -> Result<(), String> {
        let hkey = open_reg(HKEY_CURRENT_USER, RUN_KEY, KEY_SET_VALUE)?;
        unsafe {
            let cmd = format!("\"{}\" --silent", exe.display());
            let wide = to_wide_null(&cmd);
            let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            let res = RegSetValueExW(hkey, w!("Tomoyo"), Some(0), REG_SZ, Some(bytes))
                .ok()
                .map_err(|e| format!("RegSetValueExW failed: {e}"));
            let _ = RegCloseKey(hkey);
            res
        }
    }

    pub fn has_runasadmin_layer(exe: &Path) -> bool {
        let wide = to_wide_null(&exe.to_string_lossy());
        let name = PCWSTR(wide.as_ptr());
        [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
            .into_iter()
            .filter_map(|root| read_reg_string(root, LAYERS_KEY, name))
            .any(|val| is_runasadmin_flag_value(&val))
    }

    fn current_windows_user_id() -> Option<String> {
        let user = std::env::var("USERNAME").ok()?.trim().to_string();
        if user.is_empty() {
            return None;
        }
        match std::env::var("USERDOMAIN") {
            Ok(d) if !d.trim().is_empty() => Some(format!("{}\\{}", d.trim(), user)),
            _ => Some(user),
        }
    }

    fn register_autostart_task(exe: &Path) -> Result<(), String> {
        let work_dir = exe.parent().unwrap_or_else(|| Path::new("."));
        let xml = build_autostart_task_xml(exe, work_dir, current_windows_user_id().as_deref());
        let tmp = std::env::temp_dir().join(format!("tomoyo_autostart_{}.xml", std::process::id()));
        // `schtasks.exe /XML` fails with "unable to switch the encoding" on UTF-8 XML; use UTF-16LE + BOM.
        std::fs::write(&tmp, encode_utf16le_bom(&xml))
            .map_err(|e| format!("write task xml: {e}"))?;
        let out = Command::new("schtasks.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["/Create", "/F", "/TN", AUTOSTART_TASK_NAME, "/XML"])
            .arg(&tmp)
            .output();
        let _ = std::fs::remove_file(&tmp);
        match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                let std = String::from_utf8_lossy(&o.stdout);
                let msg = if err.trim().is_empty() { std.trim() } else { err.trim() };
                Err(format!("schtasks /Create failed: {msg}"))
            }
            Err(e) => Err(format!("spawn schtasks failed: {e}")),
        }
    }

    fn delete_autostart_task() {
        let _ = Command::new("schtasks.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["/Delete", "/F", "/TN", AUTOSTART_TASK_NAME])
            .output();
    }

    pub fn is_autostart_enabled() -> bool {
        has_scheduled_task_fast() || (has_run_entry() && !is_run_disabled_in_startup_approved())
    }

    pub fn set_autostart_enabled(enable: bool) -> Result<(), String> {
        if !enable {
            delete_autostart_task();
            delete_run_entry();
            return Ok(());
        }
        let exe = resolve_tomoyo_exe()?;
        // Prefer Scheduled Task (`RunLevel=HighestAvailable`) so Windows `RUNASADMIN` compat layers
        // or elevated privileges never get silently rejected (PID 0) by `explorer.exe` Run key.
        match register_autostart_task(&exe) {
            Ok(()) => {
                delete_run_entry();
                Ok(())
            }
            Err(task_err) => {
                if has_runasadmin_layer(&exe) {
                    return Err(format!(
                        "当前处于非提权状态，且 tomoyo.exe 被设为兼容“以管理员身份运行”，无法写入 Run 键自启；请以管理员身份运行 Tomoyo 后重试 ({task_err})"
                    ));
                }
                write_run_entry(&exe).map_err(|run_err| format!("{task_err}; fallback: {run_err}"))
            }
        }
    }

    pub fn heal_windows_autostart_if_blocked() {
        if !has_run_entry() {
            return;
        }
        if is_run_disabled_in_startup_approved() {
            tracing::info!("Windows StartupApproved disabled HKCU Run\\Tomoyo; skipping auto-heal.");
            return;
        }
        if has_scheduled_task_fast() {
            delete_run_entry();
            tracing::info!("Cleaned duplicate HKCU\\...\\Run\\Tomoyo (Task '{AUTOSTART_TASK_NAME}' active).");
            return;
        }
        let Ok(exe) = resolve_tomoyo_exe() else { return; };
        let runas_admin = has_runasadmin_layer(&exe);
        match register_autostart_task(&exe) {
            Ok(()) => {
                delete_run_entry();
                tracing::info!(
                    "Auto-healed Windows autostart (runasadmin={runas_admin}): migrated Run -> Task '{AUTOSTART_TASK_NAME}'."
                );
            }
            Err(err) => {
                tracing::warn!(
                    "Windows autostart self-heal could not create '{AUTOSTART_TASK_NAME}' (runasadmin={runas_admin}): {err}"
                );
            }
        }
    }
}

pub(crate) fn is_runasadmin_flag_value(raw: &str) -> bool {
    raw.split_ascii_whitespace()
        .any(|token| token.eq_ignore_ascii_case("RUNASADMIN"))
}

pub(crate) fn xml_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn encode_utf16le_bom(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(2 + text.len() * 2);
    bytes.extend_from_slice(&[0xFF, 0xFE]);
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

pub(crate) fn build_autostart_task_xml(
    exe: &Path,
    working_dir: &Path,
    user_id: Option<&str>,
) -> String {
    let user_tag = user_id
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(|u| format!("      <UserId>{}</UserId>\n", xml_escape(u)))
        .unwrap_or_default();
    let exe_esc = xml_escape(&exe.to_string_lossy());
    let dir_esc = xml_escape(&working_dir.to_string_lossy());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n\
         <Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n\
           <RegistrationInfo><Description>Tomoyo desktop launcher autostart (--silent)</Description></RegistrationInfo>\n\
           <Triggers><LogonTrigger><Enabled>true</Enabled>\n{user_tag}    </LogonTrigger></Triggers>\n\
           <Principals><Principal id=\"Author\">\n{user_tag}      <LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>\n\
           <Settings>\n\
             <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n\
             <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n\
             <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n\
             <AllowHardTerminate>false</AllowHardTerminate>\n\
             <StartWhenAvailable>true</StartWhenAvailable>\n\
             <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\n\
             <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>\n\
             <AllowStartOnDemand>true</AllowStartOnDemand><Enabled>true</Enabled><Hidden>false</Hidden>\n\
             <RunOnlyIfIdle>false</RunOnlyIfIdle><WakeToRun>false</WakeToRun>\n\
             <ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Priority>5</Priority>\n\
           </Settings>\n\
           <Actions Context=\"Author\"><Exec><Command>{exe_esc}</Command><Arguments>--silent</Arguments><WorkingDirectory>{dir_esc}</WorkingDirectory></Exec></Actions>\n\
         </Task>\n"
    )
}

#[cfg(windows)]
pub fn is_autostart_enabled() -> bool {
    win_impl::is_autostart_enabled()
}

#[cfg(windows)]
pub fn set_autostart_enabled(enable: bool) -> Result<(), String> {
    win_impl::set_autostart_enabled(enable)
}

/// Automatically heals legacy `HKCU\...\Run\Tomoyo` entries into Task Scheduler `TomoyoAutostart`
/// (`HighestAvailable`) so Windows `RUNASADMIN` compat layers never cause silent `PID 0` boot skips.
#[cfg(windows)]
pub fn heal_windows_autostart_if_blocked() {
    win_impl::heal_windows_autostart_if_blocked();
}

#[cfg(not(windows))]
pub fn heal_windows_autostart_if_blocked() {}

#[cfg(target_os = "linux")]
fn linux_autostart_file() -> Option<PathBuf> {
    if let Ok(cfg) = std::env::var("XDG_CONFIG_HOME")
        && !cfg.trim().is_empty()
    {
        return Some(PathBuf::from(cfg.trim()).join("autostart").join("tomoyo.desktop"));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config").join("autostart").join("tomoyo.desktop"))
}

#[cfg(target_os = "linux")]
pub fn is_autostart_enabled() -> bool {
    let Some(path) = linux_autostart_file() else { return false; };
    let Ok(raw) = std::fs::read_to_string(path) else { return false; };
    !raw.lines().any(|l| {
        l.trim().eq_ignore_ascii_case("Hidden=true")
            || l.trim().eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
    })
}

#[cfg(target_os = "linux")]
pub fn set_autostart_enabled(enable: bool) -> Result<(), String> {
    let Some(path) = linux_autostart_file() else {
        return Err("HOME 未设置，无法写入自启动项".into());
    };
    if !enable {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("remove autostart: {e}"))?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create autostart dir: {e}"))?;
    }
    let exe = resolve_tomoyo_exe()?;
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Tomoyo\n\
         Comment=Low-memory open desktop launcher\nExec=\"{}\" --silent\nIcon=tomoyo\n\
         Terminal=false\nCategories=Utility;\nX-KDE-autostart-after=panel\n\
         X-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    std::fs::write(&path, desktop).map_err(|e| format!("write autostart: {e}"))
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn is_autostart_enabled() -> bool { false }

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn set_autostart_enabled(_enable: bool) -> Result<(), String> {
    let _ = resolve_tomoyo_exe();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_runasadmin_token_accurately() {
        assert!(is_runasadmin_flag_value("~ RUNASADMIN"));
        assert!(is_runasadmin_flag_value("HIGHDPIAWARE runasadmin"));
        assert!(!is_runasadmin_flag_value("~ HIGHDPIAWARE"));
        assert!(!is_runasadmin_flag_value("MY_RUNASADMIN_NOTE"));
    }

    #[test]
    fn builds_escaped_highest_available_task_xml() {
        let xml = build_autostart_task_xml(
            Path::new(r"C:\Apps\Tomoyo & Tools\tomoyo.exe"),
            Path::new(r"C:\Apps\Tomoyo & Tools"),
            Some(r"PC\User<1>"),
        );
        assert!(xml.contains("encoding=\"UTF-16\""));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<Arguments>--silent</Arguments>"));
        assert!(xml.contains("<Command>C:\\Apps\\Tomoyo &amp; Tools\\tomoyo.exe</Command>"));
        assert!(xml.contains("<UserId>PC\\User&lt;1&gt;</UserId>"));
        let bytes = encode_utf16le_bom(&xml);
        assert!(bytes.starts_with(&[0xFF, 0xFE]));
    }
}
