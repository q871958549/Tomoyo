//! Developer hot-reload watcher (`tomoyo-cli dev`) & automated registry integrity verifier (`tomoyo-cli registry check`).

use crate::hash::compute_plugin_sha256;
use crate::verify::{resolve_plugin_dir, validate_plugin_dir};
use crate::{flag_val, load_registry};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tomoyo_core::ExternalPluginBridge;
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{
    ExecuteRequest, PluginEventKind, PluginEventPayload, PluginManifest, QueryContext, RuntimeConfig,
};

/// `tomoyo-cli dev <plugin-dir> [--query "<args>"] [--build] [--exec <id>] [--event <kind>] [--once]`
pub(crate) fn cmd_dev(args: &[String]) -> i32 {
    let Some(dir_str) = args.first().filter(|s| !s.starts_with('-')) else {
        eprintln!(
            "Usage: tomoyo-cli dev <plugin-dir> [--query \"<args>\"] [--build] [--exec <action_id>] [--event <hotkey_triggered|clipboard_text|clipboard_image>] [--data <text>] [--once]"
        );
        return 1;
    };
    let canon = resolve_plugin_dir(Path::new(dir_str));
    if !canon.is_dir() {
        eprintln!("Plugin directory not found: {}", canon.display());
        return 1;
    }
    let sample_args = flag_val(args, "--query").unwrap_or("hello");
    let once = args.iter().any(|a| a == "--once");
    let build_first = args.iter().any(|a| a == "--build");
    let exec_action = flag_val(args, "--exec");
    let event_kind = flag_val(args, "--event");
    let event_data = flag_val(args, "--data").unwrap_or(sample_args);

    println!("=== Tomoyo Plugin Dev Mode (Protocol v2) ===");
    println!("Watching: {} (Ctrl+C to exit)", canon.display());

    let mut last_snap = snapshot_mtimes(&canon);
    let first_code = run_dev_cycle(&canon, sample_args, build_first, exec_action, event_kind, event_data);
    if once {
        return first_code;
    }

    loop {
        std::thread::sleep(Duration::from_millis(500));
        let cur = snapshot_mtimes(&canon);
        if cur != last_snap {
            last_snap = cur;
            println!("\n[CHANGE DETECTED] Rebuilding & hot-reloading Tomoyo...");
            let auto_build = build_first || canon.join("Cargo.toml").is_file();
            let _ = run_dev_cycle(&canon, sample_args, auto_build, exec_action, event_kind, event_data);
        }
    }
}

fn try_build_plugin_cargo(dir: &Path, manifest: Option<&PluginManifest>) -> Result<(), String> {
    if !dir.join("Cargo.toml").is_file() {
        return Ok(());
    }
    let is_wasm = manifest.is_some_and(|m| matches!(m.runtime, RuntimeConfig::Wasm { .. }));
    let mut cmd = Command::new("cargo");
    cmd.current_dir(dir);
    if is_wasm {
        cmd.args(["build", "--target", "wasm32-unknown-unknown", "--release"]);
    } else {
        cmd.args(["build", "--release"]);
    }
    println!("[BUILD] Running `{}`...", if is_wasm { "cargo build --target wasm32-unknown-unknown --release" } else { "cargo build --release" });
    let status = cmd.status().map_err(|e| format!("failed to run cargo: {e}"))?;
    if !status.success() {
        return Err("cargo build failed".into());
    }
    if is_wasm
        && let Some(PluginManifest { runtime: RuntimeConfig::Wasm { module, .. }, .. }) = manifest
    {
        let rel_dir = dir.join("target").join("wasm32-unknown-unknown").join("release");
        if let Some(built) = std::fs::read_dir(&rel_dir)
            .ok()
            .and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.extension().and_then(|e| e.to_str()) == Some("wasm")))
        {
            std::fs::copy(&built, dir.join(module))
                .map_err(|e| format!("failed copying built WASM to {module}: {e}"))?;
            println!("[BUILD] Updated {}", dir.join(module).display());
        }
    } else if !is_wasm
        && let Some(PluginManifest { runtime: RuntimeConfig::StdioJsonrpc { command, .. }, .. }) = manifest
    {
        let bin_name = Path::new(command).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let exe_suf = std::env::consts::EXE_SUFFIX;
        let cand_name = if !exe_suf.is_empty() && !bin_name.ends_with(exe_suf) {
            format!("{bin_name}{exe_suf}")
        } else {
            bin_name.clone()
        };
        let built = dir.join("target").join("release").join(&cand_name);
        if built.is_file() {
            let dst = dir.join(if command.contains('/') || command.contains('\\') { Path::new(command) } else { Path::new(&cand_name) });
            let _ = std::fs::copy(&built, &dst);
            println!("[BUILD] Updated {}", dst.display());
        }
    }
    Ok(())
}

fn run_dev_cycle(
    canon: &Path,
    sample_args: &str,
    build: bool,
    exec_action: Option<&str>,
    event_kind: Option<&str>,
    event_data: &str,
) -> i32 {
    let manifest_raw = match std::fs::read_to_string(canon.join("tomoyo-plugin.toml")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[ERROR] Cannot read tomoyo-plugin.toml: {e}");
            return 1;
        }
    };
    let parsed_hint = PluginManifest::from_toml_str(&manifest_raw).ok();
    if build && let Err(e) = try_build_plugin_cargo(canon, parsed_hint.as_ref()) {
        eprintln!("[ERROR] {e}");
        return 1;
    }

    let (_, manifest, warnings) = match validate_plugin_dir(canon) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[ERROR] {e}");
            return 1;
        }
    };
    for w in warnings {
        println!("[WARN] {w}");
    }

    let sha = compute_plugin_sha256(canon).unwrap_or_else(|_| "error".into());
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

    if let Some(parent) = canon.parent() {
        let host = PluginHost::new(vec![parent.to_path_buf()]);
        host.set_interactive_prompts(false);

        if let Some(first_kw) = manifest.triggers.keywords.first() {
            let started = Instant::now();
            let ctx = QueryContext::new(format!("{first_kw} {sample_args}"), 1, 8)
                .with_keyword(first_kw, sample_args);
            let items: Vec<_> = host
                .query_keyword(first_kw, &ctx)
                .into_iter()
                .filter(|it| it.provider_id == manifest.id)
                .collect();
            let ms = started.elapsed().as_millis();
            println!("[OK] Query `{first_kw} {sample_args}` -> {} item(s) in {ms} ms", items.len());
            for (i, it) in items.iter().take(3).enumerate() {
                println!("     {}. {} — {}", i + 1, it.title, it.subtitle);
            }
        } else {
            println!("[INFO] No keyword trigger declared; skipped keyword smoke query.");
        }

        if let Some(act_id) = exec_action {
            let req = ExecuteRequest {
                action_id: act_id.to_string(),
                item_id: format!("{}:dev", manifest.id),
                payload: serde_json::json!({ "text": event_data }),
            };
            match host.execute_action(&manifest.id, req) {
                Ok(resp) => println!("[OK] Execute `{act_id}` -> hide={}, toast={:?}, copy={:?}", resp.hide_window, resp.toast, resp.copy_text),
                Err(e) => {
                    eprintln!("[ERROR] Execute `{act_id}` failed: {e}");
                    host.shutdown_all();
                    return 1;
                }
            }
        }

        if let Some(ev_name) = event_kind {
            if let Some(kind) = parse_event_kind(ev_name) {
                if !manifest.events.contains(&kind) {
                    println!("[WARN] `{}` does not list `{ev_name}` in `events = [...]`; add it in tomoyo-plugin.toml to receive runtime events.", manifest.id);
                }
                let payload = PluginEventPayload { event: kind, data: serde_json::json!({ "text": event_data }) };
                let resps = host.dispatch_event(Some(&manifest.id), &payload);
                println!("[OK] Event `{ev_name}` -> {} response(s): {:?}", resps.len(), resps.first().and_then(|r| r.toast.as_deref()));
            } else {
                eprintln!("[WARN] Unknown event `{ev_name}` (expected hotkey_triggered, clipboard_text, or clipboard_image)");
            }
        }

        host.shutdown_all();
    }

    if notify_running_tomoyo_reload() {
        println!("[HOT-RELOAD] Signaled running Tomoyo instance to reload plugins.");
    }
    0
}

fn parse_event_kind(raw: &str) -> Option<PluginEventKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "hotkey_triggered" | "hotkey" => Some(PluginEventKind::HotkeyTriggered),
        "clipboard_text" | "text" => Some(PluginEventKind::ClipboardText),
        "clipboard_image" | "image" => Some(PluginEventKind::ClipboardImage),
        _ => None,
    }
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
        if matches!(name.as_str(), "target" | "node_modules" | "__pycache__" | ".git" | "dist") {
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
        let manifest = match validate_plugin_dir(&src_dir) {
            Ok((_, m, _)) => m,
            Err(e) => {
                eprintln!("  [FAIL] `{}`: {e}", entry.id);
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
            println!("  [OK]   {:<28} v{} [{}] manifest verified", entry.id, entry.version, manifest.runtime.kind_badge());
            continue;
        }
        match compute_plugin_sha256(&src_dir) {
            Ok(actual) if actual.eq_ignore_ascii_case(&entry.sha256) => {
                println!("  [OK]   {:<28} v{} [{}] sha256={}…", entry.id, entry.version, manifest.runtime.kind_badge(), &actual[..16]);
            }
            Ok(actual) => {
                eprintln!("  [FAIL] `{}` SHA-256 drift!\n         registry: {}\n         actual:   {}", entry.id, entry.sha256, actual);
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
