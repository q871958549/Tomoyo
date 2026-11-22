//! Isolated Linux child-process launcher (`systemd-run --user` transient app units
//! with `ExitType=cgroup` and `setsid` fallback) so launched apps never inherit Tomoyo's cgroup.

use std::ffi::c_int;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

unsafe extern "C" {
    fn setsid() -> c_int;
}

static LAUNCH_SEQ: AtomicU64 = AtomicU64::new(1);

/// Returns `true` if `cmd` resolves to an existing executable regular file (directly or via `PATH`).
pub fn command_in_path(cmd: &str) -> bool {
    resolve_executable(cmd).is_some()
}

/// Resolves `cmd` to an executable regular file path, checking direct path or `PATH` entries.
pub fn resolve_executable(cmd: &str) -> Option<PathBuf> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = Path::new(trimmed);
    if trimmed.contains('/') {
        return is_executable_file(p).then(|| p.to_path_buf());
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(trimmed))
        .find(|cand| is_executable_file(cand))
}

fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && (meta.permissions().mode() & 0o111) != 0)
}

/// Sanitizes an arbitrary identifier (such as a plugin ID) into a safe single-segment token
/// shared by Unix socket paths in `tomoyo-app` and `tomoyo-webview-host`.
pub fn sanitize_instance_tag(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect()
}

/// Sanitizes a `.desktop` path or executable name into a systemd `<ApplicationID>` token.
pub fn sanitize_unit_id(raw: &str) -> String {
    let p = Path::new(raw.trim());
    let base = if p
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("desktop"))
    {
        p.file_stem().and_then(|s| s.to_str()).unwrap_or(raw)
    } else {
        p.file_name().and_then(|s| s.to_str()).unwrap_or(raw)
    };

    let mut out = String::with_capacity(base.len().min(48));
    for ch in base.chars() {
        if ch == '-' {
            if out.len() + 4 > 48 {
                break;
            }
            out.push_str(r"\x2d");
            continue;
        }
        if out.len() >= 48 {
            break;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':') {
            out.push(ch);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "launch".into()
    } else {
        trimmed.to_string()
    }
}

/// Builds a collision-free transient unit name following systemd's Desktop Environments spec.
pub fn systemd_app_unit_name(hint: Option<&str>) -> String {
    let seq = LAUNCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let app_id = hint
        .map(sanitize_unit_id)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "launch".into());
    // Use `_` in the `@<instance>` suffix so KDE libprocesscore's greedy `(?:[^-]*-)?`
    // prefix matcher never consumes part of `<app_id>` as a launcher prefix.
    format!("app-{app_id}@tomoyo_{pid}_{seq}.service")
}

/// Launches `prog` inside an isolated `systemd-run --user` transient service (`app.slice`,
/// `ExitType=cgroup`) so launched applications never stay in Tomoyo's cgroup.
pub fn spawn_isolated(
    prog: &str,
    args: &[String],
    workdir: Option<&str>,
    unit_hint: Option<&str>,
) -> bool {
    let Some(resolved) = resolve_executable(prog) else {
        return false;
    };
    if spawn_via_systemd_run(&resolved, args, workdir, unit_hint.unwrap_or(prog)) {
        return true;
    }
    spawn_detached(&resolved, args, workdir)
}

fn spawn_via_systemd_run(
    prog_path: &Path,
    args: &[String],
    workdir: Option<&str>,
    label_hint: &str,
) -> bool {
    let Some(systemd_run) = resolve_executable("systemd-run") else {
        return false;
    };
    let unit = systemd_app_unit_name(Some(label_hint));
    let display_hint = Path::new(label_hint)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("desktop"))
        .then(|| Path::new(label_hint).file_stem().and_then(|s| s.to_str()))
        .flatten()
        .unwrap_or(label_hint);
    let clean_label: String = display_hint
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(64)
        .collect();
    let mut cmd = Command::new(systemd_run);
    cmd.args([
        "--user",
        "--collect",
        "--quiet",
        "--slice=app.slice",
        "--service-type=exec",
        "--property=ExitType=cgroup",
        &format!("--unit={unit}"),
        &format!("--description={clean_label}"),
    ]);
    if let Some(d) = workdir.filter(|d| Path::new(d).is_dir()) {
        cmd.arg(format!("--working-directory={d}"));
    }
    for var in ["DISPLAY", "WAYLAND_DISPLAY"] {
        if let Ok(val) = std::env::var(var)
            && !val.trim().is_empty()
        {
            cmd.arg(format!("--setenv={var}={val}"));
        }
    }
    cmd.arg("--").arg(prog_path).args(args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.status().is_ok_and(|s| s.success())
}

fn spawn_detached(prog_path: &Path, args: &[String], workdir: Option<&str>) -> bool {
    let mut cmd = Command::new(prog_path);
    cmd.args(args);
    if let Some(d) = workdir.filter(|d| Path::new(d).is_dir()) {
        cmd.current_dir(d);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Detach into its own session when systemd --user is unavailable (non-systemd desktops).
    unsafe {
        cmd.pre_exec(|| {
            let _ = setsid();
            Ok(())
        });
    }
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let _ = std::thread::Builder::new()
        .name("tomoyo-detach-reap".into())
        .spawn(move || {
            let _ = child.wait();
        });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_desktop_and_binary_unit_ids() {
        assert_eq!(
            sanitize_unit_id("/usr/share/applications/steam.desktop"),
            "steam"
        );
        assert_eq!(
            sanitize_unit_id("/usr/share/applications/org.kde.konsole.desktop"),
            "org.kde.konsole"
        );
        assert_eq!(
            sanitize_unit_id("plasma-systemmonitor"),
            r"plasma\x2dsystemmonitor"
        );
        assert_eq!(sanitize_unit_id("   "), "launch");
    }

    #[test]
    fn generates_unique_app_service_names() {
        let a = systemd_app_unit_name(Some("/usr/share/applications/steam.desktop"));
        let b = systemd_app_unit_name(Some("/usr/share/applications/steam.desktop"));
        assert!(a.starts_with("app-steam@tomoyo_"));
        assert!(a.ends_with(".service"));
        assert!(!a.split_once('@').unwrap().1.contains('-'));
        assert_ne!(a, b);
    }

    #[test]
    fn sanitizes_instance_tags() {
        assert_eq!(sanitize_instance_tag("dev.tomoyo.json/1"), "dev.tomoyo.json_1");
        assert_eq!(sanitize_instance_tag(&"a".repeat(80)).len(), 48);
    }

    #[test]
    fn non_executable_file_is_not_treated_as_command() {
        assert!(!command_in_path("Cargo.toml"));
        assert!(command_in_path("sh"));
    }

    #[test]
    fn spawn_isolated_executes_and_isolates_cgroup() {
        let marker = std::env::temp_dir().join(format!(
            "tomoyo_iso_test_{}_{}.txt",
            std::process::id(),
            LAUNCH_SEQ.load(Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&marker);
        let script = format!("cat /proc/self/cgroup > '{}'", marker.display());
        let ok = spawn_isolated(
            "sh",
            &["-c".into(), script],
            None,
            Some("tomoyo-test-iso"),
        );
        assert!(ok, "spawn_isolated should launch sh cleanly");

        let mut content = String::new();
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(&marker)
                && !s.is_empty()
            {
                content = s;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(&marker);
        assert!(!content.is_empty(), "isolated child should write cgroup info");
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
            && resolve_executable("systemd-run").is_some()
        {
            let parent_cg = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
            if parent_cg.contains("user@") {
                assert_ne!(content.trim(), parent_cg.trim());
                assert!(content.contains(r"app-tomoyo\x2dtest\x2diso@tomoyo_"));
            }
        }
    }
}
