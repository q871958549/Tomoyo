use crate::{FrecencyStore, record_hibernation_cycle, trim_working_set};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tomoyo_protocol::{AttachedContext, ClipboardSnapshot, QueryContext, ResultItem};

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

/// Central orchestrator routing queries across built-in providers and external plugins.
pub struct QueryRouter {
    providers: Vec<Arc<dyn Provider>>,
    plugin_bridge: Option<Arc<dyn ExternalPluginBridge>>,
    frecency: Arc<RwLock<FrecencyStore>>,
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
            generation: Arc::new(AtomicU64::new(1)),
        }
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
        if let Ok(mut store) = self.frecency.write() {
            store.record_use(item_id);
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

    /// Invoked during hibernation (hidden > 15s) or explicit `:gc` command.
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

    /// Executes a synchronous routed search with generation-based cooperative cancellation.
    pub fn search(
        &self,
        raw_input: &str,
        generation: u64,
        limit: usize,
        clipboard: Option<ClipboardSnapshot>,
        attached: Option<AttachedContext>,
    ) -> Vec<ResultItem> {
        let trimmed = raw_input.trim();
        let mut ctx = QueryContext::new(raw_input, generation, limit);
        ctx.clipboard = clipboard;
        ctx.attached = attached;

        // 1. Check for dedicated command prefix (`:` commands)
        if trimmed.starts_with(':') {
            let mut results = Vec::new();
            for p in &self.providers {
                if p.prefixes().contains(&":") {
                    results.extend(p.query(&ctx));
                }
            }
            return self.rank_and_cap(results, limit);
        }

        // 1.5. Super Match short-circuit: when an image/file capsule is attached,
        // focus exclusively on Super Match system actions + matching plugins (zero app icon churn!).
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
            return self.rank_and_cap(results, limit);
        }

        // 2. Check for `<keyword> <args>` or exact `<keyword>` routing
        let (first_token, rest) = split_first_token(trimmed);
        let first_lower = first_token.to_lowercase();
        let has_space_after = trimmed.len() > first_token.len();

        // Check built-in prefix providers (`f `, `ev `, `cb`, `w`, `kill`, `ps`, `=`, or config web keywords)
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
                        return Vec::new();
                    }
                    results.extend(p.query(&p_ctx));
                }
                return self.rank_and_cap(results, limit);
            }

            // Check external plugin keyword triggers
            if let Some(bridge) = &self.plugin_bridge {
                if bridge.matches_keyword(&first_lower) {
                    let p_ctx = ctx.clone().with_keyword(&first_lower, rest);
                    let results = bridge.query_keyword(&first_lower, &p_ctx);
                    return self.rank_and_cap(results, limit);
                }
            }
        }

        // 3. Global smart hybrid query (fast in-memory providers first, then slow IPC like Everything)
        let mut results = Vec::new();
        for p in &self.providers {
            if !p.supports_global() || p.is_slow_ipc() {
                continue;
            }
            if self.is_stale(generation) {
                return Vec::new();
            }
            results.extend(p.query(&ctx));
        }

        if !trimmed.is_empty() {
            for p in &self.providers {
                if !p.supports_global() || !p.is_slow_ipc() {
                    continue;
                }
                if self.is_stale(generation) {
                    return Vec::new();
                }
                let mut sub_ctx = ctx.clone();
                sub_ctx.limit = sub_ctx.limit.min(8);
                results.extend(p.query(&sub_ctx));
            }

            if let Some(bridge) = &self.plugin_bridge {
                if !self.is_stale(generation) {
                    results.extend(bridge.query_global(&ctx));
                }
            }
        }

        self.rank_and_cap(results, limit)
    }

    fn rank_and_cap(&self, mut items: Vec<ResultItem>, limit: usize) -> Vec<ResultItem> {
        let frecency = self.frecency.read().ok();
        for item in &mut items {
            if let Some(store) = frecency.as_ref() {
                item.score = item.score.saturating_add(store.bonus(&item.id));
            }
        }
        items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));

        let mut seen = HashSet::with_capacity(items.len().min(limit));
        let mut deduped = Vec::with_capacity(limit.min(items.len()));
        for item in items {
            if seen.insert(item.id.clone()) {
                deduped.push(item);
                if deduped.len() >= limit {
                    break;
                }
            }
        }
        deduped
    }
}

fn split_first_token(s: &str) -> (&str, &str) {
    if let Some(idx) = s.find(char::is_whitespace) {
        (&s[..idx], s[idx..].trim_start())
    } else {
        (s, "")
    }
}
