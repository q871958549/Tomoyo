//! Unified Multi-Runtime Plugin Host lifecycle orchestrator (lazy spawn, query, execute, idle reap).

use crate::discovery::{build_webview_trigger_item, scan_plugin_dirs};
use crate::process::ProcessError;
use crate::runtime::{ensure_live, plugin_error_result_item, LiveRuntime, RegisteredPlugin};
use crate::v2_support::{
    build_runtime_info, collect_active_plugin_hotkeys, decorate_plugin_items,
    dispatch_plugin_event, effective_plugin_settings, ActivePluginHotkey,
};
use crate::v3_support::{collect_file_actions_for, load_declared_plugin_secrets};
pub use crate::v2_support::PluginRuntimeInfo;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tomoyo_core::{
    next_trace_id, ExternalPluginBridge, PermissionStore, PluginSummary, TomoyoConfig,
};
use tomoyo_protocol::{
    ActionSpec, ExecuteRequest, ExecuteResponse, PluginEventKind, PluginEventPayload, QueryContext,
    QueryResponse, ResultItem, RuntimeConfig, METHOD_EXECUTE, METHOD_QUERY,
};

pub(crate) struct HostState {
    pub(crate) search_dirs: Vec<PathBuf>,
    pub(crate) plugins: HashMap<String, RegisteredPlugin>,
    pub(crate) keyword_index: HashMap<String, Vec<String>>,
    pub(crate) disabled_ids: HashSet<String>,
    pub(crate) plugin_settings: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    pub(crate) perms: PermissionStore,
    pub(crate) shutting_down: bool,
}

/// Unified Plugin Host managing STDIO JSON-RPC, WASM Sandbox, and On-Demand WebView plugins.
pub struct PluginHost {
    pub(crate) state: Arc<(Mutex<HostState>, Condvar)>,
    pub(crate) interactive_prompts: AtomicBool,
}

impl PluginHost {
    pub fn new(search_dirs: Vec<PathBuf>) -> Arc<Self> {
        let state = Arc::new((
            Mutex::new(HostState {
                search_dirs,
                plugins: HashMap::new(),
                keyword_index: HashMap::new(),
                disabled_ids: HashSet::new(),
                plugin_settings: BTreeMap::new(),
                perms: PermissionStore::load_default(),
                shutting_down: false,
            }),
            Condvar::new(),
        ));
        let host = Arc::new(Self { state: Arc::clone(&state), interactive_prompts: AtomicBool::new(true) });
        host.discover_plugins();
        host.spawn_idle_reaper();
        host
    }

    pub fn set_interactive_prompts(&self, enabled: bool) {
        self.interactive_prompts.store(enabled, Ordering::Release);
    }

    pub fn discover_plugins(&self) {
        let (lock, _) = &*self.state;
        let mut st = lock.lock().unwrap();
        st.perms = PermissionStore::load_default();
        for plug in st.plugins.values_mut() {
            if let Some(mut live) = plug.live.take() { live.shutdown(); }
        }
        st.plugins.clear();
        st.keyword_index.clear();

        let cfg = tomoyo_core::TomoyoConfig::load_or_create();
        st.disabled_ids = cfg.disabled_plugins.iter().map(|s| s.to_ascii_lowercase()).collect();
        st.plugin_settings = cfg.plugin_settings.clone();
        for item in scan_plugin_dirs(&st.search_dirs) {
            let id = item.manifest.id.clone();
            if !st.disabled_ids.contains(&id.to_ascii_lowercase()) {
                for kw in &item.manifest.triggers.keywords {
                    let kw_lower = kw.trim().to_lowercase();
                    if !kw_lower.is_empty() { st.keyword_index.entry(kw_lower).or_default().push(id.clone()); }
                }
            }
            st.plugins.insert(id, RegisteredPlugin { manifest: item.manifest, dir: item.dir, live: None });
        }
    }

    pub fn list_plugins(&self) -> Vec<PluginRuntimeInfo> {
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        let mut list: Vec<PluginRuntimeInfo> = st
            .plugins
            .values()
            .map(|p| build_runtime_info(p, &st.perms))
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn active_plugin_hotkeys(&self, cfg: &TomoyoConfig) -> Vec<ActivePluginHotkey> {
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        collect_active_plugin_hotkeys(&st.plugins, &st.disabled_ids, cfg)
    }

    pub fn has_event_subscriber(&self, event: PluginEventKind) -> bool {
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        st.plugins.iter().any(|(id, p)| {
            !st.disabled_ids.contains(&id.to_ascii_lowercase()) && p.manifest.events.contains(&event)
        })
    }

    pub fn dispatch_event(&self, target_plugin: Option<&str>, payload: &PluginEventPayload) -> Vec<ExecuteResponse> {
        let interactive = self.interactive_prompts.load(Ordering::Acquire);
        let (lock, cvar) = &*self.state;
        let mut st = lock.lock().unwrap();
        let has_sub = st.plugins.iter().any(|(id, p)| {
            target_plugin.is_none_or(|t| id.eq_ignore_ascii_case(t))
                && !st.disabled_ids.contains(&id.to_ascii_lowercase())
                && p.manifest.events.contains(&payload.event)
        });
        if !has_sub { return Vec::new(); }
        let HostState { plugins, disabled_ids, perms, plugin_settings, .. } = &mut *st;
        let (res, spawned) = dispatch_plugin_event(
            plugins,
            disabled_ids,
            perms,
            plugin_settings,
            target_plugin,
            payload,
            interactive,
        );
        if spawned { cvar.notify_one(); }
        res
    }

    pub fn execute_action(
        &self,
        plugin_id: &str,
        mut req: ExecuteRequest,
    ) -> Result<ExecuteResponse, ProcessError> {
        let interactive = self.interactive_prompts.load(Ordering::Acquire);
        let (lock, cvar) = &*self.state;
        let mut st = lock.lock().unwrap();
        let HostState { plugins, perms, plugin_settings, .. } = &mut *st;
        let Some(plug) = plugins.get_mut(plugin_id) else {
            return Err(ProcessError::Spawn(format!("plugin `{plugin_id}` not found")));
        };
        if req.plugin_settings.is_empty() {
            req.plugin_settings = effective_plugin_settings(&plug.manifest, plugin_settings);
        }
        if req.plugin_secrets.is_empty() {
            req.plugin_secrets = load_declared_plugin_secrets(&plug.manifest);
        }
        let trace_id = next_trace_id("EXE");
        let started = Instant::now();
        let timeout = Duration::from_millis(plug.manifest.resources.query_timeout_ms.max(2000));
        if ensure_live(plug, perms, interactive)? {
            cvar.notify_one();
        }
        
        match plug.live.as_mut() {
            Some(LiveRuntime::Stdio(proc)) => {
                let params = serde_json::to_value(&req)?;
                match proc.call(METHOD_EXECUTE, params, timeout) {
                    Ok(val) => Ok(serde_json::from_value(val)?),
                    Err(err) => {
                        tracing::error!(trace_id = %trace_id, plugin_id = %plugin_id, ms = started.elapsed().as_millis(), "Plugin execute failed: {err}");
                        plug.live = None;
                        Err(err)
                    }
                }
            }
            Some(LiveRuntime::Wasm(wasm)) => wasm.execute(&req).map_err(|e| {
                tracing::error!(trace_id = %trace_id, plugin_id = %plugin_id, ms = started.elapsed().as_millis(), "WASM execute failed: {e}");
                plug.live = None;
                ProcessError::Spawn(e.to_string())
            }),
            None => Ok(ExecuteResponse::default()),
        }
    }

    pub fn reap_plugins(&self, force_all: bool) -> usize {
        let (lock, _) = &*self.state;
        let mut st = lock.lock().unwrap();
        let now = Instant::now();
        let mut reaped = 0;
        for plug in st.plugins.values_mut() {
            let idle_ttl = Duration::from_secs(plug.manifest.resources.idle_timeout_secs);
            let should_reap = plug
                .live
                .as_ref()
                .map(|l| force_all || now.duration_since(l.last_used()) >= idle_ttl)
                .unwrap_or(false);
            if should_reap
                && let Some(mut live) = plug.live.take() {
                    tracing::info!(
                        "Reaping idle [{}] plugin `{}` (pid={:?})",
                        plug.manifest.runtime.kind_badge(),
                        plug.manifest.id,
                        live.pid()
                    );
                    live.shutdown();
                    reaped += 1;
                }
        }
        reaped
    }

    pub fn shutdown_all(&self) {
        let (lock, cvar) = &*self.state;
        let mut st = lock.lock().unwrap();
        st.shutting_down = true;
        for plug in st.plugins.values_mut() {
            if let Some(mut live) = plug.live.take() {
                live.shutdown();
            }
        }
        cvar.notify_all();
    }

    fn query_plugin_ids(&self, ids: &[String], ctx: &QueryContext, surface_errors: bool) -> Vec<ResultItem> {
        let interactive = self.interactive_prompts.load(Ordering::Acquire);
        let (lock, cvar) = &*self.state;
        let mut st = lock.lock().unwrap();
        let HostState { plugins, perms, plugin_settings, .. } = &mut *st;
        let mut items = Vec::new();
        let mut any_spawned = false;

        for id in ids {
            let Some(plug) = plugins.get_mut(id) else { continue };
            let mut plug_ctx = ctx.clone();
            plug_ctx.plugin_settings = effective_plugin_settings(&plug.manifest, plugin_settings);
            plug_ctx.plugin_secrets = load_declared_plugin_secrets(&plug.manifest);
            if let RuntimeConfig::Webview { entry, utools_compat, width, height } = &plug.manifest.runtime {
                items.push(build_webview_trigger_item(&plug.manifest, &plug.dir, entry, *utools_compat, *width, *height, &plug_ctx));
                continue;
            }

            let trace_id = next_trace_id("PLG");
            let started = Instant::now();
            let timeout = Duration::from_millis(plug.manifest.resources.query_timeout_ms);
            match ensure_live(plug, perms, interactive) {
                Ok(spawned) => {
                    if spawned {
                        any_spawned = true;
                        tracing::info!(
                            trace_id = %trace_id,
                            plugin_id = %plug.manifest.id,
                            pid = ?plug.live.as_ref().and_then(|l| l.pid()),
                            ms = started.elapsed().as_millis(),
                            "Spawned plugin runtime"
                        );
                    }
                }
                Err(err) => {
                    tracing::error!(trace_id = %trace_id, plugin_id = %plug.manifest.id, "Failed to start plugin: {err}");
                    if surface_errors {
                        items.push(plugin_error_result_item(&plug.manifest.id, &plug.manifest.name, &trace_id, &err.to_string(), &[]));
                    }
                    continue;
                }
            }

            let query_res = match plug.live.as_mut() {
                Some(LiveRuntime::Stdio(proc)) => {
                    let Ok(params) = serde_json::to_value(&plug_ctx) else { continue };
                    proc.call(METHOD_QUERY, params, timeout)
                        .and_then(|val| serde_json::from_value::<QueryResponse>(val).map_err(Into::into))
                }
                Some(LiveRuntime::Wasm(wasm)) => {
                    wasm.query(&plug_ctx).map_err(|e| ProcessError::Spawn(e.to_string()))
                }
                None => continue,
            };
            let elapsed_ms = started.elapsed().as_millis();

            match query_res {
                Ok(mut resp) => {
                    if elapsed_ms >= 800 {
                        tracing::warn!(trace_id = %trace_id, plugin_id = %plug.manifest.id, ms = elapsed_ms, hits = resp.items.len(), "Slow plugin query");
                    }
                    decorate_plugin_items(plug, &mut resp.items);
                    items.extend(resp.items);
                }
                Err(err) => {
                    let stderr_tail = match plug.live.as_ref() {
                        Some(LiveRuntime::Stdio(proc)) => proc.recent_stderr(),
                        _ => Vec::new(),
                    };
                    tracing::error!(trace_id = %trace_id, plugin_id = %plug.manifest.id, ms = elapsed_ms, "Plugin query error: {err}");
                    plug.live = None;
                    if surface_errors {
                        items.push(plugin_error_result_item(&plug.manifest.id, &plug.manifest.name, &trace_id, &err.to_string(), &stderr_tail));
                    }
                }
            }
        }

        if any_spawned {
            cvar.notify_one();
        }
        items
    }

    fn spawn_idle_reaper(&self) {
        let state = Arc::clone(&self.state);
        let _ = std::thread::Builder::new()
            .name("tomoyo-plugin-reaper".to_string())
            .spawn(move || {
                let (lock, cvar) = &*state;
                let mut st = lock.lock().unwrap();
                loop {
                    if st.shutting_down {
                        break;
                    }
                    if !st.plugins.values().any(|p| p.live.is_some()) {
                        st = cvar.wait(st).unwrap();
                        continue;
                    }
                    let (guard, _) = cvar.wait_timeout(st, Duration::from_secs(5)).unwrap();
                    st = guard;
                    if st.shutting_down {
                        break;
                    }
                    let now = Instant::now();
                    for plug in st.plugins.values_mut() {
                        let ttl = Duration::from_secs(plug.manifest.resources.idle_timeout_secs);
                        let expired = plug
                            .live
                            .as_ref()
                            .map(|l| now.duration_since(l.last_used()) >= ttl)
                            .unwrap_or(false);
                        if expired
                            && let Some(mut live) = plug.live.take() {
                                tracing::info!(
                                    "Idle reaper stopping [{}] plugin `{}`",
                                    plug.manifest.runtime.kind_badge(),
                                    plug.manifest.id
                                );
                                live.shutdown();
                            }
                    }
                }
            });
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

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
