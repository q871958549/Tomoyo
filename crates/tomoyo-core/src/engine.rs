use crate::engine_rank::{
    append_keyword_plugin_settings_entries, is_settings_intent_token,
};
use crate::engine_slow::{empty_fast_batch, provider_allowed_for_filter, split_first_token};
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
        self.prefixes().contains(&token_lower)
    }
    /// Whether this provider participates in unprefixed global searches.
    fn supports_global(&self) -> bool {
        true
    }
    /// Whether this provider should be queried after fast in-memory providers (e.g. IPC).
    fn is_slow_ipc(&self) -> bool {
        false
    }
    /// Whether this provider indexes files and responds to file-type filters (`@pdf`, `@word`, etc.).
    fn supports_file_filters(&self) -> bool {
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

/// Lightweight searchable metadata for an installed plugin (used by global search & quick settings).
#[derive(Debug, Clone)]
pub struct PluginSearchEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub runtime_badge: String,
    pub keywords: Vec<String>,
    pub enabled: bool,
    pub icon_path: Option<String>,
    pub ui_open_action: Option<tomoyo_protocol::ActionKind>,
}

/// Bridge trait allowing `tomoyo-plugin-host` to plug into `QueryRouter` without circular crates.
pub trait ExternalPluginBridge: Send + Sync {
    fn matches_keyword(&self, keyword: &str) -> bool;
    fn query_keyword(&self, keyword: &str, ctx: &QueryContext) -> Vec<ResultItem>;
    fn query_global(&self, ctx: &QueryContext) -> Vec<ResultItem>;
    fn reload(&self);
    fn reap_idle(&self);
    fn summary_lines(&self) -> Vec<PluginSummary>;
    /// Zero-I/O metadata list of all installed plugins for launcher search & quick settings jump.
    fn search_entries(&self) -> Vec<PluginSearchEntry> {
        Vec::new()
    }
    /// Protocol v3: zero-I/O extension-aware file actions contributed by enabled plugins.
    fn file_actions_for(&self, _full_path: &str, _ext_lower: &str) -> Vec<tomoyo_protocol::ActionSpec> {
        Vec::new()
    }
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
    pub(crate) providers: Vec<Arc<dyn Provider>>,
    pub(crate) plugin_bridge: Option<Arc<dyn ExternalPluginBridge>>,
    pub(crate) frecency: Arc<RwLock<FrecencyStore>>,
    pub(crate) config: Option<Arc<RwLock<TomoyoConfig>>>,
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

    pub fn is_first_run_frecency(&self) -> bool {
        self.frecency.read().is_ok_and(|f| f.is_empty())
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

        // 1. Dedicated `:` system commands & `?` / `/` command hub
        let is_cmd_hub = trimmed.starts_with('?')
            || trimmed.starts_with('？')
            || (trimmed.starts_with('/') && !trimmed[1..].contains('/'));
        if trimmed.starts_with(':') || is_cmd_hub {
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

        // 1.5. Super Match pasted capsule routing:
        // If user typed an explicit plugin keyword (e.g. `ocr`, `ocr en`), invoke that plugin
        // WITH `ctx.attached` intact instead of short-circuiting back to generic Super Match hints!
        if ctx.attached.is_some() {
            let (first_token, rest) = split_first_token(trimmed);
            let first_lower = first_token.to_lowercase();
            if !first_lower.is_empty()
                && let Some(bridge) = &self.plugin_bridge
                    && bridge.matches_keyword(&first_lower) {
                        let p_ctx = ctx.clone().with_keyword(&first_lower, rest);
                        let results = bridge.query_keyword(&first_lower, &p_ctx);
                        return FastQueryBatch {
                            items: self.rank_and_cap_for_query(trimmed, results, limit),
                            effective_ctx: p_ctx,
                            is_dedicated_route: true,
                            run_slow_ipc: false,
                        };
                    }
            let mut results = Vec::new();
            for p in &self.providers {
                if p.id() == "builtin.system" {
                    results.extend(p.query(&ctx));
                }
            }
            if let Some(bridge) = &self.plugin_bridge
                && !self.is_stale(generation) {
                    results.extend(bridge.query_global(&ctx));
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
            let inline_filter = self
                .config
                .as_ref()
                .and_then(|c| c.read().ok())
                .and_then(|c| SearchFilter::parse_inline_tag_with_custom(&first_lower, &c.custom_filters))
                .or_else(|| SearchFilter::parse_inline_tag(&first_lower));
            if let Some(inline_filter) = inline_filter {
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
                        || matches!(first_lower.as_str(), "cb" | "w" | "kill" | "ps" | "port"));
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

            if let Some(bridge) = &self.plugin_bridge
                && bridge.matches_keyword(&first_lower)
            {
                let settings_only = has_space_after && is_settings_intent_token(rest.trim());
                if !settings_only {
                    let p_ctx = ctx.clone().with_keyword(&first_lower, rest);
                    let mut results = bridge.query_keyword(&first_lower, &p_ctx);
                    if !has_space_after && rest.trim().is_empty() {
                        append_keyword_plugin_settings_entries(&mut results, bridge.as_ref(), &first_lower);
                    }
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
            if !provider_allowed_for_filter(p.as_ref(), ctx.filter) {
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
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
