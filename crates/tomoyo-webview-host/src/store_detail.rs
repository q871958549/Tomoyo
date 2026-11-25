//! Lazy single-plugin detail lookup for the Store Detail Drawer.

use crate::store_cache::is_newer_semver;
use crate::store_index::{load_or_fetch_catalog, normalize_category, resolve_store_item_icon_url};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};
use tomoyo_protocol::current_platform_key;

/// Resolves full store metadata + live installation/update state for a single `plugin_id`.
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
