//! Unified Multi-Runtime Plugin Host lifecycle orchestrator (lazy spawn, query, execute, idle reap).

use crate::discovery::{build_webview_trigger_item, scan_plugin_dirs};
use crate::process::{LivePluginProcess, ProcessError};
use crate::wasm::LiveWasmPlugin;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tomoyo_core::{inspect_pid_working_set, ExternalPluginBridge, PluginSummary};
use tomoyo_protocol::{
    ExecuteRequest, ExecuteResponse, PluginManifest, QueryContext, QueryResponse, ResultItem,
    RuntimeConfig, METHOD_EXECUTE, METHOD_QUERY,
};

#[derive(Debug, Clone)]
pub struct PluginRuntimeInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub runtime_badge: String,
    pub keywords: Vec<String>,
    pub running: bool,
    pub pid: Option<u32>,
    pub working_set_bytes: Option<u64>,
    pub max_memory_mb: u32,
    pub idle_timeout_secs: u64,
}

enum LiveRuntime {
    Stdio(LivePluginProcess),
    Wasm(LiveWasmPlugin),
}

impl LiveRuntime {
    fn last_used(&self) -> Instant {
        match self {
            Self::Stdio(p) => p.last_used(),
            Self::Wasm(w) => w.last_used(),
        }
    }
    fn pid(&self) -> Option<u32> {
        match self {
            Self::Stdio(p) => Some(p.pid()),
            Self::Wasm(_) => None,
        }
    }
    fn memory_bytes(&self) -> Option<u64> {
        match self {
            Self::Stdio(p) => inspect_pid_working_set(p.pid()),
            Self::Wasm(w) => Some(w.linear_memory_bytes()),
        }
    }
    fn shutdown(&mut self) {
        if let Self::Stdio(p) = self {
            p.shutdown_gracefully();
        }
    }
}

struct RegisteredPlugin {
    manifest: PluginManifest,
    dir: PathBuf,
    live: Option<LiveRuntime>,
}

struct HostState {
    search_dirs: Vec<PathBuf>,
    plugins: HashMap<String, RegisteredPlugin>,
    keyword_index: HashMap<String, Vec<String>>,
    shutting_down: bool,
}

/// Unified Plugin Host managing STDIO JSON-RPC, WASM Sandbox, and On-Demand WebView plugins.
pub struct PluginHost {
    state: Arc<(Mutex<HostState>, Condvar)>,
}

impl PluginHost {
    pub fn new(search_dirs: Vec<PathBuf>) -> Arc<Self> {
        let state = Arc::new((
            Mutex::new(HostState {
                search_dirs,
                plugins: HashMap::new(),
                keyword_index: HashMap::new(),
                shutting_down: false,
            }),
            Condvar::new(),
        ));
        let host = Arc::new(Self {
            state: Arc::clone(&state),
        });
        host.discover_plugins();
        host.spawn_idle_reaper();
        host
    }

    pub fn discover_plugins(&self) {
        let (lock, _) = &*self.state;
        let mut st = lock.lock().unwrap();
        for plug in st.plugins.values_mut() {
            if let Some(mut live) = plug.live.take() {
                live.shutdown();
            }
        }
        st.plugins.clear();
        st.keyword_index.clear();

        let discovered = scan_plugin_dirs(&st.search_dirs);
        for item in discovered {
            let id = item.manifest.id.clone();
            for kw in &item.manifest.triggers.keywords {
                let kw_lower = kw.trim().to_lowercase();
                if !kw_lower.is_empty() {
                    st.keyword_index.entry(kw_lower).or_default().push(id.clone());
                }
            }
            st.plugins.insert(
                id,
                RegisteredPlugin {
                    manifest: item.manifest,
                    dir: item.dir,
                    live: None,
                },
            );
        }
    }

    pub fn list_plugins(&self) -> Vec<PluginRuntimeInfo> {
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        let mut list: Vec<PluginRuntimeInfo> = st
            .plugins
            .values()
            .map(|p| PluginRuntimeInfo {
                id: p.manifest.id.clone(),
                name: p.manifest.name.clone(),
                version: p.manifest.version.clone(),
                description: p.manifest.description.clone(),
                runtime_badge: p.manifest.runtime.kind_badge().to_string(),
                keywords: p.manifest.triggers.keywords.clone(),
                running: p.live.is_some(),
                pid: p.live.as_ref().and_then(|l| l.pid()),
                working_set_bytes: p.live.as_ref().and_then(|l| l.memory_bytes()),
                max_memory_mb: p.manifest.resources.max_memory_mb,
                idle_timeout_secs: p.manifest.resources.idle_timeout_secs,
            })
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn execute_action(
        &self,
        plugin_id: &str,
        req: ExecuteRequest,
    ) -> Result<ExecuteResponse, ProcessError> {
        let (lock, cvar) = &*self.state;
        let mut st = lock.lock().unwrap();
        let Some(plug) = st.plugins.get_mut(plugin_id) else {
            return Err(ProcessError::Spawn(format!("plugin `{plugin_id}` not found")));
        };
        let timeout = Duration::from_millis(plug.manifest.resources.query_timeout_ms.max(2000));
        if ensure_live(plug)? {
            cvar.notify_one();
        }
        match plug.live.as_mut() {
            Some(LiveRuntime::Stdio(proc)) => {
                let params = serde_json::to_value(&req)?;
                match proc.call(METHOD_EXECUTE, params, timeout) {
                    Ok(val) => Ok(serde_json::from_value(val)?),
                    Err(err) => {
                        plug.live = None;
                        Err(err)
                    }
                }
            }
            Some(LiveRuntime::Wasm(wasm)) => wasm.execute(&req).map_err(|e| {
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
            if should_reap {
                if let Some(mut live) = plug.live.take() {
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

    fn query_plugin_ids(&self, ids: &[String], ctx: &QueryContext) -> Vec<ResultItem> {
        let (lock, cvar) = &*self.state;
        let mut st = lock.lock().unwrap();
        let mut items = Vec::new();
        let mut any_spawned = false;

        for id in ids {
            let Some(plug) = st.plugins.get_mut(id) else {
                continue;
            };
            if let RuntimeConfig::Webview {
                entry,
                utools_compat,
                width,
                height,
            } = &plug.manifest.runtime
            {
                items.push(build_webview_trigger_item(
                    &plug.manifest,
                    &plug.dir,
                    entry,
                    *utools_compat,
                    *width,
                    *height,
                    ctx,
                ));
                continue;
            }

            let timeout = Duration::from_millis(plug.manifest.resources.query_timeout_ms);
            match ensure_live(plug) {
                Ok(spawned) => any_spawned |= spawned,
                Err(err) => {
                    tracing::warn!("Failed to start plugin `{}`: {err}", plug.manifest.id);
                    continue;
                }
            }

            let query_res = match plug.live.as_mut() {
                Some(LiveRuntime::Stdio(proc)) => {
                    let Ok(params) = serde_json::to_value(ctx) else {
                        continue;
                    };
                    proc.call(METHOD_QUERY, params, timeout)
                        .and_then(|val| serde_json::from_value::<QueryResponse>(val).map_err(Into::into))
                }
                Some(LiveRuntime::Wasm(wasm)) => {
                    wasm.query(ctx).map_err(|e| ProcessError::Spawn(e.to_string()))
                }
                None => continue,
            };

            match query_res {
                Ok(resp) => {
                    for mut item in resp.items {
                        if item.provider_id.is_empty() {
                            item.provider_id = plug.manifest.id.clone();
                        }
                        items.push(item);
                    }
                }
                Err(err) => {
                    tracing::warn!("Plugin `{}` query error: {err}", plug.manifest.id);
                    plug.live = None;
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
                        if expired {
                            if let Some(mut live) = plug.live.take() {
                                tracing::info!(
                                    "Idle reaper stopping [{}] plugin `{}`",
                                    plug.manifest.runtime.kind_badge(),
                                    plug.manifest.id
                                );
                                live.shutdown();
                            }
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
            self.query_plugin_ids(&ids, ctx)
        }
    }

    fn query_global(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let ids: Vec<String> = {
            let (lock, _) = &*self.state;
            lock.lock()
                .unwrap()
                .plugins
                .values()
                .filter(|p| p.manifest.triggers.match_all)
                .map(|p| p.manifest.id.clone())
                .collect()
        };
        if ids.is_empty() {
            Vec::new()
        } else {
            self.query_plugin_ids(&ids, ctx)
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
}

fn ensure_live(plug: &mut RegisteredPlugin) -> Result<bool, ProcessError> {
    if plug.live.is_some() {
        return Ok(false);
    }
    match &plug.manifest.runtime {
        RuntimeConfig::StdioJsonrpc { command, args, env } => {
            let proc = LivePluginProcess::spawn(
                &plug.dir,
                command,
                args,
                env,
                plug.manifest.resources.max_memory_mb,
                plug.manifest.resources.query_timeout_ms,
            )?;
            plug.live = Some(LiveRuntime::Stdio(proc));
            Ok(true)
        }
        RuntimeConfig::Wasm { module, max_fuel } => {
            let mod_path = plug.dir.join(module);
            let wasm = LiveWasmPlugin::load(
                &plug.manifest.id,
                &mod_path,
                plug.manifest.permissions.clone(),
                plug.manifest.resources.max_memory_mb,
                *max_fuel,
            )
            .map_err(|e| ProcessError::Spawn(e.to_string()))?;
            plug.live = Some(LiveRuntime::Wasm(wasm));
            Ok(true)
        }
        RuntimeConfig::Webview { .. } => Ok(false),
    }
}

pub fn discover_from(path: &Path) -> Arc<PluginHost> {
    PluginHost::new(vec![path.to_path_buf()])
}
