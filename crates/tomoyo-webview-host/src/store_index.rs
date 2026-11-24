//! Scalable Store Catalog v2 index supporting sharded 10,000+ plugin registries,
//! disk ETag/304 caching, parallel shard fetching, SemVer update detection, and lazy plugin details.

use crate::store::candidate_local_paths;
use crate::store_cache::{
    fetch_remote_index_cached, fetch_shards_parallel, is_newer_semver, read_cached_index_at,
    store_cache_dir,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use tomoyo_core::{read_icon_data_url, resolve_bundled_module_icon};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};
use tomoyo_protocol::{current_platform_key, PluginRegistryIndex, RegistryPluginEntry};

#[derive(Debug, Clone, Default)]
pub struct StoreQuery {
    pub sources: Vec<String>,
    pub query: String,
    pub category: String,
    pub runtime: String,
    pub sort: String,
    pub page: usize,
    pub page_size: usize,
    pub force_refresh: bool,
}

#[derive(Clone)]
struct CachedCatalog {
    sources_key: String,
    entries: Vec<RegistryPluginEntry>,
    loaded_from: Vec<String>,
    errors: Vec<String>,
}

static CATALOG_CACHE: OnceLock<Mutex<Option<CachedCatalog>>> = OnceLock::new();

pub fn invalidate_catalog_cache() {
    let lock = CATALOG_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut g) = lock.lock() {
        *g = None;
    }
}

fn load_single_index(
    source: &str,
    force_refresh: bool,
    cache_only: bool,
) -> Result<(PluginRegistryIndex, String), String> {
    if source.starts_with("https://") || source.starts_with("http://") {
        if cache_only {
            let (idx, _, _) = read_cached_index_at(&store_cache_dir(), source)
                .ok_or_else(|| "未命中本地磁盘缓存".to_string())?;
            return Ok((idx, format!("{source} (cached)")));
        }
        let (idx, from_cache) = fetch_remote_index_cached(source, force_refresh)?;
        let origin = if from_cache {
            format!("{source} (cached)")
        } else {
            source.to_string()
        };
        return Ok((idx, origin));
    }
    for cand in candidate_local_paths(source) {
        if cand.is_file() {
            let raw = std::fs::read_to_string(&cand).map_err(|e| e.to_string())?;
            let idx: PluginRegistryIndex = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            return Ok((idx, cand.to_string_lossy().into_owned()));
        }
    }
    Err("本地索引文件未找到".into())
}

fn resolve_shard_source(parent: &str, shard_src: &str) -> String {
    if shard_src.starts_with("https://") || shard_src.starts_with("http://") {
        return shard_src.to_string();
    }
    if let Some((prefix, _)) = parent.rsplit_once('/')
        && (parent.starts_with("https://") || parent.starts_with("http://"))
    {
        return format!("{prefix}/{}", shard_src.trim_start_matches('/'));
    }
    if std::path::Path::new(shard_src).is_absolute() || std::path::Path::new(shard_src).exists() {
        return shard_src.to_string();
    }
    if let Some(parent_dir) = std::path::Path::new(parent).parent() {
        let rel = parent_dir.join(shard_src);
        if rel.is_file() {
            return rel.to_string_lossy().into_owned();
        }
        if let Some(root) = parent_dir.parent() {
            let rel2 = root.join(shard_src);
            if rel2.is_file() {
                return rel2.to_string_lossy().into_owned();
            }
        }
    }
    shard_src.to_string()
}

fn load_or_fetch_catalog(
    sources: &[String],
    force_refresh: bool,
    cache_only: bool,
) -> CachedCatalog {
    let eff: Vec<String> = if sources.is_empty() {
        vec!["registry/index.json".into()]
    } else {
        sources.to_vec()
    };
    let key = eff.join("|");
    let lock = CATALOG_CACHE.get_or_init(|| Mutex::new(None));
    if !force_refresh
        && let Ok(g) = lock.lock()
        && let Some(c) = g.as_ref()
        && c.sources_key == key
    {
        return c.clone();
    }

    let mut seen_ids = HashSet::new();
    let mut entries = Vec::new();
    let mut loaded_from = Vec::new();
    let mut errors = Vec::new();

    for src in &eff {
        match load_single_index(src, force_refresh, cache_only) {
            Ok((idx, origin)) => {
                loaded_from.push(origin.clone());
                for p in idx.plugins {
                    if seen_ids.insert(p.id.clone()) {
                        entries.push(p);
                    }
                }
                let shard_jobs: Vec<(String, String, String)> = idx
                    .shards
                    .into_iter()
                    .map(|sh| {
                        let resolved = resolve_shard_source(src, &sh.source);
                        (sh.id, sh.category, resolved)
                    })
                    .collect();
                for (shard_id, shard_cat, res) in fetch_shards_parallel(&shard_jobs, |u| {
                    load_single_index(u, force_refresh, cache_only).map(|(i, _)| i)
                }) {
                    match res {
                        Ok(sub) => {
                            for mut p in sub.plugins {
                                if p.category.is_empty() && !shard_cat.is_empty() {
                                    p.category = shard_cat.clone();
                                }
                                if seen_ids.insert(p.id.clone()) {
                                    entries.push(p);
                                }
                            }
                        }
                        Err(e) => errors.push(format!("shard `{shard_id}`: {e}")),
                    }
                }
            }
            Err(e) => errors.push(format!("{src}: {e}")),
        }
    }

    let cached = CachedCatalog { sources_key: key, entries, loaded_from, errors };
    if !cache_only && let Ok(mut g) = lock.lock() {
        *g = Some(cached.clone());
    }
    cached
}

pub fn normalize_category(raw: &str, keywords: &[String], runtime: &str) -> String {
    let t = raw.trim().to_ascii_lowercase();
    if !t.is_empty() {
        return t;
    }
    if runtime.eq_ignore_ascii_case("webview")
        || keywords.iter().any(|k| matches!(k.as_str(), "json" | "wasm" | "jwt" | "py" | "hash" | "hi" | "case"))
    {
        return "dev".into();
    }
    if keywords.iter().any(|k| matches!(k.as_str(), "note" | "memo" | "md" | "markdown" | "edit" | "quiet")) {
        return "docs".into();
    }
    if keywords.iter().any(|k| matches!(k.as_str(), "ocr")) {
        return "ai".into();
    }
    "productivity".into()
}

fn resolve_store_item_icon_url(p: &RegistryPluginEntry, resolved_source: &str) -> Option<String> {
    let rel_icon = p.icon.as_deref().unwrap_or("icon.png");
    let src = if !resolved_source.is_empty() { resolved_source } else { p.source.as_str() };
    if !src.is_empty() && !src.starts_with("http") && !rel_icon.contains("..") {
        for cand in candidate_local_paths(&format!("{}/{rel_icon}", src.trim_end_matches('/'))) {
            if cand.is_file()
                && let Some(url) = read_icon_data_url(&cand)
            {
                return Some(url);
            }
        }
    }
    if let Some(local) = resolve_bundled_module_icon(&p.id)
        && let Some(url) = read_icon_data_url(&local)
    {
        return Some(url);
    }
    None
}

/// Fast zero-network lookup of catalog entries for Settings Init SemVer upgrade badges.
pub fn local_catalog_index_map(sources: &[String]) -> HashMap<String, RegistryPluginEntry> {
    let catalog = load_or_fetch_catalog(sources, false, true);
    catalog.entries.into_iter().map(|e| (e.id.to_ascii_lowercase(), e)).collect()
}

/// Queries indexed catalog items (O(N) scan with zero DOM bloat) and returns strictly 1 page.
pub fn query_store_page_json(req: &StoreQuery) -> String {
    let catalog = load_or_fetch_catalog(&req.sources, req.force_refresh, false);
    let host = PluginHost::new(default_plugin_dirs());
    let installed_versions: HashMap<String, String> = host
        .list_plugins()
        .into_iter()
        .map(|p| (p.id.to_ascii_lowercase(), p.version))
        .collect();
    host.shutdown_all();
    let user_plugins_dir = tomoyo_core::data_dir().join("plugins");

    let tokens: Vec<String> = req.query.split_whitespace().map(|s| s.to_ascii_lowercase()).collect();
    let want_rt = req.runtime.trim().to_ascii_lowercase();
    let want_cat = req.category.trim().to_ascii_lowercase();

    let mut cat_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut matched: Vec<(&RegistryPluginEntry, String)> = Vec::new();

    for p in &catalog.entries {
        if !want_rt.is_empty() && want_rt != "all" && !p.runtime_kind.eq_ignore_ascii_case(&want_rt) {
            continue;
        }
        if !tokens.is_empty() {
            let hay = format!(
                "{} {} {} {} {} {}",
                p.id.to_ascii_lowercase(),
                p.name.to_ascii_lowercase(),
                p.description.to_ascii_lowercase(),
                p.author.to_ascii_lowercase(),
                p.keywords.join(" ").to_ascii_lowercase(),
                p.tags.join(" ").to_ascii_lowercase()
            );
            if !tokens.iter().all(|t| hay.contains(t)) {
                continue;
            }
        }
        let cat = normalize_category(&p.category, &p.keywords, &p.runtime_kind);
        *cat_counts.entry(cat.clone()).or_insert(0) += 1;
        if want_cat.is_empty() || want_cat == "all" || cat == want_cat {
            matched.push((p, cat));
        }
    }

    if req.sort.eq_ignore_ascii_case("downloads") {
        matched.sort_by(|a, b| b.0.downloads.cmp(&a.0.downloads).then_with(|| a.0.name.cmp(&b.0.name)));
    } else if req.sort.eq_ignore_ascii_case("name") {
        matched.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    }

    let page_size = req.page_size.clamp(6, 48);
    let total = matched.len();
    let total_pages = total.div_ceil(page_size).max(1);
    let page = req.page.clamp(1, total_pages);
    let skip = (page - 1) * page_size;

    let plat = current_platform_key();
    let items: Vec<serde_json::Value> = matched
        .into_iter()
        .skip(skip)
        .take(page_size)
        .map(|(p, cat)| {
            let slug = p.id.replace('.', "-");
            let in_user_dir = user_plugins_dir.join(&slug).is_dir();
            let inst_ver = installed_versions.get(&p.id.to_ascii_lowercase()).cloned();
            let is_installed = inst_ver.is_some() || in_user_dir;
            let (eff_source, eff_sha, platform_ok) = match p.resolve_for_platform(plat) {
                Some((src, sha)) => (src, sha, true),
                None => ("", "", false),
            };
            let update_available = platform_ok
                && inst_ver.as_deref().is_some_and(|v| is_newer_semver(&p.version, v));
            serde_json::json!({
                "id": p.id, "name": p.name, "version": p.version,
                "description": p.description, "author": p.author, "license": p.license,
                "runtime_kind": p.runtime_kind, "category": cat,
                "display_mode": p.display_mode.as_str(),
                "icon_url": resolve_store_item_icon_url(p, eff_source),
                "tags": p.tags, "keywords": p.keywords, "permissions": p.permissions,
                "homepage": p.homepage, "downloads": p.downloads, "updated_at": p.updated_at,
                "source": eff_source, "sha256": eff_sha,
                "platform_compatible": platform_ok,
                "supported_platforms": p.supported_platforms(),
                "min_tomoyo_version": p.min_tomoyo_version,
                "installed": is_installed, "removable": in_user_dir,
                "installed_version": inst_ver, "update_available": update_available,
            })
        })
        .collect();

    serde_json::json!({
        "event": "store_loaded", "items": items, "page": page, "page_size": page_size,
        "total": total, "catalog_total": catalog.entries.len(), "total_pages": total_pages,
        "category_counts": cat_counts, "sources": catalog.loaded_from, "errors": catalog.errors,
    })
    .to_string()
}

/// Lazy single-plugin detail lookup for the Store Detail Drawer.
pub fn query_plugin_detail_json(plugin_id: &str, sources: &[String]) -> String {
    let catalog = load_or_fetch_catalog(sources, false, false);
    let Some(p) = catalog.entries.iter().find(|e| e.id.eq_ignore_ascii_case(plugin_id)) else {
        return serde_json::json!({
            "event": "plugin_detail", "ok": false, "plugin_id": plugin_id,
            "error": "未在插件市场索引中找到该插件详情"
        })
        .to_string();
    };
    let host = PluginHost::new(default_plugin_dirs());
    let installed_entry = host.list_plugins().into_iter().find(|i| i.id.eq_ignore_ascii_case(&p.id));
    host.shutdown_all();

    let slug = p.id.replace('.', "-");
    let in_user_dir = tomoyo_core::data_dir().join("plugins").join(&slug).is_dir();
    let inst_ver = installed_entry.as_ref().map(|i| i.version.clone());
    let (eff_source, eff_sha, platform_ok) = match p.resolve_for_platform(current_platform_key()) {
        Some((src, sha)) => (src, sha, true),
        None => ("", "", false),
    };
    let update_available =
        platform_ok && inst_ver.as_deref().is_some_and(|v| is_newer_semver(&p.version, v));
    let perms = if !p.permissions.is_empty() {
        p.permissions.clone()
    } else if let Some(inst) = &installed_entry {
        inst.declared_permissions.clone()
    } else {
        Vec::new()
    };
    let cat = normalize_category(&p.category, &p.keywords, &p.runtime_kind);
    serde_json::json!({
        "event": "plugin_detail", "ok": true,
        "detail": {
            "id": p.id, "name": p.name, "version": p.version,
            "description": p.description, "author": p.author, "license": p.license,
            "homepage": p.homepage, "runtime_kind": p.runtime_kind, "category": cat,
            "display_mode": p.display_mode.as_str(),
            "icon_url": resolve_store_item_icon_url(p, eff_source),
            "tags": p.tags, "keywords": p.keywords, "permissions": perms,
            "source": eff_source, "sha256": eff_sha,
            "platform_compatible": platform_ok,
            "supported_platforms": p.supported_platforms(),
            "min_tomoyo_version": p.min_tomoyo_version,
            "downloads": p.downloads, "updated_at": p.updated_at,
            "installed": inst_ver.is_some() || in_user_dir,
            "removable": in_user_dir,
            "installed_version": inst_ver,
            "update_available": update_available,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_10k_sharded_catalog_and_paginates_cleanly() {
        let dir = std::env::temp_dir().join(format!("tomoyo_10k_store_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let plugins: Vec<serde_json::Value> = (0..10_000)
            .map(|i| {
                serde_json::json!({
                    "id": format!("community.plugin.{i}"),
                    "name": format!("Community Plugin #{i}"),
                    "version": "1.0.0",
                    "description": if i % 200 == 0 { "Markdown quiet helper" } else { "General utility tool" },
                    "author": "Community", "license": "MIT",
                    "runtime_kind": if i % 3 == 0 { "wasm" } else { "stdio_jsonrpc" },
                    "category": if i % 200 == 0 { "docs" } else { "productivity" },
                    "display_mode": if i % 2 == 0 { "inline" } else { "window" },
                    "keywords": ["tool", format!("k{i}")],
                    "permissions": ["clipboard"],
                    "source": format!("plugins/community-{i}"),
                    "sha256": "0".repeat(64)
                })
            })
            .collect();
        let shard_path = dir.join("shard_docs.json");
        std::fs::write(&shard_path, serde_json::json!({ "schema_version": 2, "name": "shard", "plugins": plugins }).to_string()).unwrap();

        let index_path = dir.join("index.json");
        std::fs::write(&index_path, serde_json::json!({
            "schema_version": 2, "name": "10K Benchmark Index", "total_count": 10000,
            "shards": [{ "id": "all", "category": "productivity", "source": shard_path.to_string_lossy() }],
            "plugins": []
        }).to_string()).unwrap();

        invalidate_catalog_cache();
        let started = std::time::Instant::now();
        let out_raw = query_store_page_json(&StoreQuery {
            sources: vec![index_path.to_string_lossy().into_owned()],
            query: "quiet".into(), category: "docs".into(), page: 2, page_size: 24, force_refresh: true,
            ..Default::default()
        });
        assert!(started.elapsed().as_millis() < 2_500);
        let v: serde_json::Value = serde_json::from_str(&out_raw).unwrap();
        assert_eq!(v["catalog_total"].as_u64(), Some(10_000));
        assert_eq!(v["total"].as_u64(), Some(50));
        assert_eq!(v["total_pages"].as_u64(), Some(3));
        assert_eq!(v["page"].as_u64(), Some(2));
        assert_eq!(v["items"].as_array().unwrap().len(), 24);
        assert_eq!(v["items"][0]["platform_compatible"].as_bool(), Some(true));
        let detail_raw = query_plugin_detail_json("community.plugin.200", &[index_path.to_string_lossy().into_owned()]);
        let dv: serde_json::Value = serde_json::from_str(&detail_raw).unwrap();
        assert_eq!(dv["event"], "plugin_detail");
        assert_eq!(dv["ok"], true);
        assert_eq!(dv["detail"]["id"], "community.plugin.200");
        assert_eq!(dv["detail"]["permissions"][0], "clipboard");

        let plat_index = dir.join("plat.json");
        std::fs::write(&plat_index, serde_json::json!({
            "schema_version": 2, "name": "Platform Matrix",
            "plugins": [{
                "id": "dev.tomoyo.only-other", "name": "Other OS Tool", "version": "1.0.0",
                "description": "Not for current OS", "author": "Community", "license": "MIT",
                "runtime_kind": "stdio_jsonrpc",
                "platforms": { "plan9-riscv64": { "source": "https://example.com/p9.zip", "sha256": "a".repeat(64) } }
            }]
        }).to_string()).unwrap();
        let plat_raw = query_store_page_json(&StoreQuery {
            sources: vec![plat_index.to_string_lossy().into_owned()], page: 1, page_size: 12, force_refresh: true,
            ..Default::default()
        });
        let pv: serde_json::Value = serde_json::from_str(&plat_raw).unwrap();
        assert_eq!(pv["items"][0]["platform_compatible"].as_bool(), Some(false));
        assert_eq!(pv["items"][0]["source"].as_str(), Some(""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
