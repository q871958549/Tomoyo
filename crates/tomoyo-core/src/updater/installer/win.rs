//! Windows self-update launcher that breaks away from the WebView Job Object and logs cleanly.

use crate::diagnostics::{format_beijing_now, logs_dir};
use crate::pkg_zip::unpack_zip_to_dir;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
const MAX_UPDATE_LOG_BYTES: u64 = 512 * 1024;

const COMMON_PS1_PRELUDE: &str = r#"function Write-UpdateLog([string]$Msg) {
  if (-not $LogFile) { return }
  try {
    $ts = (Get-Date).ToUniversalTime().AddHours(8).ToString('yyyy-MM-dd HH:mm:ss.fff +08:00')
    Add-Content -LiteralPath $LogFile -Value "$ts [ps1] $Msg" -Encoding UTF8 -ErrorAction SilentlyContinue
  } catch {}
}
Write-UpdateLog "Updater helper started (PID=$PID)"
Start-Sleep -Milliseconds 500
$targets = @('tomoyo-webview-host','tomoyo','tomoyo-capture','tomoyo-plugin-ocr','tomoyo-plugin-notes','tomoyo-plugin-md-editor','tomoyo-plugin-translate','tomoyo-plugin-capture','tomoyo-plugin-clipboard','tomoyo-plugin-hello','tomoyo-cli')
foreach ($name in $targets) {
  $waitSec = if ($name -eq 'tomoyo') { 4.0 } else { 2.0 }
  $deadline = (Get-Date).AddSeconds($waitSec)
  while ((Get-Process -Name $name -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 120 }
  $rem = Get-Process -Name $name -ErrorAction SilentlyContinue
  if ($rem) {
    Write-UpdateLog "Force-stopping remaining $name (count=$($rem.Count))"
    $rem | Stop-Process -Force -ErrorAction SilentlyContinue
  }
}
Start-Sleep -Milliseconds 250"#;

pub(super) fn append_update_log(msg: &str) {
    let path = logs_dir().join("update.log");
    if let Ok(meta) = std::fs::metadata(&path)
        && meta.len() > MAX_UPDATE_LOG_BYTES
    {
        let _ = std::fs::rename(&path, logs_dir().join("update.1.log"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {msg}", format_beijing_now());
    }
}

fn is_dev_build_path(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    lower.contains("\\target\\debug\\") || lower.contains("\\target\\release\\")
}

pub(super) fn resolve_target_exes() -> (PathBuf, PathBuf, PathBuf) {
    let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("tomoyo.exe"));
    let exe_dir = current_exe
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let local_exe = if current_exe
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("tomoyo.exe"))
    {
        current_exe
    } else {
        exe_dir.join("tomoyo.exe")
    };
    let default_setup_exe = crate::data_dir()
        .parent()
        .map(|l| l.join("Programs").join("Tomoyo").join("tomoyo.exe"))
        .unwrap_or_else(|| local_exe.clone());
    if is_dev_build_path(&local_exe) {
        (exe_dir, default_setup_exe.clone(), local_exe)
    } else {
        (exe_dir, local_exe, default_setup_exe)
    }
}

fn build_helper_ps1_body(is_portable: bool, is_msi: bool) -> String {
    if is_portable {
        let params = r#"param([string]$StageDir,[string]$InstallDir,[string]$TargetExe,[string]$FallbackExe,[string]$ArchivePath,[string]$LogFile)"#;
        let body = r#"$launch = if (Test-Path -LiteralPath $TargetExe) { $TargetExe } else { $FallbackExe }
try {
  Copy-Item -Path (Join-Path $StageDir '*') -Destination $InstallDir -Recurse -Force
  Remove-Item -LiteralPath $StageDir -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $ArchivePath -Force -ErrorAction SilentlyContinue
  Write-UpdateLog "Portable files updated; launching $launch --updated"
  if ($launch -and (Test-Path -LiteralPath $launch)) { Start-Process -FilePath $launch -ArgumentList '--updated' }
} catch {
  Write-UpdateLog "Portable update failed: $($_.Exception.Message)"
  if ($launch -and (Test-Path -LiteralPath $launch)) { Start-Process -FilePath $launch -ArgumentList '--update-failed' }
}"#;
        return format!("{params}\n{COMMON_PS1_PRELUDE}\n{body}\n");
    }

    let params = r#"param([string]$Installer,[string]$TargetExe,[string]$FallbackExe,[string]$LogFile,[string]$InnoLog)"#;
    let install_step = if is_msi {
        r#"$exitCode = -1
try {
  Write-UpdateLog "Launching MSI: $Installer"
  $p = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', "`"$Installer`"", '/qn', '/norestart', '/l*v', "`"$InnoLog`"") -Wait -PassThru
  $exitCode = $p.ExitCode
  Write-UpdateLog "Installer finished ExitCode=$exitCode"
} catch {
  Write-UpdateLog "MSI launch error: $($_.Exception.Message)"
}"#
    } else {
        r#"$exitCode = -1
$setupArgs = @('/SP-','/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/CLOSEAPPLICATIONS',"/LOG=`"$InnoLog`"")
try {
  Write-UpdateLog "Launching Inno Setup: $Installer $($setupArgs -join ' ')"
  $p = Start-Process -FilePath $Installer -ArgumentList $setupArgs -Wait -PassThru
  $exitCode = $p.ExitCode
  Write-UpdateLog "Installer finished ExitCode=$exitCode"
} catch {
  Write-UpdateLog "Setup launch error: $($_.Exception.Message)"
}"#
    };
    let finish = r#"if ($exitCode -eq 0) {
  Remove-Item -LiteralPath $Installer -Force -ErrorAction SilentlyContinue
  Remove-Item -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomoyo' -Recurse -Force -ErrorAction SilentlyContinue
}
$launch = if ($TargetExe -and (Test-Path -LiteralPath $TargetExe)) { $TargetExe } elseif ($FallbackExe -and (Test-Path -LiteralPath $FallbackExe)) { $FallbackExe } else { $TargetExe }
$exists = [bool]($launch -and (Test-Path -LiteralPath $launch))
$arg = if ($exitCode -eq 0) { '--updated' } else { '--update-failed' }
if ($exists -and -not (Get-Process -Name 'tomoyo' -ErrorAction SilentlyContinue)) {
  Write-UpdateLog "Relaunching $launch $arg"
  Start-Process -FilePath $launch -ArgumentList $arg
} else {
  Write-UpdateLog "Skipped relaunch (launch=$launch, exists=$exists)"
}"#;
    format!("{params}\n{COMMON_PS1_PRELUDE}\n{install_step}\n{finish}\n")
}

fn write_update_helper_ps1(is_portable: bool, is_msi: bool) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("tomoyo-update");
    let _ = std::fs::create_dir_all(&dir);
    let script_path = dir.join(if is_portable {
        "apply_portable_update.ps1"
    } else {
        "apply_setup_update.ps1"
    });
    let body = build_helper_ps1_body(is_portable, is_msi);
    std::fs::write(&script_path, body).map_err(|e| format!("write update helper failed: {e}"))?;
    Ok(script_path)
}

fn quote_win_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(ch);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

fn spawn_cim_win32_process(ps_args: &[&str]) -> Result<(), String> {
    let mut cmdline = String::from("powershell.exe");
    for arg in ps_args {
        cmdline.push(' ');
        cmdline.push_str(&quote_win_arg(arg));
    }
    let escaped = cmdline.replace('\'', "''");
    let ps_cmd = format!(
        "$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{{ CommandLine = '{escaped}' }}; if (-not $r -or $r.ReturnValue -ne 0) {{ throw \"Win32_Process.Create failed: $($r.ReturnValue)\" }}"
    );
    let out = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &ps_cmd,
        ])
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("CIM wrapper spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("CIM Win32_Process.Create error: {}", stderr.trim()));
    }
    Ok(())
}

fn spawn_detached_powershell(ps_args: &[&str], err_prefix: &str) -> Result<(), String> {
    let breakaway = Command::new("powershell.exe")
        .args(ps_args)
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
        .spawn();
    match breakaway {
        Ok(child) => {
            append_update_log(&format!(
                "Spawned updater powershell with CREATE_BREAKAWAY_FROM_JOB (pid={})",
                child.id()
            ));
            Ok(())
        }
        Err(e) if e.raw_os_error() == Some(5) => {
            append_update_log(
                "CREATE_BREAKAWAY_FROM_JOB denied (errno=5); falling back to CIM Win32_Process.Create",
            );
            if let Err(cim_err) = spawn_cim_win32_process(ps_args) {
                append_update_log(&format!(
                    "CIM fallback failed ({cim_err}); falling back to direct powershell spawn"
                ));
                let child = Command::new("powershell.exe")
                    .args(ps_args)
                    .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
                    .spawn()
                    .map_err(|e2| format!("{err_prefix}: {e2}"))?;
                append_update_log(&format!("Spawned fallback powershell (pid={})", child.id()));
            } else {
                append_update_log("Spawned updater powershell via CIM Win32_Process.Create");
            }
            Ok(())
        }
        Err(e) => Err(format!("{err_prefix}: {e}")),
    }
}

pub(super) fn launch_windows_update(installer_path: &Path) -> Result<bool, String> {
    let ext = installer_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (exe_dir, preferred_exe, fallback_exe) = resolve_target_exes();
    let log_dir = logs_dir();
    let update_log = log_dir.join("update.log");
    let inno_log = log_dir.join("inno_setup.log");

    append_update_log(&format!(
        "Preparing update install (ext={ext}, installer={}, preferred={}, fallback={})",
        installer_path.display(),
        preferred_exe.display(),
        fallback_exe.display()
    ));

    if ext == "zip" {
        let stage_dir = std::env::temp_dir().join("tomoyo-update").join("portable_stage");
        let _ = std::fs::remove_dir_all(&stage_dir);
        unpack_zip_to_dir(installer_path, &stage_dir)?;
        if !stage_dir.join("tomoyo.exe").is_file() {
            return Err("便携压缩包中未包含 tomoyo.exe，已取消覆盖".into());
        }
        let script = write_update_helper_ps1(true, false)?;
        let script_s = script.to_string_lossy().into_owned();
        let stage_s = stage_dir.to_string_lossy().into_owned();
        let dir_s = exe_dir.to_string_lossy().into_owned();
        let pref_s = preferred_exe.to_string_lossy().into_owned();
        let fall_s = fallback_exe.to_string_lossy().into_owned();
        let arch_s = installer_path.to_string_lossy().into_owned();
        let log_s = update_log.to_string_lossy().into_owned();
        let args = [
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script_s,
            "-StageDir",
            &stage_s,
            "-InstallDir",
            &dir_s,
            "-TargetExe",
            &pref_s,
            "-FallbackExe",
            &fall_s,
            "-ArchivePath",
            &arch_s,
            "-LogFile",
            &log_s,
        ];
        spawn_detached_powershell(&args, "拉起便携更新器失败")?;
        return Ok(true);
    }

    if ext == "exe" || ext == "msi" {
        let script = write_update_helper_ps1(false, ext == "msi")?;
        let script_s = script.to_string_lossy().into_owned();
        let inst_s = installer_path.to_string_lossy().into_owned();
        let pref_s = preferred_exe.to_string_lossy().into_owned();
        let fall_s = fallback_exe.to_string_lossy().into_owned();
        let log_s = update_log.to_string_lossy().into_owned();
        let inno_s = inno_log.to_string_lossy().into_owned();
        let args = [
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script_s,
            "-Installer",
            &inst_s,
            "-TargetExe",
            &pref_s,
            "-FallbackExe",
            &fall_s,
            "-LogFile",
            &log_s,
            "-InnoLog",
            &inno_s,
        ];
        spawn_detached_powershell(&args, "拉起安装更新器失败")?;
        return Ok(true);
    }

    let _ = crate::file_manager::reveal_file_in_file_manager(&installer_path.to_string_lossy());
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps1_templates_cover_setup_portable_msi_and_plugin_cleanup() {
        let setup = build_helper_ps1_body(false, false);
        assert!(setup.contains("/SUPPRESSMSGBOXES"));
        assert!(setup.contains("/VERYSILENT"));
        assert!(setup.contains("tomoyo-capture"));
        assert!(setup.contains("tomoyo-plugin-ocr"));
        assert!(setup.contains("tomoyo-plugin-md-editor"));
        assert!(setup.contains(r"Uninstall\Tomoyo"));
        assert!(setup.contains("Write-UpdateLog"));

        let portable = build_helper_ps1_body(true, false);
        assert!(portable.contains("Copy-Item"));
        assert!(portable.contains("tomoyo-plugin-notes"));

        let msi = build_helper_ps1_body(false, true);
        assert!(msi.contains("msiexec.exe"));
    }

    #[test]
    fn quote_win_arg_handles_spaces_and_quotes() {
        assert_eq!(quote_win_arg("simple"), "simple");
        assert_eq!(
            quote_win_arg(r#"C:\Users\Admin User\update.log"#),
            r#""C:\Users\Admin User\update.log""#
        );
        assert_eq!(quote_win_arg(r#"a"b"#), r#""a\"b""#);
    }
}
