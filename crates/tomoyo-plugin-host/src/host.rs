use crate::process::{LivePluginProcess, ProcessError};
use crate::wasm::LiveWasmPlugin;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tomoyo_core::{inspect_pid_working_set, ExternalPluginBridge, PluginSummary};
use tomoyo_protocol::{
    ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse, IconSpec, PluginManifest,
    PreviewSpec, QueryContext, QueryResponse, ResultCategory, ResultItem, RuntimeConfig,
    METHOD_EXECUTE, METHOD_QUERY,
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

    /// Scans all configured plugin directories for `tomoyo-plugin.toml`.
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

        let dirs = st.search_dirs.clone();
        for root in dirs {
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let manifest_path = path.join("tomoyo-plugin.toml");
                if !manifest_path.is_file() {
                    continue;
                }
                match std::fs::read_to_string(&manifest_path)
                    .map_err(|e| e.to_string())
                    .and_then(|raw| PluginManifest::from_toml_str(&raw).map_err(|e| e.to_string()))
                {
                    Ok(manifest) => {
                        let id = manifest.id.clone();
                        for kw in &manifest.triggers.keywords {
                            let kw_lower = kw.trim().to_lowercase();
                            if !kw_lower.is_empty() {
                                st.keyword_index
                                    .entry(kw_lower)
                                    .or_default()
                                    .push(id.clone());
                            }
                        }
                        tracing::info!(
                            "Discovered [{}] plugin `{}` ({}) from {}",
                            manifest.runtime.kind_badge(),
                            manifest.name,
                            manifest.id,
                            path.display()
                        );
                        st.plugins.insert(
                            id,
                            RegisteredPlugin {
                                manifest,
                                dir: path,
                                live: None,
                            },
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            "Skipping invalid plugin manifest at {}: {err}",
                            manifest_path.display()
                        );
                    }
                }
            }
        }
    }

    pub fn list_plugins(&self) -> Vec<PluginRuntimeInfo> {
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        let mut list: Vec<PluginRuntimeInfo> = st
            .plugins
            .values()
            .map(|p| {
                let pid = p.live.as_ref().and_then(|l| l.pid());
                let ws = p.live.as_ref().and_then(|l| l.memory_bytes());
                PluginRuntimeInfo {
                    id: p.manifest.id.clone(),
                    name: p.manifest.name.clone(),
                    version: p.manifest.version.clone(),
                    description: p.manifest.description.clone(),
                    runtime_badge: p.manifest.runtime.kind_badge().to_string(),
                    keywords: p.manifest.triggers.keywords.clone(),
                    running: p.live.is_some(),
                    pid,
                    working_set_bytes: ws,
                    max_memory_mb: p.manifest.resources.max_memory_mb,
                    idle_timeout_secs: p.manifest.resources.idle_timeout_secs,
                }
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
            return Err(ProcessError::Spawn(format!(
                "plugin `{plugin_id}` not found"
            )));
        };

        let timeout = Duration::from_millis(plug.manifest.resources.query_timeout_ms.max(2000));
        let spawned = ensure_live(plug)?;
        if spawned {
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

    /// Reaps any plugin (STDIO process or WASM sandbox) idle beyond TTL (or all if `force_all`).
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

            // WebView plugins consume 0 bytes until explicitly launched on Enter!
            if let RuntimeConfig::Webview {
                entry,
                utools_compat,
                width,
                height,
            } = &plug.manifest.runtime
            {
                let entry_path = plug.dir.join(entry).to_string_lossy().into_owned();
                let compat_tag = if *utools_compat {
                    "uTools Bridge"
                } else {
                    "WebView"
                };
                items.push(ResultItem {
                    id: format!("webview:{}", plug.manifest.id),
                    provider_id: plug.manifest.id.clone(),
                    title: format!(
                        "Open {} {}",
                        plug.manifest.name,
                        if ctx.args.trim().is_empty() {
                            String::new()
                        } else {
                            format!("— {}", ctx.args.trim())
                        }
                    )
                    .trim()
                    .to_string(),
                    subtitle: format!(
                        "{} · Ephemeral WebView Host (0 idle RAM, killed on close)",
                        plug.manifest.description
                    ),
                    category: ResultCategory::Plugin,
                    score: 9_600,
                    icon: IconSpec::Monogram("WV".into()),
                    badge: Some(compat_tag.into()),
                    copy_text: None,
                    actions: vec![ActionSpec {
                        id: "open_webview".into(),
                        label: format!("Launch {} (Isolated WebView)", plug.manifest.name),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::OpenWebView {
                            plugin_id: plug.manifest.id.clone(),
                            title: plug.manifest.name.clone(),
                            entry_path,
                            utools_compat: *utools_compat,
                            initial_query: ctx.args.trim().to_string(),
                            width: *width,
                            height: *height,
                            max_memory_mb: plug.manifest.resources.max_memory_mb.max(256),
                        },
                    }],
                    preview: Some(PreviewSpec::KeyValue {
                        title: plug.manifest.name.clone(),
                        entries: vec![
                            ("Runtime".into(), "Ephemeral WebView2 Host".into()),
                            ("Background RAM".into(), "0 MB (killed immediately on close)".into()),
                            ("uTools Compat".into(), if *utools_compat { "Enabled (window.utools)" } else { "Disabled" }.into()),
                            ("Window Size".into(), format!("{width}x{height}")),
                            ("Description".into(), plug.manifest.description.clone()),
                        ],
                    }),
                });
                continue;
            }

            let timeout = Duration::from_millis(plug.manifest.resources.query_timeout_ms);
            match ensure_live(plug) {
                Ok(spawned) => {
                    any_spawned |= spawned;
                }
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
                Some(LiveRuntime::Wasm(wasm)) => wasm
                    .query(ctx)
                    .map_err(|e| ProcessError::Spawn(e.to_string())),
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
                    let any_alive = st.plugins.values().any(|p| p.live.is_some());
                    if !any_alive {
                        // Zero CPU wakeups when no plugins are active!
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
        let st = lock.lock().unwrap();
        st.keyword_index.contains_key(&keyword.to_lowercase())
    }

    fn query_keyword(&self, keyword: &str, ctx: &QueryContext) -> Vec<ResultItem> {
        let ids = {
            let (lock, _) = &*self.state;
            let st = lock.lock().unwrap();
            st.keyword_index
                .get(&keyword.to_lowercase())
                .cloned()
                .unwrap_or_default()
        };
        if ids.is_empty() {
            return Vec::new();
        }
        self.query_plugin_ids(&ids, ctx)
    }

    fn query_global(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let ids: Vec<String> = {
            let (lock, _) = &*self.state;
            let st = lock.lock().unwrap();
            st.plugins
                .values()
                .filter(|p| p.manifest.triggers.match_all)
                .map(|p| p.manifest.id.clone())
                .collect()
        };
        if ids.is_empty() {
            return Vec::new();
        }
        self.query_plugin_ids(&ids, ctx)
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

/// Default plugin search roots: `./plugins`, `<exe_dir>/plugins`, and `%LOCALAPPDATA%\Tomoyo\plugins`.
pub fn default_plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        let p = cwd.join("plugins");
        if p.is_dir() {
            dirs.push(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("plugins");
            if p.is_dir() && !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    }
    let user_dir = tomoyo_core::data_dir().join("plugins");
    if !dirs.contains(&user_dir) {
        dirs.push(user_dir);
    }
    dirs
}

pub fn discover_from(path: &Path) -> Arc<PluginHost> {
    PluginHost::new(vec![path.to_path_buf()])
}
