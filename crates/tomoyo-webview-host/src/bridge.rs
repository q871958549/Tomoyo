//! Dispatches incoming WebView2 host messages safely to native OS helpers, DB, and Plugin Store.

use crate::bridge_os::{notify_parent_mode, set_clipboard_text, shell_open};
use crate::db::PluginDb;
use crate::store::{install_plugin_from_source, uninstall_user_plugin};
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
    SaveSettings { config: tomoyo_core::TomoyoConfig, autostart: bool },
    SaveFlows { flows: Vec<tomoyo_core::WorkflowDef> },
    PickFolder,
    PickCaptureDir,
    PickSyncDir,
    ClearSyncDir,
    StartCapture { #[serde(default)] mode: String },
    SyncTheme { theme: String },
    TogglePinTopmost,
    CheckUpdates { repo: String },
    InstallUpdate { release: tomoyo_core::ReleaseInfo },
    FetchStore { sources: Vec<String> },
    InstallPlugin { plugin_id: String, source: String, sha256: String },
    UninstallPlugin { plugin_id: String },
    SetPluginPermission { plugin_id: String, permission: String, allowed: bool },
    ReportJsError { message: String, #[serde(default)] source: String, #[serde(default)] line: u32 },
    ReadLogs { source: String, #[serde(default)] errors_only: bool },
    CopyDiagnosticBundle,
    OpenLogsDir,
    OpenClipboardManager,
    ClipList { #[serde(default)] query: String },
    ClipTogglePin { id_key: String },
    ClipUpdate { id_key: String, #[serde(default)] text: Option<String>, #[serde(default)] tag: String, #[serde(default)] note: String },
    ClipDelete { id_key: String },
    ClipClearUnpinned,
    ClipExport,
    ClipImport { json: String },
    OpenTranslator { text: String, #[serde(default)] target_lang: String },
    RunTranslate { text: String, #[serde(default)] source_lang: String, #[serde(default)] target_lang: String },
    TrSaveWord { word: String, translation: String, #[serde(default)] phonetic: String, #[serde(default)] note: String },
    TrDeleteWord { word: String },
    TrClearHistory,
    SetSecret { key: String, value: String },
    DeleteSecret { key: String },
    CheckHotkeyHealth,
    StartHotkeyCapture,
    StopHotkeyCapture,
    AuditHotkeys,
    ProbeHotkeyOwner { spec: String },
    RevealPath { path: String },
    ExportConfigBackup,
    ImportConfigBackup { json: String },
    Close,
}

#[derive(Debug, Default)]
pub struct HostAction {
    pub should_close: bool,
    pub apply_theme: Option<String>,
    pub toggle_topmost: bool,
    /// `0` = none, `1` = app scan folder, `2` = capture save folder.
    pub pick_folder_mode: u8,
    pub reply_json: Option<String>,
    pub async_check_updates: Option<String>,
    pub async_install_update: Option<tomoyo_core::ReleaseInfo>,
    pub async_fetch_store: Option<Vec<String>>,
    pub async_translate: Option<(String, String, String)>,
    pub async_refresh_hotkey_health: bool,
    /// `1` = start temporary exclusive WH_KEYBOARD_LL capture, `-1` = stop.
    pub hotkey_capture_cmd: i8,
    pub async_audit_hotkeys: bool,
    pub async_probe_hotkey: Option<String>,
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
            let notes = config.sanitize();
            let theme = config.theme.clone();
            let _ = config.save();
            let _ = tomoyo_core::set_autostart_enabled(autostart);
            if let Some(target) = notify_hwnd { notify_parent_reload(target, false); }
            let evt = serde_json::json!({ "event": "settings_saved", "config": config, "notes": notes }).to_string();
            HostAction {
                apply_theme: Some(theme),
                reply_json: Some(evt),
                async_refresh_hotkey_health: notify_hwnd.is_some(),
                ..Default::default()
            }
        }
        HostMessage::SaveFlows { flows } => {
            let (reply, ok) = crate::backup::save_flows_reply(&flows);
            if ok { if let Some(t) = notify_hwnd { notify_parent_reload(t, true); } }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::PickFolder => HostAction { pick_folder_mode: 1, ..Default::default() },
        HostMessage::PickCaptureDir => HostAction { pick_folder_mode: 2, ..Default::default() },
        HostMessage::PickSyncDir => HostAction { pick_folder_mode: 3, ..Default::default() },
        HostMessage::ClearSyncDir => {
            let (reply, ok) = crate::backup::clear_sync_dir_reply();
            if ok { if let Some(t) = notify_hwnd { notify_parent_reload(t, true); } }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::StartCapture { mode } => {
            crate::bridge_os::spawn_capture_tool(&mode);
            HostAction::default()
        }
        HostMessage::SyncTheme { theme } => HostAction { apply_theme: Some(theme), ..Default::default() },
        HostMessage::TogglePinTopmost => HostAction { toggle_topmost: true, ..Default::default() },
        HostMessage::CheckUpdates { repo } => {
            let safe_repo = if tomoyo_core::is_valid_github_repo(&repo) {
                repo.trim().to_string()
            } else {
                "q871958549/Tomoyo".into()
            };
            HostAction {
                async_check_updates: Some(safe_repo),
                ..Default::default()
            }
        }
        HostMessage::InstallUpdate { release } => HostAction {
            async_install_update: Some(release),
            ..Default::default()
        },
        HostMessage::FetchStore { sources } => HostAction {
            async_fetch_store: Some(sources),
            ..Default::default()
        },
        HostMessage::InstallPlugin {
            plugin_id,
            source,
            sha256,
        } => {
            let res = install_plugin_from_source(&plugin_id, &source, &sha256);
            if res.is_ok() {
                if let Some(target) = notify_hwnd {
                    notify_parent_reload(target, true);
                }
            }
            let evt = match res {
                Ok(dest) => serde_json::json!({
                    "event": "store_action_result",
                    "ok": true,
                    "action": "install",
                    "plugin_id": plugin_id,
                    "message": format!("SHA-256 校验通过，已安装到 {dest}")
                }),
                Err(e) => serde_json::json!({
                    "event": "store_action_result",
                    "ok": false,
                    "action": "install",
                    "plugin_id": plugin_id,
                    "message": e
                }),
            };
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::UninstallPlugin { plugin_id } => {
            let res = uninstall_user_plugin(&plugin_id);
            if res.is_ok() {
                if let Some(target) = notify_hwnd {
                    notify_parent_reload(target, true);
                }
            }
            let evt = match res {
                Ok(()) => serde_json::json!({
                    "event": "store_action_result",
                    "ok": true,
                    "action": "uninstall",
                    "plugin_id": plugin_id,
                    "message": "已卸载并热重载主窗口插件列表"
                }),
                Err(e) => serde_json::json!({
                    "event": "store_action_result",
                    "ok": false,
                    "action": "uninstall",
                    "plugin_id": plugin_id,
                    "message": e
                }),
            };
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::SetPluginPermission {
            plugin_id,
            permission,
            allowed,
        } => {
            let mut msg = "权限参数无效".to_string();
            let mut ok = false;
            if let Some(perm) = tomoyo_core::permission_from_key(&permission) {
                let mut pstore = tomoyo_core::PermissionStore::load_default();
                pstore.set_permission(&plugin_id, perm, allowed);
                if let Err(e) = pstore.save_default() {
                    msg = format!("保存权限失败: {e}");
                } else {
                    if let Some(target) = notify_hwnd {
                        notify_parent_reload(target, true);
                    }
                    ok = true;
                    msg = if allowed {
                        format!("已授予 {plugin_id} 权限 `{permission}`")
                    } else {
                        format!("已撤销 {plugin_id} 权限 `{permission}`")
                    };
                }
            }
            let evt = serde_json::json!({
                "event": "perm_action_result",
                "ok": ok,
                "message": msg,
            });
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::ReportJsError {
            message,
            source,
            line,
        } => {
            tracing::error!(target: "webview_js", source = %source, line = line, "JS error: {message}");
            HostAction::default()
        }
        HostMessage::ReadLogs {
            source,
            errors_only,
        } => {
            let safe_file = match source.as_str() {
                "webview.log" => "webview.log",
                "last_crash.txt" => "last_crash.txt",
                _ => "tomoyo.log",
            };
            let content = tomoyo_core::read_log_tail(safe_file, 260, errors_only);
            let (warnings, errors, has_crash_report) = tomoyo_core::count_recent_log_issues();
            let evt = serde_json::json!({
                "event": "logs_loaded",
                "source": safe_file,
                "errors_only": errors_only,
                "content": content,
                "issues": {
                    "warnings": warnings,
                    "errors": errors,
                    "has_crash_report": has_crash_report,
                },
                "logs_dir": tomoyo_core::logs_dir().to_string_lossy(),
            });
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::CopyDiagnosticBundle => {
            let bundle = tomoyo_core::build_diagnostic_bundle();
            set_clipboard_text(&bundle);
            let evt = serde_json::json!({
                "event": "diag_bundle_copied",
                "ok": true,
                "bytes": bundle.len(),
            });
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::OpenLogsDir => {
            let dir = tomoyo_core::logs_dir();
            let _ = std::fs::create_dir_all(&dir);
            shell_open(&dir.to_string_lossy());
            HostAction::default()
        }
        HostMessage::OpenClipboardManager => {
            if let Ok(exe) = std::env::current_exe() {
                let theme = tomoyo_core::TomoyoConfig::load_or_create().theme;
                let mut cmd = std::process::Command::new(exe);
                cmd.args([
                    "--plugin-id", "builtin.clipboard",
                    "--title", "Tomoyo 剪贴板工作台",
                    "--entry", crate::clip_view::CLIP_MANAGER_ENTRY,
                    "--theme", &theme,
                    "--width", "1060", "--height", "680",
                ]);
                if let Some(h) = notify_hwnd {
                    cmd.args(["--notify-hwnd", &h.to_string()]);
                }
                let _ = cmd.spawn();
            }
            HostAction::default()
        }
        HostMessage::ClipList { query } => HostAction {
            reply_json: Some(crate::clip_view::handle_clip_list(&query)),
            ..Default::default()
        },
        HostMessage::ClipTogglePin { id_key } => {
            let (reply, mutated) = crate::clip_view::handle_clip_toggle_pin(&id_key);
            if mutated { if let Some(t) = notify_hwnd { notify_parent_mode(t, 2); } }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::ClipUpdate { id_key, text, tag, note } => {
            let (reply, mutated) = crate::clip_view::handle_clip_update(&id_key, text, &tag, &note);
            if mutated { if let Some(t) = notify_hwnd { notify_parent_mode(t, 2); } }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::ClipDelete { id_key } => {
            let (reply, mutated) = crate::clip_view::handle_clip_delete(&id_key);
            if mutated { if let Some(t) = notify_hwnd { notify_parent_mode(t, 2); } }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::ClipClearUnpinned => {
            let (reply, mutated) = crate::clip_view::handle_clip_clear_unpinned();
            if mutated { if let Some(t) = notify_hwnd { notify_parent_mode(t, 2); } }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::ClipExport => HostAction {
            reply_json: Some(crate::clip_view::handle_clip_export()),
            ..Default::default()
        },
        HostMessage::ClipImport { json } => {
            let (reply, mutated) = crate::clip_view::handle_clip_import(&json);
            if mutated { if let Some(t) = notify_hwnd { notify_parent_mode(t, 2); } }
            HostAction { reply_json: Some(reply), ..Default::default() }
        }
        HostMessage::OpenTranslator { text, target_lang } => {
            crate::tr_view::open_or_spawn_translator(&text, &target_lang, notify_hwnd);
            HostAction::default()
        }
        HostMessage::RunTranslate { text, source_lang, target_lang } => HostAction {
            async_translate: Some((text, source_lang, target_lang)),
            ..Default::default()
        },
        HostMessage::TrSaveWord { word, translation, phonetic, note } => HostAction { reply_json: Some(crate::tr_view::handle_tr_word_save(&word, &translation, &phonetic, &note)), ..Default::default() },
        HostMessage::TrDeleteWord { word } => HostAction { reply_json: Some(crate::tr_view::handle_tr_word_delete(&word)), ..Default::default() },
        HostMessage::TrClearHistory => HostAction { reply_json: Some(crate::tr_view::handle_tr_history_clear()), ..Default::default() },
        HostMessage::SetSecret { key, value } => HostAction { reply_json: Some(crate::tr_view::handle_set_secret(&key, &value)), ..Default::default() },
        HostMessage::DeleteSecret { key } => HostAction { reply_json: Some(crate::tr_view::handle_delete_secret(&key)), ..Default::default() },
        HostMessage::CheckHotkeyHealth => HostAction {
            reply_json: Some(serde_json::json!({ "event": "hotkey_health", "health": tomoyo_core::load_hotkey_health() }).to_string()),
            ..Default::default()
        },
        HostMessage::StartHotkeyCapture => HostAction { hotkey_capture_cmd: 1, ..Default::default() },
        HostMessage::StopHotkeyCapture => HostAction { hotkey_capture_cmd: -1, ..Default::default() },
        HostMessage::AuditHotkeys => HostAction { async_audit_hotkeys: true, ..Default::default() },
        HostMessage::ProbeHotkeyOwner { spec } => HostAction { async_probe_hotkey: Some(spec), ..Default::default() },
        HostMessage::RevealPath { path } => {
            #[cfg(windows)]
            crate::hotkey_doctor::reveal_file_in_explorer(&path);
            let _ = path;
            HostAction::default()
        }
        HostMessage::ExportConfigBackup => HostAction {
            reply_json: Some(crate::backup::export_desensitized_backup_json()),
            ..Default::default()
        },
        HostMessage::ImportConfigBackup { json } => {
            let (reply, ok) = crate::backup::import_desensitized_backup_json(&json);
            if ok {
                if let Some(t) = notify_hwnd { notify_parent_reload(t, true); }
            }
            HostAction { reply_json: Some(reply), async_refresh_hotkey_health: ok && notify_hwnd.is_some(), ..Default::default() }
        }
        HostMessage::Close => HostAction { should_close: true, ..Default::default() },
    }
}

fn notify_parent_reload(raw_hwnd: isize, reload_plugins: bool) {
    notify_parent_mode(raw_hwnd, usize::from(reload_plugins));
}

#[cfg(test)]
mod tests {
    use crate::shim_js::build_init_script;

    #[test]
    fn generates_tomoyo_and_utools_bridge() {
        let script = build_init_script("dev.test.json", "{\"a\":1}", true, true, "{}");
        assert!(script.contains("window.tomoyo ="));
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
