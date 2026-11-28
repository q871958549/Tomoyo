use crate::hash::compute_plugin_sha256;
use crate::registry::compute_canonical_json_sha256;
use crate::verify::validate_plugin_dir;
use std::path::{Path, PathBuf};
use tomoyo_protocol::{PluginRegistryIndex, RegistryPluginEntry};

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
            platforms: std::collections::BTreeMap::new(),
            min_tomoyo_version: None,
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
        assert_eq!(crate::registry::cmd_registry_check(&["--index".into(), idx]), 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
