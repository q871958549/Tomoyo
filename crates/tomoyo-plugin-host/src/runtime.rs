//! Active runtime handle wrapper (`LiveRuntime`) & capability-gated lazy spawning (`ensure_live`).

use crate::permissions::authorize_plugin_runtime;
use crate::process::{LivePluginProcess, ProcessError};
use crate::wasm::LiveWasmPlugin;
use std::path::PathBuf;
use std::time::Instant;
use tomoyo_core::{inspect_pid_working_set, PermissionStore};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, Permission, PluginManifest, PreviewSpec, ResultCategory,
    ResultItem, RuntimeConfig,
};

pub(crate) enum LiveRuntime {
    Stdio(LivePluginProcess),
    Wasm(Box<LiveWasmPlugin>),
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
                &plug.manifest.id,
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
            plug.live = Some(LiveRuntime::Wasm(Box::new(wasm)));
            Ok(true)
        }
        RuntimeConfig::Webview { .. } => Ok(false),
    }
}

pub(crate) fn plugin_error_result_item(
    plugin_id: &str,
    plugin_name: &str,
    trace_id: &str,
    err_detail: &str,
    stderr_lines: &[String],
) -> ResultItem {
    let stderr_block = if stderr_lines.is_empty() {
        "(无 stderr 输出)".to_string()
    } else {
        stderr_lines.join("\n")
    };
    let report = format!(
        "[Tomoyo Plugin Error #{trace_id}]\nPlugin: {plugin_name} ({plugin_id})\nError: {err_detail}\nStderr:\n{stderr_block}"
    );
    ResultItem {
        id: format!("plugin_err:{plugin_id}:{trace_id}"),
        provider_id: plugin_id.to_string(),
        title: format!("[异常 #{trace_id}] {plugin_name} 执行失败"),
        subtitle: format!("{err_detail} · Enter 复制排障信息 · Ctrl+Enter 查看 :logs"),
        category: ResultCategory::System,
        score: 9_400,
        icon: IconSpec::Monogram("ERR".into()),
        badge: Some(trace_id.to_string()),
        copy_text: Some(report.clone()),
        actions: vec![
            ActionSpec {
                id: "copy_err".into(),
                label: "复制插件异常诊断信息".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText {
                    text: report.clone(),
                    paste_immediately: false,
                },
            },
            ActionSpec {
                id: "open_logs".into(),
                label: "查看 :logs 溯源日志".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::FillQuery {
                    query: ":logs".into(),
                },
            },
        ],
        preview: Some(PreviewSpec::Text {
            title: format!("插件诊断详情 #{trace_id}"),
            body: report,
        }),
    }
}
