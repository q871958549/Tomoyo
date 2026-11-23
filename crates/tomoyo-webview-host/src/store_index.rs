//! Scalable Store Catalog v2 index supporting sharded 10,000+ plugin registries,
//! category/runtime filtering, multi-token search, and bounded per-page serialization.

use crate::store::candidate_local_paths;
use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, OnceLock};
use tomoyo_core::{read_icon_data_url, resolve_bundled_module_icon};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};
use tomoyo_protocol::{PluginRegistryIndex, RegistryPluginEntry};

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

fn load_single_index(source: &str) -> Result<(PluginRegistryIndex, String), String> {
    if source.starts_with("https://") || source.starts_with("http://") {
        let resp = tomoyo_core::http::http_get(source, 8000, 8 * 1024 * 1024)?;
        if resp.status_code != 200 {
            return Err(format!("HTTP {}", resp.status_code));
        }
        let idx: PluginRegistryIndex =
            serde_json::from_slice(&resp.body).map_err(|e| e.to_string())?;
        return Ok((idx, source.to_string()));
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
    shard_src.to_string()
}

fn load_or_fetch_catalog(sources: &[String], force_refresh: bool) -> CachedCatalog {
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
        match load_single_index(src) {
            Ok((idx, origin)) => {
                loaded_from.push(origin.clone());
                for p in idx.plugins {
                    if seen_ids.insert(p.id.clone()) {
                        entries.push(p);
                    }
                }
                for shard in idx.shards {
                    let s_url = resolve_shard_source(src, &shard.source);
                    match load_single_index(&s_url) {
                        Ok((sub, _)) => {
                            for mut p in sub.plugins {
                                if p.category.is_empty() && !shard.category.is_empty() {
                                    p.category = shard.category.clone();
                                }
                                if seen_ids.insert(p.id.clone()) {
                                    entries.push(p);
                                }
                            }
                        }
                        Err(e) => errors.push(format!("shard `{}`: {e}", shard.id)),
                    }
                }
            }
            Err(e) => errors.push(format!("{src}: {e}")),
        }
    }

    let cached = CachedCatalog {
        sources_key: key,
        entries,
        loaded_from,
        errors,
    };
    if let Ok(mut g) = lock.lock() {
        *g = Some(cached.clone());
    }
    cached
}

pub fn normalize_category(raw: &str, keywords: &[String], runtime: &str) -> String {
    let t = raw.trim().to_ascii_lowercase();
    if !t.is_empty() {
        return t;
    }
    if runtime.eq_ignore_ascii_case("webview") || keywords.iter().any(|k| matches!(k.as_str(), "json" | "wasm" | "jwt" | "py" | "hash" | "hi" | "case")) {
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

fn resolve_store_item_icon_url(p: &RegistryPluginEntry) -> Option<String> {
    if let Some(local) = resolve_bundled_module_icon(&p.id)
        && let Some(url) = read_icon_data_url(&local)
    {
        return Some(url);
    }
    let rel_icon = p.icon.as_deref().unwrap_or("icon.png");
    if !p.source.starts_with("http") && !rel_icon.contains("..") {
        for cand in candidate_local_paths(&format!("{}/{rel_icon}", p.source.trim_end_matches('/'))) {
            if cand.is_file()
                && let Some(url) = read_icon_data_url(&cand)
            {
                return Some(url);
            }
        }
    }
    None
}

/// Queries indexed catalog items (O(N) scan with zero DOM bloat) and returns strictly 1 page.
pub fn query_store_page_json(req: &StoreQuery) -> String {
    let catalog = load_or_fetch_catalog(&req.sources, req.force_refresh);
    let host = PluginHost::new(default_plugin_dirs());
    let installed_ids: HashSet<String> = host.list_plugins().into_iter().map(|p| p.id).collect();
    host.shutdown_all();
    let user_plugins_dir = tomoyo_core::data_dir().join("plugins");

    let tokens: Vec<String> = req
        .query
        .split_whitespace()
        .map(|s| s.to_ascii_lowercase())
        .collect();
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

    let items: Vec<serde_json::Value> = matched
        .into_iter()
        .skip(skip)
        .take(page_size)
        .map(|(p, cat)| {
            let slug = p.id.replace('.', "-");
            let in_user_dir = user_plugins_dir.join(&slug).is_dir();
            let is_installed = installed_ids.contains(&p.id) || in_user_dir;
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "version": p.version,
                "description": p.description,
                "author": p.author,
                "license": p.license,
                "runtime_kind": p.runtime_kind,
                "category": cat,
                "display_mode": p.display_mode.as_str(),
                "icon_url": resolve_store_item_icon_url(p),
                "tags": p.tags,
                "keywords": p.keywords,
                "homepage": p.homepage,
                "downloads": p.downloads,
                "updated_at": p.updated_at,
                "source": p.source,
                "sha256": p.sha256,
                "installed": is_installed,
                "removable": in_user_dir,
            })
        })
        .collect();

    let quiet_md_ready = tomoyo_core::detect_quiet_md_binary(None)
        .map(|p| p.to_string_lossy().into_owned());

    serde_json::json!({
        "event": "store_loaded",
        "items": items,
        "page": page,
        "page_size": page_size,
        "total": total,
        "catalog_total": catalog.entries.len(),
        "total_pages": total_pages,
        "category_counts": cat_counts,
        "sources": catalog.loaded_from,
        "errors": catalog.errors,
        "quiet_md_path": quiet_md_ready,
    })
    .to_string()
}

pub fn make_install_quiet_md_event() -> String {
    match tomoyo_core::install_quiet_md_verified() {
        Ok(msg) => {
            let path = tomoyo_core::detect_quiet_md_binary(None)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            serde_json::json!({
                "event": "quiet_md_status",
                "ok": true,
                "path": path,
                "message": msg,
            })
            .to_string()
        }
        Err(e) => serde_json::json!({
            "event": "quiet_md_status",
            "ok": false,
            "path": "",
            "message": e,
        })
        .to_string(),
    }
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
                    "author": "Community",
                    "license": "MIT",
                    "runtime_kind": if i % 3 == 0 { "wasm" } else { "stdio_jsonrpc" },
                    "category": if i % 200 == 0 { "docs" } else { "productivity" },
                    "display_mode": if i % 2 == 0 { "inline" } else { "window" },
                    "keywords": ["tool", format!("k{i}")],
                    "source": format!("plugins/community-{i}"),
                    "sha256": "0".repeat(64)
                })
            })
            .collect();
        let shard_path = dir.join("shard_docs.json");
        std::fs::write(&shard_path, serde_json::json!({ "schema_version": 2, "name": "shard", "plugins": plugins }).to_string()).unwrap();

        let index_path = dir.join("index.json");
        std::fs::write(&index_path, serde_json::json!({
            "schema_version": 2,
            "name": "10K Benchmark Index",
            "total_count": 10000,
            "shards": [{ "id": "all", "category": "productivity", "source": shard_path.to_string_lossy() }],
            "plugins": []
        }).to_string()).unwrap();

        invalidate_catalog_cache();
        let started = std::time::Instant::now();
        let out_raw = query_store_page_json(&StoreQuery {
            sources: vec![index_path.to_string_lossy().into_owned()],
            query: "quiet".into(),
            category: "docs".into(),
            page: 2,
            page_size: 24,
            force_refresh: true,
            ..Default::default()
        });
        assert!(started.elapsed().as_millis() < 2_500);
        let v: serde_json::Value = serde_json::from_str(&out_raw).unwrap();
        assert_eq!(v["catalog_total"].as_u64(), Some(10_000));
        assert_eq!(v["total"].as_u64(), Some(50));
        assert_eq!(v["total_pages"].as_u64(), Some(3));
        assert_eq!(v["page"].as_u64(), Some(2));
        assert_eq!(v["items"].as_array().unwrap().len(), 24);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
