//! One-click Plugin & Registry Mirror Health Check (`CheckPluginHealth` -> `plugin_health_report`).

use crate::store::candidate_local_paths;
use crate::store_cache::is_newer_semver;
use crate::store_index::local_catalog_index_map;
use std::path::Path;
use std::time::Instant;
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};

pub fn run_plugin_health_check_json(sources: &[String]) -> String {
    let eff: Vec<String> = if sources.is_empty() {
        vec!["registry/index.json".into()]
    } else {
        sources.to_vec()
    };

    let catalog = local_catalog_index_map(&eff);
    let host = PluginHost::new(default_plugin_dirs());
    let installed = host.list_plugins();
    host.shutdown_all();

    let mut healthy = 0usize;
    let mut warnings = 0usize;
    let mut updates = 0usize;
    let mut plugin_items = Vec::with_capacity(installed.len());

    for p in installed {
        let has_icon = p
            .icon_path
            .as_deref()
            .is_some_and(|ip| Path::new(ip).is_file())
            || tomoyo_core::resolve_bundled_module_icon(&p.id).is_some();
        let cat = catalog.get(&p.id.to_ascii_lowercase());
        let upd_avail = cat.is_some_and(|e| is_newer_semver(&e.version, &p.version));
        if upd_avail {
            updates += 1;
        }
        let mut issues = Vec::new();
        if !has_icon {
            issues.push("缺少独立图标 icon.png");
        }
        let status = if issues.is_empty() {
            healthy += 1;
            "ok"
        } else {
            warnings += 1;
            "warn"
        };
        plugin_items.push(serde_json::json!({
            "id": p.id,
            "name": p.name,
            "version": p.version,
            "latest_version": cat.map(|e| e.version.as_str()),
            "update_available": upd_avail,
            "runtime": p.runtime_badge,
            "status": status,
            "issues": issues,
        }));
    }

    let mut mirrors = Vec::with_capacity(eff.len());
    let mut mirrors_ok = 0usize;
    for src in &eff {
        let started = Instant::now();
        if src.starts_with("http://") || src.starts_with("https://") {
            let (ok, detail) = match tomoyo_core::http::http_head_redirect(src, 4000) {
                Ok(r) if r.status_code < 400 => (true, format!("HTTP {}", r.status_code)),
                Ok(r) => (false, format!("HTTP {}", r.status_code)),
                Err(e) => (false, e),
            };
            if ok {
                mirrors_ok += 1;
            }
            mirrors.push(serde_json::json!({
                "source": src,
                "ok": ok,
                "latency_ms": started.elapsed().as_millis() as u64,
                "detail": detail,
            }));
        } else {
            let found = candidate_local_paths(src).into_iter().any(|p| p.is_file());
            if found {
                mirrors_ok += 1;
            }
            mirrors.push(serde_json::json!({
                "source": src,
                "ok": found,
                "latency_ms": started.elapsed().as_millis() as u64,
                "detail": if found { "本地分片就绪" } else { "本地索引未找到" },
            }));
        }
    }

    serde_json::json!({
        "event": "plugin_health_report",
        "summary": {
            "total": plugin_items.len(),
            "healthy": healthy,
            "warnings": warnings,
            "updates": updates,
            "mirrors_ok": mirrors_ok,
            "mirrors_total": mirrors.len(),
        },
        "plugins": plugin_items,
        "mirrors": mirrors,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::run_plugin_health_check_json;

    #[test]
    fn emits_structured_health_report_for_local_mirror() {
        let raw = run_plugin_health_check_json(&["registry/index.json".into()]);
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["event"], "plugin_health_report");
        assert!(v["summary"]["mirrors_total"].as_u64() == Some(1));
        assert!(v["plugins"].is_array());
    }
}
