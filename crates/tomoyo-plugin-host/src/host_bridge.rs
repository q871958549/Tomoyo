use crate::host::PluginHost;
use crate::v3_support::collect_file_actions_for;
use std::path::Path;
use std::sync::Arc;
use tomoyo_core::{ExternalPluginBridge, PluginSummary};
use tomoyo_protocol::{ActionSpec, QueryContext, ResultItem};

impl ExternalPluginBridge for PluginHost {
    fn matches_keyword(&self, keyword: &str) -> bool {
        let (lock, _) = &*self.state;
        lock.lock().unwrap().keyword_index.contains_key(&keyword.to_lowercase())
    }

    fn query_keyword(&self, keyword: &str, ctx: &QueryContext) -> Vec<ResultItem> {
        let ids = {
            let (lock, _) = &*self.state;
            lock.lock()
                .unwrap()
                .keyword_index
                .get(&keyword.to_lowercase())
                .cloned()
                .unwrap_or_default()
        };
        if ids.is_empty() {
            Vec::new()
        } else {
            self.query_plugin_ids(&ids, ctx, true)
        }
    }

    fn query_global(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let ids: Vec<String> = {
            let (lock, _) = &*self.state;
            let st = lock.lock().unwrap();
            st.plugins
                .values()
                .filter(|p| {
                    p.manifest.triggers.match_all
                        && !st.disabled_ids.contains(&p.manifest.id.to_ascii_lowercase())
                })
                .map(|p| p.manifest.id.clone())
                .collect()
        };
        if ids.is_empty() {
            Vec::new()
        } else {
            self.query_plugin_ids(&ids, ctx, false)
        }
    }

    fn reload(&self) {
        self.discover_plugins();
    }

    fn reap_idle(&self) {
        let _ = self.reap_plugins(false);
    }

    fn summary_lines(&self) -> Vec<PluginSummary> {
        self.list_plugins()
            .into_iter()
            .map(|p| PluginSummary {
                id: p.id,
                name: p.name,
                runtime_badge: p.runtime_badge,
                keywords: p.keywords.join(", "),
                running: p.running,
                working_set_bytes: p.working_set_bytes,
                max_memory_mb: p.max_memory_mb,
            })
            .collect()
    }

    fn file_actions_for(&self, full_path: &str, ext_lower: &str) -> Vec<ActionSpec> {
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        collect_file_actions_for(&st.plugins, &st.disabled_ids, full_path, ext_lower)
    }

    fn search_entries(&self) -> Vec<tomoyo_core::PluginSearchEntry> {
        let (lock, _) = &*self.state;
        crate::search_meta::collect_plugin_search_entries(&lock.lock().unwrap())
    }
}

pub fn discover_from(path: &Path) -> Arc<PluginHost> {
    PluginHost::new(vec![path.to_path_buf()])
}
