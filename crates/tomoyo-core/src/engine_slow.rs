use crate::engine::{FastQueryBatch, Provider, QueryRouter};
use crate::engine_rank::{build_zero_result_fallbacks, rank_and_dedup};
use tomoyo_protocol::{QueryContext, ResultItem, SearchFilter};

impl QueryRouter {
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

        if ctx.filter == SearchFilter::All
            && !ctx.trimmed.is_empty()
            && let Some(bridge) = &self.plugin_bridge
            && !self.is_stale(generation)
        {
            combined.extend(bridge.query_global(ctx));
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
        clipboard: Option<tomoyo_protocol::ClipboardSnapshot>,
        attached: Option<tomoyo_protocol::AttachedContext>,
    ) -> Vec<ResultItem> {
        self.search_with_filter(raw_input, generation, limit, clipboard, attached, SearchFilter::All)
    }

    pub fn search_with_filter(
        &self,
        raw_input: &str,
        generation: u64,
        limit: usize,
        clipboard: Option<tomoyo_protocol::ClipboardSnapshot>,
        attached: Option<tomoyo_protocol::AttachedContext>,
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
        let mut ranked = rank_and_dedup(&self.frecency, raw_query, items, limit);
        crate::engine_rank::attach_plugin_file_actions(self.plugin_bridge.as_deref(), &mut ranked);
        ranked
    }
}

pub(crate) fn empty_fast_batch(ctx: QueryContext) -> FastQueryBatch {
    FastQueryBatch {
        items: Vec::new(),
        effective_ctx: ctx,
        is_dedicated_route: true,
        run_slow_ipc: false,
    }
}

pub(crate) fn provider_allowed_for_filter(provider: &dyn Provider, filter: SearchFilter) -> bool {
    match filter {
        SearchFilter::All => true,
        SearchFilter::App => matches!(
            provider.id(),
            "builtin.apps" | "builtin.system" | "builtin.windows"
        ),
        SearchFilter::Folder => provider.id() == "builtin.apps",
        _ => provider.supports_file_filters(),
    }
}

pub(crate) fn split_first_token(s: &str) -> (&str, &str) {
    if let Some(idx) = s.find(char::is_whitespace) {
        (&s[..idx], s[idx..].trim_start())
    } else {
        (s, "")
    }
}
