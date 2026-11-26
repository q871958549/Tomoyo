//! Zero-I/O plugin search metadata collector for global search & quick Settings jump.

use crate::discovery::resolve_plugin_icon_path;
use crate::host::HostState;
use crate::v3_support::resolve_plugin_webview_open;
use tomoyo_core::PluginSearchEntry;
use tomoyo_protocol::OpenWebViewRequest;

pub(crate) fn collect_plugin_search_entries(st: &HostState) -> Vec<PluginSearchEntry> {
    let mut out: Vec<PluginSearchEntry> = st
        .plugins
        .values()
        .map(|p| {
            let enabled = !st.disabled_ids.contains(&p.manifest.id.to_ascii_lowercase());
            let ui_open_action = resolve_plugin_webview_open(p, &OpenWebViewRequest::default());
            let icon_path = resolve_plugin_icon_path(&p.manifest, &p.dir)
                .or_else(|| {
                    tomoyo_core::resolve_bundled_module_icon(&p.manifest.id)
                        .map(|ip| ip.to_string_lossy().into_owned())
                });
            let runtime_badge = match &p.manifest.runtime {
                tomoyo_protocol::RuntimeConfig::StdioJsonrpc { .. } => {
                    if ui_open_action.is_some() { "Hybrid" } else { "STDIO" }
                }
                tomoyo_protocol::RuntimeConfig::Wasm { .. } => "WASM",
                tomoyo_protocol::RuntimeConfig::Webview { .. } => "WebView",
            };
            PluginSearchEntry {
                id: p.manifest.id.clone(),
                name: p.manifest.name.clone(),
                description: p.manifest.description.clone(),
                runtime_badge: runtime_badge.into(),
                keywords: p.manifest.triggers.keywords.clone(),
                enabled,
                icon_path,
                ui_open_action,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
