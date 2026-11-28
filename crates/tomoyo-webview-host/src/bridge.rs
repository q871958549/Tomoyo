//! Dispatches incoming WebView2 host messages safely to native OS helpers, DB, and Plugin Store.

use crate::bridge_ops::{
    handle_clear_logs, handle_copy_diag_bundle, handle_install_plugin, handle_open_logs_dir,
    handle_read_logs, handle_set_plugin_permission, handle_uninstall_plugin,
};
use crate::bridge_os::{notify_parent_mode, set_clipboard_text, shell_open};
use crate::db::PluginDb;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum HostMessage {
    CopyText { text: String },
    CopyAndPaste { text: String },
    OpenExternal { url: String },
    OpenPath { path: String },
    DbPut { id: String, doc: Value },
    DbRemove { id: String },
    SaveSettings { config: Box<tomoyo_core::TomoyoConfig>, autostart: bool },
    SaveFlows { flows: Vec<tomoyo_core::WorkflowDef> },
    SaveSnippets { snippets: Vec<crate::assets_ops::SnippetRow> },
    DryRunFlow { flow: tomoyo_core::WorkflowDef, #[serde(default)] sample_clip: String },
    SaveMcpServers { servers: Vec<crate::mcp_ops::McpServerRow> },
    ProbeMcpServer { server: crate::mcp_ops::McpServerRow },
    PickFolder,
    PickSyncDir,
    PickPluginPath { plugin_id: String, key: String, #[serde(default)] kind: String },
    PluginRpc { req_id: String, method: String, #[serde(default)] params: Value },
    OpenPluginUi { plugin_id: String, #[serde(default)] query: String },
    ClearSyncDir,
    SyncTheme { theme: String },
    TogglePinTopmost,
    CheckUpdates { repo: String },
    InstallUpdate { release: Box<tomoyo_core::ReleaseInfo> },
    FetchStore { sources: Vec<String>, #[serde(default)] query: String, #[serde(default)] category: String, #[serde(default)] runtime: String, #[serde(default)] sort: String, #[serde(default)] page: usize, #[serde(default)] page_size: usize, #[serde(default)] force_refresh: bool },
    FetchPluginDetail { plugin_id: String, #[serde(default)] sources: Vec<String> },
    CheckPluginHealth { #[serde(default)] sources: Vec<String> },
    CheckPluginUpdates { #[serde(default)] sources: Vec<String> },
    InstallPlugin { plugin_id: String, source: String, sha256: String },
    UninstallPlugin { plugin_id: String },
    SetPluginPermission { plugin_id: String, permission: String, allowed: bool },
    ReportJsError { message: String, #[serde(default)] source: String, #[serde(default)] line: u32 },
    ReadLogs { source: String, #[serde(default)] errors_only: bool },
    ClearDiagnosticLogs { #[serde(default)] source: String, #[serde(default)] errors_only: bool },
    CopyDiagnosticBundle,
    OpenLogsDir,
    SetSecret { key: String, value: String },
    DeleteSecret { key: String },
    CheckHotkeyHealth,
    StartHotkeyCapture,
    StopHotkeyCapture,
    AuditHotkeys,
    ProbeHotkeyOwner { spec: String },
    TerminateConflictProcess { pid: u32, process_name: String },
    RevealPath { path: String },
    ExportConfigBackup,
    ImportConfigBackup { json: String },
    OpenSettings { #[serde(default)] target: String },
    PopOutWindow,
    StartWindowDrag,
    MinimizeWindow,
    ToggleMaximizeWindow,
    WindowReady,
    Close,
}

#[derive(Debug, Default)]
pub struct HostAction {
    pub should_close: bool,
    pub window_ready: bool,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub pop_out_window: bool,
    pub window_control: u8,
    pub apply_theme: Option<String>,
    pub toggle_topmost: bool,
    /// `0` = none, `1` = app scan folder, `3` = sync folder.
    pub pick_folder_mode: u8,
    /// Protocol v3: `(plugin_id, field_key, is_file)` picker request for plugin `dir` / `file` settings.
    pub pick_plugin_path: Option<(String, String, bool)>,
    /// Protocol v3: `(req_id, method, params)` Hybrid WebView → Plugin backend RPC call.
    pub async_plugin_rpc: Option<(String, String, Value)>,
    pub reply_json: Option<String>,
    pub async_check_updates: Option<String>,
    pub async_install_update: Option<tomoyo_core::ReleaseInfo>,
    pub async_fetch_store: Option<crate::store_index::StoreQuery>,
    pub async_fetch_plugin_detail: Option<(String, Vec<String>)>,
    pub async_check_plugin_health: Option<Vec<String>>,
    pub async_check_plugin_updates: Option<Vec<String>>,
    pub async_refresh_hotkey_health: bool,
    /// `1` = start temporary exclusive WH_KEYBOARD_LL capture, `-1` = stop.
    pub hotkey_capture_cmd: i8,
    pub async_audit_hotkeys: bool,
    pub async_probe_hotkey: Option<String>,
    pub async_probe_mcp: Option<crate::mcp_ops::McpServerRow>,
}

pub fn handle_host_message(raw: &str, db: &mut PluginDb, notify_hwnd: Option<isize>) -> HostAction {
    let msg = match serde_json::from_str::<HostMessage>(raw) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Ignored malformed WebView host message: {e}");
            return HostAction::default();
        }
    };
    match msg {
        HostMessage::CopyText { text } => { set_clipboard_text(&text); HostAction::default() }
        HostMessage::CopyAndPaste { text } => {
            set_clipboard_text(&text);
            HostAction { should_close: true, ..Default::default() }
        }
        HostMessage::OpenExternal { url } => {
            if tomoyo_core::is_safe_http_url(&url) { shell_open(&url); }
            HostAction::default()
        }
        HostMessage::OpenPath { path } => {
            if std::path::Path::new(&path).exists() { shell_open(&path); }
            HostAction::default()
        }
        HostMessage::DbPut { id, doc } => { db.put(id, doc); HostAction::default() }
        HostMessage::DbRemove { id } => { db.remove(&id); HostAction::default() }
        HostMessage::SaveSettings { mut config, autostart } => {
            if config.filter_tab_order.is_empty() {
                config.filter_tab_order = tomoyo_core::TomoyoConfig::load_or_create().filter_tab_order;
            }
            let notes = config.sanitize();
            let theme = config.theme.clone();
            let _ = config.save();
            let _ = tomoyo_core::set_autostart_enabled(autostart);
            if let Some(target) = notify_hwnd {
                notify_parent_reload(target, false);
            } else if cfg!(target_os = "linux") {
                notify_parent_reload(0, false);
            }
            let evt = serde_json::json!({ "event": "settings_saved", "config": config, "notes": notes }).to_string();
            HostAction {
                apply_theme: Some(theme),
                reply_json: Some(evt),
                async_refresh_hotkey_health: notify_hwnd.is_some() || cfg!(target_os = "linux"),
                ..Default::default()
            }
        }
        HostMessage::SaveFlows { flows } => {
            let (reply, ok) = crate::backup::save_flows_reply(&flows);
            if ok && (notify_hwnd.is_some() || cfg!(target_os = "linux")) {
                notify_parent_reload(notify_hwnd.unwrap_or(0), true);
            }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::SaveSnippets { snippets } => {
            let (reply, ok) = crate::assets_ops::save_snippets_reply(&snippets);
            if ok && (notify_hwnd.is_some() || cfg!(target_os = "linux")) {
                notify_parent_reload(notify_hwnd.unwrap_or(0), true);
            }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::DryRunFlow { flow, sample_clip } => HostAction {
            reply_json: Some(crate::assets_ops::dry_run_flow_reply(&flow, &sample_clip)),
            ..Default::default()
        },
        HostMessage::SaveMcpServers { servers } => {
            let (reply, _) = crate::mcp_ops::save_mcp_servers_reply(&servers);
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::ProbeMcpServer { server } => HostAction {
            async_probe_mcp: Some(server),
            ..Default::default()
        },
        HostMessage::PickFolder => HostAction { pick_folder_mode: 1, ..Default::default() },
        HostMessage::PickSyncDir => HostAction { pick_folder_mode: 3, ..Default::default() },
        HostMessage::PickPluginPath { plugin_id, key, kind } => HostAction {
            pick_plugin_path: Some((plugin_id, key, kind.eq_ignore_ascii_case("file"))),
            ..Default::default()
        },
        HostMessage::PluginRpc { req_id, method, params } => HostAction {
            async_plugin_rpc: Some((req_id, method, params)),
            ..Default::default()
        },
        HostMessage::OpenPluginUi { plugin_id, query } => {
            crate::plugin_rpc::open_plugin_ui(&plugin_id, &query, notify_hwnd);
            HostAction::default()
        }
        HostMessage::ClearSyncDir => {
            let (reply, ok) = crate::backup::clear_sync_dir_reply();
            if ok && (notify_hwnd.is_some() || cfg!(target_os = "linux")) {
                notify_parent_reload(notify_hwnd.unwrap_or(0), true);
            }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::SyncTheme { theme } => HostAction { apply_theme: Some(theme), ..Default::default() },
        HostMessage::TogglePinTopmost => HostAction { toggle_topmost: true, ..Default::default() },
        HostMessage::CheckUpdates { repo } => {
            let safe_repo = if tomoyo_core::is_valid_github_repo(&repo) {
                repo.trim().to_string()
            } else {
                "q871958549/Tomoyo".into()
            };
            HostAction { async_check_updates: Some(safe_repo), ..Default::default() }
        }
        HostMessage::InstallUpdate { release } => HostAction {
            async_install_update: Some(*release),
            ..Default::default()
        },
        HostMessage::FetchStore { sources, query, category, runtime, sort, page, page_size, force_refresh } => HostAction {
            async_fetch_store: Some(crate::store_index::StoreQuery {
                sources,
                query,
                category,
                runtime,
                sort,
                page,
                page_size,
                force_refresh,
            }),
            ..Default::default()
        },
        HostMessage::FetchPluginDetail { plugin_id, sources } => HostAction {
            async_fetch_plugin_detail: Some((plugin_id, sources)),
            ..Default::default()
        },
        HostMessage::CheckPluginHealth { sources } => HostAction {
            async_check_plugin_health: Some(sources),
            ..Default::default()
        },
        HostMessage::CheckPluginUpdates { sources } => HostAction {
            async_check_plugin_updates: Some(sources),
            ..Default::default()
        },
        HostMessage::InstallPlugin { plugin_id, source, sha256 } => {
            handle_install_plugin(plugin_id, source, sha256, notify_hwnd)
        }
        HostMessage::UninstallPlugin { plugin_id } => handle_uninstall_plugin(plugin_id, notify_hwnd),
        HostMessage::SetPluginPermission { plugin_id, permission, allowed } => {
            handle_set_plugin_permission(plugin_id, permission, allowed, notify_hwnd)
        }
        HostMessage::ReportJsError { message, source, line } => {
            tracing::error!(target: "webview_js", source = %source, line = line, "JS error: {message}");
            HostAction::default()
        }
        HostMessage::ReadLogs { source, errors_only } => handle_read_logs(source, errors_only),
        HostMessage::ClearDiagnosticLogs { source, errors_only } => handle_clear_logs(source, errors_only),
        HostMessage::CopyDiagnosticBundle => handle_copy_diag_bundle(),
        HostMessage::OpenLogsDir => handle_open_logs_dir(),
        HostMessage::SetSecret { key, value } => HostAction { reply_json: Some(crate::plugin_rpc::handle_set_secret(&key, &value)), ..Default::default() },
        HostMessage::DeleteSecret { key } => HostAction { reply_json: Some(crate::plugin_rpc::handle_delete_secret(&key)), ..Default::default() },
        HostMessage::CheckHotkeyHealth => HostAction {
            reply_json: Some(serde_json::json!({ "event": "hotkey_health", "health": tomoyo_core::load_hotkey_health() }).to_string()),
            ..Default::default()
        },
        HostMessage::StartHotkeyCapture => HostAction { hotkey_capture_cmd: 1, ..Default::default() },
        HostMessage::StopHotkeyCapture => HostAction { hotkey_capture_cmd: -1, ..Default::default() },
        HostMessage::AuditHotkeys => HostAction { async_audit_hotkeys: true, ..Default::default() },
        HostMessage::ProbeHotkeyOwner { spec } => HostAction { async_probe_hotkey: Some(spec), ..Default::default() },
        HostMessage::TerminateConflictProcess { pid, process_name } => {
            #[cfg(windows)]
            let res = crate::hotkey_doctor::terminate_conflict_process(pid, &process_name);
            #[cfg(target_os = "linux")]
            let res = crate::linux_hotkeys::terminate_conflict_process(pid, &process_name);
            #[cfg(not(any(windows, target_os = "linux")))]
            let res: Result<String, String> = Err(format!("Unsupported platform ({pid} {process_name})"));
            let (ok, msg) = match res { Ok(m) => (true, m), Err(e) => (false, e) };
            let reply = serde_json::json!({
                "event": "conflict_process_terminated",
                "ok": ok, "pid": pid, "process_name": process_name, "message": msg
            }).to_string();
            HostAction { reply_json: Some(reply), async_audit_hotkeys: ok, ..Default::default() }
        }
        HostMessage::RevealPath { path } => {
            #[cfg(windows)]
            crate::hotkey_doctor::reveal_file_in_explorer(&path);
            #[cfg(target_os = "linux")]
            if !tomoyo_core::linux_sys::dbus_reveal_file(&path) {
                let fallback = std::path::Path::new(&path)
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or(&path);
                shell_open(fallback);
            }
            let _ = path;
            HostAction::default()
        }
        HostMessage::OpenSettings { target } => {
            crate::bridge_os::spawn_settings_center(&target, notify_hwnd);
            HostAction::default()
        }
        HostMessage::ExportConfigBackup => HostAction {
            reply_json: Some(crate::backup::export_desensitized_backup_json()),
            ..Default::default()
        },
        HostMessage::ImportConfigBackup { json } => {
            let (reply, ok) = crate::backup::import_desensitized_backup_json(&json);
            if ok && let Some(t) = notify_hwnd { notify_parent_reload(t, true); }
            HostAction { reply_json: Some(reply), async_refresh_hotkey_health: ok && notify_hwnd.is_some(), ..Default::default() }
        }
        HostMessage::PopOutWindow => HostAction { pop_out_window: true, ..Default::default() },
        HostMessage::StartWindowDrag => HostAction { window_control: 1, ..Default::default() },
        HostMessage::MinimizeWindow => HostAction { window_control: 2, ..Default::default() },
        HostMessage::ToggleMaximizeWindow => HostAction { window_control: 3, ..Default::default() },
        HostMessage::WindowReady => HostAction { window_ready: true, ..Default::default() },
        HostMessage::Close => HostAction { should_close: true, ..Default::default() },
    }
}

pub(crate) fn notify_parent_reload(raw_hwnd: isize, reload_plugins: bool) {
    notify_parent_mode(raw_hwnd, usize::from(reload_plugins));
}

#[cfg(test)]
mod tests {
    use crate::shim_js::build_init_script;

    #[test]
    fn generates_tomoyo_and_utools_bridge() {
        let script = build_init_script("dev.test.json", "{\"a\":1}", "dark", true, "{}");
        assert!(script.contains("window.tomoyo ="));
        assert!(script.contains("callPlugin:"));
        assert!(script.contains("window.utools ="));
        assert!(script.contains("onPluginEnter"));
        assert!(script.contains("setSubInput"));
        assert!(script.contains("hideMainWindowPasteText"));
        assert!(script.contains("dbStorage"));
    }

    #[test]
    fn read_logs_returns_structured_issue_counts() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_bridge_test_{}.json", std::process::id()));
        let mut db = crate::db::PluginDb::open_at(tmp.clone());
        let act = super::handle_host_message(
            r#"{"cmd":"read_logs","source":"tomoyo.log","errors_only":false}"#,
            &mut db,
            None,
        );
        let v: serde_json::Value = serde_json::from_str(act.reply_json.as_deref().unwrap()).unwrap();
        assert!(v["issues"]["warnings"].is_u64());
        assert!(v["issues"]["errors"].is_u64());
        assert!(v["issues"]["has_crash_report"].is_boolean());
        let _ = std::fs::remove_file(&tmp);
    }
}
