//! Decentralized sharded registry loader & recursive integrity verifier (`tomoyo-cli registry check`).

use crate::hash::compute_plugin_sha256;
pub(crate) use crate::registry_shards::build_registry_shards;
use crate::verify::validate_plugin_dir;
use crate::{flag_val, load_registry};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tomoyo_protocol::{PluginRegistryIndex, RegistryPluginEntry};

/// Computes LF-normalized SHA-256 of a JSON shard so Windows CRLF checkouts match Linux LF checkouts.
pub(crate) fn compute_canonical_json_sha256(raw: &[u8]) -> String {
    let mut hasher = Sha256::new();
    for chunk in raw.split(|&b| b == b'\r') {
        hasher.update(chunk);
    }
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn resolve_shard_path(index_source: &str, shard_source: &str) -> String {
    if shard_source.starts_with("https://") || shard_source.starts_with("http://") {
        return shard_source.to_string();
    }
    if (index_source.starts_with("https://") || index_source.starts_with("http://"))
        && let Some((prefix, _)) = index_source.rsplit_once('/')
    {
        return format!("{prefix}/{}", shard_source.trim_start_matches('/'));
    }
    if Path::new(shard_source).is_absolute() || Path::new(shard_source).exists() {
        return shard_source.to_string();
    }
    let idx_path = Path::new(index_source);
    if let Some(parent) = idx_path.parent() {
        let p1 = parent.join(shard_source);
        if p1.is_file() {
            return p1.to_string_lossy().into_owned();
        }
        if let Some(root) = parent.parent() {
            let p2 = root.join(shard_source);
            if p2.is_file() {
                return p2.to_string_lossy().into_owned();
            }
        }
    }
    shard_source.to_string()
}

/// Loads root registry index and flattens both inline `plugins` and every shard in `shards`.
pub(crate) fn load_registry_all_entries(
    index_source: &str,
) -> Result<(PluginRegistryIndex, Vec<RegistryPluginEntry>), String> {
    let root = load_registry(index_source)?;
    let mut seen = HashSet::new();
    let mut all = Vec::new();
    for p in &root.plugins {
        if seen.insert(p.id.to_ascii_lowercase()) {
            all.push(p.clone());
        }
    }
    for shard in &root.shards {
        let resolved = resolve_shard_path(index_source, &shard.source);
        let sub = load_registry(&resolved)
            .map_err(|e| format!("failed to load shard `{}` ({resolved}): {e}", shard.id))?;
        for mut p in sub.plugins {
            if p.category.is_empty() && !shard.category.is_empty() {
                p.category = shard.category.clone();
            }
            if seen.insert(p.id.to_ascii_lowercase()) {
                all.push(p);
            }
        }
    }
    Ok((root, all))
}

/// `tomoyo-cli registry check [--index registry/index.json] [--manifest-only]`
/// Recursively validates root index, all shards (count + SHA-256), plugin manifests, and plugin SHA-256s.
pub(crate) fn cmd_registry_check(args: &[String]) -> i32 {
    let index_source = flag_val(args, "--index").unwrap_or("registry/index.json");
    let manifest_only = args.iter().any(|a| a == "--manifest-only");
    let index_path = PathBuf::from(index_source);
    let root = match load_registry(index_source) {
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
    let mut entries: Vec<RegistryPluginEntry> = root.plugins.clone();

    if !root.shards.is_empty() {
        println!("=== Checking {} Registry Shard(s) ===", root.shards.len());
        let mut seen_shards = HashSet::new();
        for shard in &root.shards {
            if shard.id.trim().is_empty() || !seen_shards.insert(shard.id.to_ascii_lowercase()) {
                eprintln!("  [FAIL] Empty or duplicate shard id: `{}`", shard.id);
                errors += 1;
                continue;
            }
            let resolved = resolve_shard_path(index_source, &shard.source);
            if !resolved.starts_with("http://") && !resolved.starts_with("https://") {
                match std::fs::read(&resolved) {
                    Ok(bytes) => {
                        if !shard.sha256.is_empty() {
                            let actual = compute_canonical_json_sha256(&bytes);
                            if !actual.eq_ignore_ascii_case(&shard.sha256) {
                                eprintln!(
                                    "  [FAIL] shard `{}` SHA-256 drift!\n         index:  {}\n         actual: {}",
                                    shard.id, shard.sha256, actual
                                );
                                errors += 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  [FAIL] shard `{}` unreadable ({resolved}): {e}", shard.id);
                        errors += 1;
                        continue;
                    }
                }
            }
            match load_registry(&resolved) {
                Ok(sub) => {
                    if shard.count > 0 && sub.plugins.len() != shard.count {
                        eprintln!(
                            "  [FAIL] shard `{}` count mismatch: index says {}, file has {}",
                            shard.id,
                            shard.count,
                            sub.plugins.len()
                        );
                        errors += 1;
                    } else {
                        println!("  [OK]   shard {:<18} ({} plugin(s)) {}", shard.id, sub.plugins.len(), shard.source);
                    }
                    for mut p in sub.plugins {
                        if p.category.is_empty() && !shard.category.is_empty() {
                            p.category = shard.category.clone();
                        }
                        entries.push(p);
                    }
                }
                Err(e) => {
                    eprintln!("  [FAIL] shard `{}` parse error ({resolved}): {e}", shard.id);
                    errors += 1;
                }
            }
        }
    }

    if let Some(expected_total) = root.total_count
        && expected_total != entries.len()
    {
        eprintln!(
            "  [FAIL] total_count mismatch: index says {}, flattened entries = {}",
            expected_total,
            entries.len()
        );
        errors += 1;
    }

    let mut seen_ids = HashSet::new();
    println!("=== Checking Tomoyo Plugin Registry ({} entries) ===", entries.len());

    for entry in &entries {
        if entry.id.trim().is_empty() || !seen_ids.insert(entry.id.to_ascii_lowercase()) {
            eprintln!("  [FAIL] Empty or duplicate plugin id: `{}`", entry.id);
            errors += 1;
            continue;
        }
        if !entry.platforms.is_empty()
            && entry.platforms.values().any(|p| {
                p.source.trim().is_empty()
                    || p.sha256.len() != 64
                    || !p.sha256.chars().all(|c| c.is_ascii_hexdigit())
            })
        {
            eprintln!("  [FAIL] `{}` has invalid source or SHA-256 in `platforms`", entry.id);
            errors += 1;
            continue;
        }
        let Some((src_str, expected_sha)) =
            entry.resolve_for_platform(tomoyo_protocol::current_platform_key())
        else {
            println!("  [OK]   {:<28} v{} (targets={:?}, skipped local verify)", entry.id, entry.version, entry.supported_platforms());
            continue;
        };
        if expected_sha.len() != 64 || !expected_sha.chars().all(|c| c.is_ascii_hexdigit()) {
            eprintln!("  [FAIL] `{}` has invalid SHA-256 format: `{}`", entry.id, expected_sha);
            errors += 1;
            continue;
        }
        if src_str.starts_with("https://") || src_str.starts_with("http://") {
            println!("  [OK]   {:<28} v{} (remote URL, SHA-256 format valid)", entry.id, entry.version);
            continue;
        }

        let src_dir = if Path::new(src_str).is_absolute() || Path::new(src_str).exists() {
            PathBuf::from(src_str)
        } else {
            base_dir.join(src_str)
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
            Ok(actual) if actual.eq_ignore_ascii_case(expected_sha) => {
                println!("  [OK]   {:<28} v{} [{}] sha256={}…", entry.id, entry.version, manifest.runtime.kind_badge(), &actual[..16]);
            }
            Ok(actual) => {
                eprintln!("  [FAIL] `{}` SHA-256 drift!\n         registry: {}\n         actual:   {}", entry.id, expected_sha, actual);
                errors += 1;
            }
            Err(e) => {
                eprintln!("  [FAIL] `{}` hash error: {e}", entry.id);
                errors += 1;
            }
        }
    }

    if errors == 0 {
        println!("[PASS] All {} registry plugin(s) across {} shard(s) passed verification!", entries.len(), root.shards.len());
        0
    } else {
        eprintln!("[FAILED] Registry check found {errors} error(s).");
        1
    }
}

/// `tomoyo-cli registry build-shards --dir plugins --out registry`
/// Scans plugin subdirectories, verifies manifests, hashes bundles, and emits sharded v2 JSONs.
pub(crate) fn cmd_registry_build_shards(args: &[String]) -> i32 {
    let src_dir = PathBuf::from(flag_val(args, "--dir").unwrap_or("plugins"));
    let out_dir = PathBuf::from(flag_val(args, "--out").unwrap_or("registry"));
    match build_registry_shards(&src_dir, &out_dir) {
        Ok((shard_count, plugin_count)) => {
            println!(
                "[OK] Built {shard_count} registry shard(s) covering {plugin_count} plugin(s) -> {}",
                out_dir.display()
            );
            0
        }
        Err(e) => {
            eprintln!("[FAIL] build-shards failed: {e}");
            1
        }
    }
}
