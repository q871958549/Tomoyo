use crate::engine_rank::{build_zero_result_fallbacks, rank_and_dedup};
use crate::{record_hibernation_cycle, trim_working_set, FrecencyStore, TomoyoConfig};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tomoyo_protocol::{AttachedContext, ClipboardSnapshot, QueryContext, ResultItem, SearchFilter};

/// Trait implemented by all built-in Tier-1 zero-overhead providers.
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// Dedicated static prefix keywords (e.g. `["f", "ev"]` for Everything).
    fn prefixes(&self) -> &'static [&'static str] {
        &[]
    }
    /// Dynamic prefix check (e.g. config-driven web search keywords).
    fn matches_prefix(&self, token_lower: &str) -> bool {
        self.prefixes().iter().any(|&p| p == token_lower)
    }
    /// Whether this provider participates in unprefixed global searches.
    fn supports_global(&self) -> bool {
        true
    }
    /// Whether this provider should be queried after fast in-memory providers (e.g. IPC).
    fn is_slow_ipc(&self) -> bool {
        false
    }
    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem>;
    fn reload(&self) {}
    fn trim_memory(&self) {}
}

/// Summary line for `:plugins` diagnostics.
#[derive(Debug, Clone)]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub runtime_badge: String,
    pub keywords: String,
    pub running: bool,
    pub working_set_bytes: Option<u64>,
    pub max_memory_mb: u32,
}

/// Bridge trait allowing `tomoyo-plugin-host` to plug into `QueryRouter` without circular crates.
pub trait ExternalPluginBridge: Send + Sync {
    fn matches_keyword(&self, keyword: &str) -> bool;
    fn query_keyword(&self, keyword: &str, ctx: &QueryContext) -> Vec<ResultItem>;
    fn query_global(&self, ctx: &QueryContext) -> Vec<ResultItem>;
    fn reload(&self);
    fn reap_idle(&self);
    fn summary_lines(&self) -> Vec<PluginSummary>;
}

/// Result of Phase-1 instant in-memory provider execution before slow IPC merge.
pub struct FastQueryBatch {
    pub items: Vec<ResultItem>,
    pub effective_ctx: QueryContext,
    pub is_dedicated_route: bool,
    pub run_slow_ipc: bool,
}

/// Central orchestrator routing queries across built-in providers and external plugins.
pub struct QueryRouter {
    providers: Vec<Arc<dyn Provider>>,
    plugin_bridge: Option<Arc<dyn ExternalPluginBridge>>,
    frecency: Arc<RwLock<FrecencyStore>>,
    config: Option<Arc<RwLock<TomoyoConfig>>>,
    generation: Arc<AtomicU64>,
}

impl QueryRouter {
    pub fn new(
        providers: Vec<Arc<dyn Provider>>,
        plugin_bridge: Option<Arc<dyn ExternalPluginBridge>>,
        frecency: Arc<RwLock<FrecencyStore>>,
    ) -> Self {
        Self {
            providers,
            plugin_bridge,
            frecency,
            config: None,
            generation: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn with_config(mut self, config: Arc<RwLock<TomoyoConfig>>) -> Self {
        self.config = Some(config);
        self
    }

    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn is_stale(&self, generation: u64) -> bool {
        self.current_generation() != generation
    }

    pub fn record_execution(&self, item_id: &str) {
        self.record_execution_for_query("", item_id);
    }

    pub fn record_execution_for_query(&self, raw_query: &str, item_id: &str) {
        if let Ok(mut store) = self.frecency.write() {
            store.record_use_for_query(raw_query, item_id);
        }
    }

    pub fn reload_all(&self) {
        for p in &self.providers {
            p.reload();
        }
        if let Some(bridge) = &self.plugin_bridge {
            bridge.reload();
        }
    }

    /// Invoked during hibernation (hidden > delay) or explicit `:gc` command.
    pub fn hibernate_trim(&self) -> u64 {
        for p in &self.providers {
            p.trim_memory();
        }
        if let Some(bridge) = &self.plugin_bridge {
            bridge.reap_idle();
        }
        if let Ok(mut store) = self.frecency.write() {
            store.shrink();
        }
        trim_working_set();
        record_hibernation_cycle()
    }

    /// Runs Phase 1 (instant in-memory providers) so UI renders immediately without waiting on IPC.
    pub fn search_fast(
        &self,
        raw_input: &str,
        generation: u64,
        limit: usize,
        clipboard: Option<ClipboardSnapshot>,
        attached: Option<AttachedContext>,
        filter: SearchFilter,
    ) -> FastQueryBatch {
        let trimmed = raw_input.trim();
        let mut ctx = QueryContext::new(raw_input, generation, limit).with_filter(filter);
        ctx.clipboard = clipboard;
        ctx.attached = attached;

        // 1. Dedicated `:` system commands
        if trimmed.starts_with(':') {
            let mut results = Vec::new();
            for p in &self.providers {
                if p.prefixes().contains(&":") {
                    results.extend(p.query(&ctx));
                }
            }
            return FastQueryBatch {
                items: self.rank_and_cap_for_query(trimmed, results, limit),
                effective_ctx: ctx,
                is_dedicated_route: true,
                run_slow_ipc: false,
            };
        }

        // 1.5. Super Match pasted capsule short-circuit
        if ctx.attached.is_some() {
            let mut results = Vec::new();
            for p in &self.providers {
                if p.id() == "builtin.system" {
                    results.extend(p.query(&ctx));
                }
            }
            if let Some(bridge) = &self.plugin_bridge {
                if !self.is_stale(generation) {
                    results.extend(bridge.query_global(&ctx));
                }
            }
            return FastQueryBatch {
                items: self.rank_and_cap_for_query(trimmed, results, limit),
                effective_ctx: ctx,
                is_dedicated_route: true,
                run_slow_ipc: false,
            };
        }

        // 2. Check `<keyword> <args>` prefix routing (also supports `@pdf`, `@excel`, etc. inline tags)
        let (first_token, rest) = split_first_token(trimmed);
        let first_lower = first_token.to_lowercase();
        let has_space_after = trimmed.len() > first_token.len();

        if filter == SearchFilter::All {
            if let Some(inline_filter) = SearchFilter::parse_inline_tag(&first_lower) {
                let mut f_ctx = QueryContext::new(rest, generation, limit).with_filter(inline_filter);
                f_ctx.clipboard = ctx.clipboard.clone();
                return self.run_filtered_fast(f_ctx, generation, limit);
            }
        }

        if !first_lower.is_empty() {
            let mut matched_prefix_providers = Vec::new();
            for p in &self.providers {
                let exact_match = p.matches_prefix(&first_lower);
                let trigger_now = exact_match
                    && (has_space_after
                        || matches!(first_lower.as_str(), "cb" | "w" | "kill" | "ps"));
                if trigger_now {
                    matched_prefix_providers.push(p);
                }
            }
            if !matched_prefix_providers.is_empty() {
                let p_ctx = ctx.clone().with_keyword(&first_lower, rest);
                let mut results = Vec::new();
                for p in matched_prefix_providers {
                    if self.is_stale(generation) {
                        return empty_fast_batch(p_ctx);
                    }
                    results.extend(p.query(&p_ctx));
                }
                return FastQueryBatch {
                    items: self.rank_and_cap_for_query(trimmed, results, limit),
                    effective_ctx: p_ctx,
                    is_dedicated_route: true,
                    run_slow_ipc: false,
                };
            }

            if let Some(bridge) = &self.plugin_bridge {
                if bridge.matches_keyword(&first_lower) {
                    let p_ctx = ctx.clone().with_keyword(&first_lower, rest);
                    let results = bridge.query_keyword(&first_lower, &p_ctx);
                    return FastQueryBatch {
                        items: self.rank_and_cap_for_query(trimmed, results, limit),
                        effective_ctx: p_ctx,
                        is_dedicated_route: true,
                        run_slow_ipc: false,
                    };
                }
            }
        }

        self.run_filtered_fast(ctx, generation, limit)
    }

    fn run_filtered_fast(
        &self,
        ctx: QueryContext,
        generation: u64,
        limit: usize,
    ) -> FastQueryBatch {
        let mut results = Vec::new();
        for p in &self.providers {
            if !p.supports_global() || p.is_slow_ipc() {
                continue;
            }
            if !provider_allowed_for_filter(p.id(), ctx.filter) {
                continue;
            }
            if self.is_stale(generation) {
                return empty_fast_batch(ctx);
            }
            results.extend(p.query(&ctx));
        }

        let run_slow_ipc = ctx.filter != SearchFilter::App
            && (!ctx.trimmed.is_empty() || ctx.filter.is_file_only());
        let ranked = self.rank_and_cap_for_query(&ctx.trimmed, results, limit);
        FastQueryBatch {
            items: ranked,
            effective_ctx: ctx,
            is_dedicated_route: false,
            run_slow_ipc,
        }
    }

    /// Phase 2: executes slow IPC providers (Everything) and external plugins, then merges & ranks.
    pub fn search_slow_and_merge(&self, batch: FastQueryBatch, limit: usize) -> Vec<ResultItem> {
        if batch.is_dedicated_route || !batch.run_slow_ipc {
            return batch.items;
        }
        let ctx = &batch.effective_ctx;
        let generation = ctx.generation;
        let mut combined = batch.items;

        for p in &self.providers {
            if !p.supports_global() || !p.is_slow_ipc() {
                continue;
            }
            if self.is_stale(generation) {
                return Vec::new();
            }
            let mut sub_ctx = ctx.clone();
            sub_ctx.limit = if ctx.filter.is_file_only() {
                limit
            } else {
                limit.min(10)
            };
            combined.extend(p.query(&sub_ctx));
        }

        if ctx.filter == SearchFilter::All && !ctx.trimmed.is_empty() {
            if let Some(bridge) = &self.plugin_bridge {
                if !self.is_stale(generation) {
                    combined.extend(bridge.query_global(ctx));
                }
            }
        }

        let mut ranked = self.rank_and_cap_for_query(&ctx.trimmed, combined, limit);
        if ranked.is_empty() && ctx.filter == SearchFilter::All && !ctx.trimmed.is_empty() {
            ranked = build_zero_result_fallbacks(self.config.as_ref(), &ctx.trimmed);
        }
        ranked
    }

    /// Synchronous convenience search combining Phase 1 and Phase 2 (used by tests & self-test).
    pub fn search(
        &self,
        raw_input: &str,
        generation: u64,
        limit: usize,
        clipboard: Option<ClipboardSnapshot>,
        attached: Option<AttachedContext>,
    ) -> Vec<ResultItem> {
        self.search_with_filter(raw_input, generation, limit, clipboard, attached, SearchFilter::All)
    }

    pub fn search_with_filter(
        &self,
        raw_input: &str,
        generation: u64,
        limit: usize,
        clipboard: Option<ClipboardSnapshot>,
        attached: Option<AttachedContext>,
        filter: SearchFilter,
    ) -> Vec<ResultItem> {
        let fast = self.search_fast(raw_input, generation, limit, clipboard, attached, filter);
        self.search_slow_and_merge(fast, limit)
    }

    pub fn rank_and_cap(&self, items: Vec<ResultItem>, limit: usize) -> Vec<ResultItem> {
        self.rank_and_cap_for_query("", items, limit)
    }

    pub fn rank_and_cap_for_query(
        &self,
        raw_query: &str,
        items: Vec<ResultItem>,
        limit: usize,
    ) -> Vec<ResultItem> {
        rank_and_dedup(&self.frecency, raw_query, items, limit)
    }
}

fn empty_fast_batch(ctx: QueryContext) -> FastQueryBatch {
    FastQueryBatch {
        items: Vec::new(),
        effective_ctx: ctx,
        is_dedicated_route: true,
        run_slow_ipc: false,
    }
}

fn provider_allowed_for_filter(provider_id: &str, filter: SearchFilter) -> bool {
    match filter {
        SearchFilter::All => true,
        SearchFilter::App => matches!(
            provider_id,
            "builtin.apps" | "builtin.system" | "builtin.windows"
        ),
        _ => false,
    }
}

fn split_first_token(s: &str) -> (&str, &str) {
    if let Some(idx) = s.find(char::is_whitespace) {
        (&s[..idx], s[idx..].trim_start())
    } else {
        (s, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomoyo_protocol::ResultCategory;

    struct StubProvider {
        id: &'static str,
        slow: bool,
    }

    impl Provider for StubProvider {
        fn id(&self) -> &'static str {
            self.id
        }
        fn name(&self) -> &'static str {
            self.id
        }
        fn is_slow_ipc(&self) -> bool {
            self.slow
        }
        fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
            vec![ResultItem {
                id: format!("{}:{:?}:{}", self.id, ctx.filter, ctx.trimmed),
                provider_id: self.id.into(),
                title: format!("{}:{}", self.id, ctx.trimmed),
                subtitle: String::new(),
                category: if self.slow { ResultCategory::File } else { ResultCategory::App },
                score: 5000,
                icon: Default::default(),
                badge: None,
                copy_text: None,
                actions: vec![],
                preview: None,
            }]
        }
    }

    #[test]
    fn routes_inline_tags_and_group_filters_correctly() {
        let dir = std::env::temp_dir().join("tomoyo_router_test_frecency.json");
        let frecency = Arc::new(RwLock::new(FrecencyStore::load_or_default(dir)));
        let providers: Vec<Arc<dyn Provider>> = vec![
            Arc::new(StubProvider { id: "builtin.apps", slow: false }),
            Arc::new(StubProvider { id: "builtin.everything", slow: true }),
        ];
        let router = QueryRouter::new(providers, None, frecency);

        let g1 = router.next_generation();
        let pdf_hits = router.search("@pdf 报价", g1, 20, None, None);
        assert_eq!(pdf_hits.len(), 1);
        assert_eq!(pdf_hits[0].id, "builtin.everything:Pdf:报价");

        let g2 = router.next_generation();
        let app_hits = router.search_with_filter("oc", g2, 20, None, None, SearchFilter::App);
        assert_eq!(app_hits.len(), 1);
        assert_eq!(app_hits[0].id, "builtin.apps:App:oc");
    }

    #[test]
    fn empty_global_search_falls_back_to_web_and_everything() {
        let frecency = Arc::new(RwLock::new(FrecencyStore::default()));
        let router = QueryRouter::new(vec![], None, frecency)
            .with_config(Arc::new(RwLock::new(TomoyoConfig::default())));
        let g = router.next_generation();
        let hits = router.search("zzzz_no_such_local_item", g, 20, None, None);
        assert_eq!(hits.len(), 2);
        assert!(hits[0].id.starts_with("fallback:web:"));
        assert!(hits[1].id.starts_with("fallback:ev:"));
    }
}
