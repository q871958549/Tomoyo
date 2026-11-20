//! Active runtime handle wrapper (`LiveRuntime`) & capability-gated lazy spawning (`ensure_live`).

use crate::permissions::authorize_plugin_runtime;
use crate::process::{LivePluginProcess, ProcessError};
use crate::wasm::LiveWasmPlugin;
use std::path::PathBuf;
use std::time::Instant;
use tomoyo_core::{inspect_pid_working_set, PermissionStore};
use tomoyo_protocol::{Permission, PluginManifest, RuntimeConfig};

pub(crate) enum LiveRuntime {
    Stdio(LivePluginProcess),
    Wasm(LiveWasmPlugin),
}

impl LiveRuntime {
    pub fn last_used(&self) -> Instant {
        match self {
            Self::Stdio(p) => p.last_used(),
            Self::Wasm(w) => w.last_used(),
        }
    }

    pub fn pid(&self) -> Option<u32> {
        match self {
            Self::Stdio(p) => Some(p.pid()),
            Self::Wasm(_) => None,
        }
    }

    pub fn memory_bytes(&self) -> Option<u64> {
        match self {
            Self::Stdio(p) => inspect_pid_working_set(p.pid()),
            Self::Wasm(w) => Some(w.linear_memory_bytes()),
        }
    }

    pub fn shutdown(&mut self) {
        if let Self::Stdio(p) = self {
            p.shutdown_gracefully();
        }
    }
}

pub(crate) struct RegisteredPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
    pub live: Option<LiveRuntime>,
}

pub(crate) fn ensure_live(
    plug: &mut RegisteredPlugin,
    perms: &mut PermissionStore,
    interactive: bool,
) -> Result<bool, ProcessError> {
    if plug.live.is_some() {
        return Ok(false);
    }
    let granted_perms: Vec<Permission> =
        authorize_plugin_runtime(perms, &plug.manifest, &plug.dir, interactive)
            .map_err(ProcessError::Spawn)?;
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
                granted_perms,
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
