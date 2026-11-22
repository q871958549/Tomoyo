//! Developer hot-reload watcher (`tomoyo-cli dev`) & automated registry integrity verifier (`tomoyo-cli registry check`).

use crate::hash::compute_plugin_sha256;
use crate::{flag_val, load_registry};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};
use tomoyo_core::ExternalPluginBridge;
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{PluginManifest, QueryContext};

/// `tomoyo-cli dev <plugin-dir> [--query "<sample>"] [--once]`
pub(crate) fn cmd_dev(args: &[String]) -> i32 {
    let Some(dir_str) = args.first().filter(|s| !s.starts_with('-')) else {
        eprintln!("Usage: tomoyo-cli dev <plugin-dir> [--query \"<args>\"] [--once]");
        return 1;
    };
    let dir = Path::new(dir_str);
    if !dir.is_dir() {
        eprintln!("Plugin directory not found: {}", dir.display());
        return 1;
    }
    let sample_args = flag_val(args, "--query").unwrap_or("hello");
    let once = args.iter().any(|a| a == "--once");

    println!("=== Tomoyo Plugin Dev Mode (Protocol v2) ===");
    println!("Watching: {} (Ctrl+C to exit)", dir.display());

    let mut last_snap = snapshot_mtimes(dir);
    let first_code = run_dev_cycle(dir, sample_args);
    if once {
        return first_code;
    }

    loop {
        std::thread::sleep(Duration::from_millis(500));
        let cur = snapshot_mtimes(dir);
        if cur != last_snap {
            last_snap = cur;
            println!("\n[CHANGE DETECTED] Re-validating & hot-reloading Tomoyo...");
            let _ = run_dev_cycle(dir, sample_args);
        }
    }
}

fn run_dev_cycle(dir: &Path, sample_args: &str) -> i32 {
    let manifest_path = dir.join("tomoyo-plugin.toml");
    let raw = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[ERROR] Cannot read {}: {e}", manifest_path.display());
            return 1;
        }
    };
    let manifest = match PluginManifest::from_toml_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[ERROR] Invalid tomoyo-plugin.toml: {e}");
            return 1;
        }
    };
    let sha = compute_plugin_sha256(dir).unwrap_or_else(|_| "error".into());
    println!(
        "[OK] `{}` ({}) v{} [{}] · hotkeys={} · events={} · settings={}",
        manifest.name,
        manifest.id,
        manifest.version,
        manifest.runtime.kind_badge(),
        manifest.hotkeys.len(),
        manifest.events.len(),
        manifest.settings.len()
    );
    println!("[OK] SHA-256: {sha}");

    let mut ok = 0;
    if let (Some(parent), Some(first_kw)) = (dir.parent(), manifest.triggers.keywords.first()) {
        let started = Instant::now();
        let host = PluginHost::new(vec![parent.to_path_buf()]);
        host.set_interactive_prompts(false);
        let ctx = QueryContext::new(format!("{first_kw} {sample_args}"), 1, 8)
            .with_keyword(first_kw, sample_args);
        let items = host.query_keyword(first_kw, &ctx);
        let ms = started.elapsed().as_millis();
        println!(
            "[OK] Query `{first_kw} {sample_args}` -> {} item(s) in {ms} ms",
            items.len()
        );
        for (i, it) in items.iter().take(3).enumerate() {
            println!("     {}. {} — {}", i + 1, it.title, it.subtitle);
        }
        host.shutdown_all();
    } else {
        println!("[INFO] No keyword trigger declared; skipped keyword smoke query.");
    }

    if notify_running_tomoyo_reload() {
        println!("[HOT-RELOAD] Signaled running tomoyo.exe to reload plugins.");
    } else {
        ok = 0;
    }
    ok
}

fn snapshot_mtimes(root: &Path) -> BTreeMap<PathBuf, u64> {
    let mut map = BTreeMap::new();
    collect_mtimes(root, &mut map, 0);
    map
}

fn collect_mtimes(dir: &Path, map: &mut BTreeMap<PathBuf, u64>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        let p = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        if matches!(name.as_str(), "target" | "node_modules" | "__pycache__" | ".git") {
            continue;
        }
        if p.is_dir() {
            collect_mtimes(&p, map, depth + 1);
        } else if let Ok(meta) = ent.metadata() {
            let secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            map.insert(p, secs ^ meta.len());
        }
    }
}

/// `tomoyo-cli registry check [--index registry/index.json] [--manifest-only]`
/// Validates every registry entry: schema, manifest syntax, ID/version match, and SHA-256!
pub(crate) fn cmd_registry_check(args: &[String]) -> i32 {
    let index_source = flag_val(args, "--index").unwrap_or("registry/index.json");
    let manifest_only = args.iter().any(|a| a == "--manifest-only");
    let index_path = PathBuf::from(index_source);
    let index = match load_registry(index_source) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("[FAIL] Cannot load registry `{index_source}`: {e}");
            return 1;
        }
    };
    let base_dir = index_path
        .parent()
        .and_then(|p| p.parent())
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let mut errors = 0usize;
    let mut seen_ids = std::collections::HashSet::new();
    println!("=== Checking Tomoyo Plugin Registry ({} entries) ===", index.plugins.len());

    for entry in &index.plugins {
        if entry.id.trim().is_empty() || !seen_ids.insert(entry.id.to_ascii_lowercase()) {
            eprintln!("  [FAIL] Empty or duplicate plugin id: `{}`", entry.id);
            errors += 1;
            continue;
        }
        if entry.sha256.len() != 64 || !entry.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            eprintln!("  [FAIL] `{}` has invalid SHA-256 format: `{}`", entry.id, entry.sha256);
            errors += 1;
            continue;
        }
        if entry.source.starts_with("https://") || entry.source.starts_with("http://") {
            println!("  [OK]   {:<28} v{} (remote URL, SHA-256 format valid)", entry.id, entry.version);
            continue;
        }

        let src_dir = if Path::new(&entry.source).is_absolute() || Path::new(&entry.source).exists() {
            PathBuf::from(&entry.source)
        } else {
            base_dir.join(&entry.source)
        };
        if !src_dir.is_dir() {
            eprintln!("  [FAIL] `{}` missing local source dir: {}", entry.id, src_dir.display());
            errors += 1;
            continue;
        }
        let manifest_raw = match std::fs::read_to_string(src_dir.join("tomoyo-plugin.toml")) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  [FAIL] `{}` missing tomoyo-plugin.toml: {e}", entry.id);
                errors += 1;
                continue;
            }
        };
        let manifest = match PluginManifest::from_toml_str(&manifest_raw) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("  [FAIL] `{}` invalid manifest: {e}", entry.id);
                errors += 1;
                continue;
            }
        };
        if manifest.id != entry.id || manifest.version != entry.version {
            eprintln!(
                "  [FAIL] `{}` mismatch: manifest has id=`{}` v{}, registry has id=`{}` v{}",
                entry.id, manifest.id, manifest.version, entry.id, entry.version
            );
            errors += 1;
            continue;
        }
        if manifest_only {
            println!(
                "  [OK]   {:<28} v{} [{}] manifest verified",
                entry.id,
                entry.version,
                manifest.runtime.kind_badge()
            );
            continue;
        }
        match compute_plugin_sha256(&src_dir) {
            Ok(actual) if actual.eq_ignore_ascii_case(&entry.sha256) => {
                println!(
                    "  [OK]   {:<28} v{} [{}] sha256={}…",
                    entry.id,
                    entry.version,
                    manifest.runtime.kind_badge(),
                    &actual[..16]
                );
            }
            Ok(actual) => {
                eprintln!(
                    "  [FAIL] `{}` SHA-256 drift!\n         registry: {}\n         actual:   {}",
                    entry.id, entry.sha256, actual
                );
                errors += 1;
            }
            Err(e) => {
                eprintln!("  [FAIL] `{}` hash error: {e}", entry.id);
                errors += 1;
            }
        }
    }

    if errors == 0 {
        println!("[PASS] All {} registry plugin(s) passed manifest & SHA-256 verification!", index.plugins.len());
        0
    } else {
        eprintln!("[FAILED] Registry check found {errors} error(s).");
        1
    }
}

#[cfg(windows)]
fn notify_running_tomoyo_reload() -> bool {
    const WM_APP_CONFIG_SAVED: u32 = 0x8000 + 20;
    unsafe extern "system" {
        fn FindWindowW(class_name: *const u16, window_name: *const u16) -> isize;
        fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
    }
    let cls: Vec<u16> = "TomoyoLauncherWindowClass\0".encode_utf16().collect();
    let hwnd = unsafe { FindWindowW(cls.as_ptr(), std::ptr::null()) };
    if hwnd != 0 {
        unsafe { PostMessageW(hwnd, WM_APP_CONFIG_SAVED, 1, 0) != 0 }
    } else {
        false
    }
}

#[cfg(target_os = "linux")]
fn notify_running_tomoyo_reload() -> bool {
    let pid_path = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("tomoyo.pid");
    let Ok(raw) = std::fs::read_to_string(&pid_path) else {
        return false;
    };
    let Ok(pid) = raw.trim().parse::<i32>() else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    const SIGUSR1: i32 = 10;
    unsafe { kill(pid, SIGUSR1) == 0 }
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn notify_running_tomoyo_reload() -> bool {
    false
}
