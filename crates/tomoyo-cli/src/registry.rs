//! Decentralized sharded registry loader & recursive integrity verifier (`tomoyo-cli registry check`).

use crate::hash::compute_plugin_sha256;
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

pub(crate) fn build_registry_shards(src_root: &Path, out_root: &Path) -> Result<(usize, usize), String> {
    if !src_root.is_dir() {
        return Err(format!("Plugins directory not found: {}", src_root.display()));
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(src_root)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("tomoyo-plugin.toml").is_file())
        .collect();
    dirs.sort();

    let mut buckets: std::collections::BTreeMap<String, Vec<RegistryPluginEntry>> =
        std::collections::BTreeMap::new();
    let src_prefix = src_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("plugins");

    for dir in dirs {
        let (_, m, _) = validate_plugin_dir(&dir)?;
        let sha = compute_plugin_sha256(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let folder = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let shard_id = if folder.starts_with("official-") || folder.starts_with("builtin-") {
            "official".to_string()
        } else if folder.starts_with("example-") {
            "examples".to_string()
        } else {
            m.category.clone().unwrap_or_else(|| "community".into()).to_ascii_lowercase()
        };
        let perms: Vec<String> = m
            .permissions
            .iter()
            .filter_map(|p| serde_json::to_value(p).ok()?.as_str().map(ToString::to_string))
            .collect();
        let entry = RegistryPluginEntry {
            id: m.id,
            name: m.name,
            version: m.version,
            description: m.description,
            author: if m.author.is_empty() { "Tomoyo 贡献者".into() } else { m.author },
            license: if m.license.is_empty() { "MIT".into() } else { m.license },
            runtime_kind: match m.runtime {
                tomoyo_protocol::RuntimeConfig::StdioJsonrpc { .. } => "stdio_jsonrpc".into(),
                tomoyo_protocol::RuntimeConfig::Wasm { .. } => "wasm".into(),
                tomoyo_protocol::RuntimeConfig::Webview { .. } => "webview".into(),
            },
            category: m.category.unwrap_or_else(|| if shard_id == "examples" { "dev".into() } else { "productivity".into() }),
            icon: m.icon.or_else(|| dir.join("icon.png").is_file().then(|| "icon.png".into())),
            display_mode: m.display_mode,
            tags: m.tags,
            keywords: m.triggers.keywords,
            permissions: perms,
            source: format!("{src_prefix}/{folder}"),
            sha256: sha,
            homepage: m.homepage,
            updated_at: String::new(),
            downloads: 0,
        };
        buckets.entry(shard_id).or_default().push(entry);
    }

    let shards_dir = out_root.join("shards");
    std::fs::create_dir_all(&shards_dir).map_err(|e| e.to_string())?;
    let out_prefix = out_root.file_name().and_then(|s| s.to_str()).unwrap_or("registry");
    let mut shard_ptrs = Vec::new();
    let mut total_plugins = 0usize;

    for (shard_id, plugins) in buckets {
        total_plugins += plugins.len();
        let shard_cat = plugins.first().map(|p| p.category.clone()).unwrap_or_default();
        let today = tomoyo_core::format_beijing_now().chars().take(10).collect::<String>();
        let shard_doc = PluginRegistryIndex {
            schema_version: 2,
            name: format!("Tomoyo Registry Shard ({shard_id})"),
            updated_at: today,
            total_count: None,
            shards: Vec::new(),
            plugins,
        };
        let mut json = serde_json::to_string_pretty(&shard_doc).map_err(|e| e.to_string())?;
        json.push('\n');
        let shard_file = shards_dir.join(format!("{shard_id}.json"));
        std::fs::write(&shard_file, json.as_bytes()).map_err(|e| e.to_string())?;
        let shard_sha = compute_canonical_json_sha256(json.as_bytes());
        shard_ptrs.push(tomoyo_protocol::RegistryShardEntry {
            id: shard_id.clone(),
            category: shard_cat,
            source: format!("{out_prefix}/shards/{shard_id}.json"),
            count: shard_doc.plugins.len(),
            sha256: shard_sha,
        });
    }

    let root_doc = PluginRegistryIndex {
        schema_version: 2,
        name: "Tomoyo 开源插件清单 v2（免登录直装 · 支持万级分片索引）".into(),
        updated_at: tomoyo_core::format_beijing_now().chars().take(10).collect(),
        total_count: Some(total_plugins),
        shards: shard_ptrs,
        plugins: Vec::new(),
    };
    let mut root_json = serde_json::to_string_pretty(&root_doc).map_err(|e| e.to_string())?;
    root_json.push('\n');
    std::fs::write(out_root.join("index.json"), root_json.as_bytes()).map_err(|e| e.to_string())?;
    Ok((root_doc.shards.len(), total_plugins))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_hash_is_crlf_independent() {
        let lf = b"{\n  \"a\": 1\n}\n";
        let crlf = b"{\r\n  \"a\": 1\r\n}\r\n";
        assert_eq!(compute_canonical_json_sha256(lf), compute_canonical_json_sha256(crlf));
    }

    #[test]
    fn builds_and_checks_sharded_registry_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_build_shards_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let pdir = tmp.join("plugins").join("example-mini");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("tomoyo-plugin.toml"),
            "id=\"dev.test.mini\"\nname=\"Mini\"\nversion=\"0.1.0\"\ncategory=\"dev\"\n[runtime]\nkind=\"webview\"\nentry=\"index.html\"\n[triggers]\nkeywords=[\"mini\"]\n",
        ).unwrap();
        std::fs::write(pdir.join("index.html"), "<html>ok</html>").unwrap();

        let out_reg = tmp.join("registry");
        let (sc, pc) = build_registry_shards(&tmp.join("plugins"), &out_reg).unwrap();
        assert_eq!((sc, pc), (1, 1));
        let idx = out_reg.join("index.json").to_string_lossy().into_owned();
        assert_eq!(cmd_registry_check(&["--index".into(), idx]), 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
