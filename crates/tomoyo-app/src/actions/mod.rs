//! Unified native Windows action router.

#[cfg(windows)]
mod clipboard_win;
#[cfg(windows)]
mod clipboard_win_img;
#[cfg(target_os = "linux")]
#[path = "clipboard_linux.rs"]
mod clipboard_win;
mod file_ops;
#[cfg(windows)]
mod shell_win;
#[cfg(target_os = "linux")]
#[path = "shell_linux.rs"]
mod shell_win;

#[cfg(target_os = "linux")]
pub use clipboard_win::{
    clipboard_has_image, is_clipboard_confidential, read_primary_selection_text,
    set_clipboard_file_uri,
};
#[cfg(windows)]
pub use clipboard_win::{
    decode_and_cache_dib_bytes, open_clipboard_owned, read_clipboard_files_or_text_once,
    read_clipboard_snapshot_once, set_clipboard_owner_hwnd, ClipboardCaptureSnapshot,
};
pub use clipboard_win::{
    clipboard_has_rich_attachment, clipboard_source_app_name, copy_image_as_base64_uri,
    read_clipboard_files, read_clipboard_image_png, read_clipboard_text,
    set_clipboard_image_from_png, set_clipboard_text,
};
pub use file_ops::{
    compute_file_sha256_bounded, copy_file_contents_bounded, extract_zip_into_sibling_dir,
};
#[cfg(target_os = "linux")]
pub use shell_win::send_ctrl_c_cleanly;
pub use shell_win::{
    kill_process_by_pid, paste_into_previous_window_async, shell_open, shell_open_as_admin,
    spawn_ephemeral_webview_ex, spawn_settings_window_at,
};

use shell_win::{focus_window, open_terminal_at, reveal_in_file_manager, run_system_command};
use std::sync::Arc;
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{ActionKind, ExecuteRequest, InternalCommandKind};

#[derive(Debug)]
pub struct ActionOutcome {
    pub hide_window: bool,
    pub paste_after_hide: bool,
    pub refresh_query: bool,
    pub toast: Option<String>,
    pub fill_query: Option<String>,
    pub internal: Option<InternalCommandKind>,
}

impl Default for ActionOutcome {
    fn default() -> Self {
        Self {
            hide_window: true,
            paste_after_hide: false,
            refresh_query: false,
            toast: None,
            fill_query: None,
            internal: None,
        }
    }
}

pub fn execute_action(
    kind: &ActionKind,
    item_id: &str,
    plugin_host: &Arc<PluginHost>,
    is_dark_theme: bool,
) -> ActionOutcome {
    match kind {
        ActionKind::LaunchPath {
            path,
            args,
            working_dir,
        } => {
            let ok = shell_open(path, args.as_deref(), working_dir.as_deref());
            if !ok {
                let tid = tomoyo_core::next_trace_id("ACT");
                tracing::warn!(trace_id = %tid, item_id = %item_id, path = %path, "LaunchPath failed");
            }
            ActionOutcome {
                hide_window: ok,
                toast: if ok { None } else { Some(format!("打开失败: {path}")) },
                ..Default::default()
            }
        }
        ActionKind::LaunchAsAdmin {
            path,
            args,
            working_dir,
        } => {
            let ok = shell_open_as_admin(path, args.as_deref(), working_dir.as_deref());
            if !ok {
                let tid = tomoyo_core::next_trace_id("ACT");
                tracing::warn!(trace_id = %tid, item_id = %item_id, path = %path, "LaunchAsAdmin failed or UAC cancelled");
            }
            ActionOutcome {
                hide_window: ok,
                toast: if ok {
                    Some("已请求以管理员身份启动".into())
                } else {
                    Some(format!("管理员启动已取消或失败: {path}"))
                },
                ..Default::default()
            }
        }
        ActionKind::RevealInExplorer { path } => {
            let ok = reveal_in_file_manager(path);
            ActionOutcome {
                hide_window: ok,
                toast: if ok { None } else { Some(format!("Failed to reveal: {path}")) },
                ..Default::default()
            }
        }
        ActionKind::OpenTerminalAt { dir } => {
            let ok = open_terminal_at(dir);
            ActionOutcome {
                hide_window: ok,
                toast: if ok { None } else { Some("Failed to open terminal".into()) },
                ..Default::default()
            }
        }
        ActionKind::OpenUrl { url } => {
            let ok = shell_open(url, None, None);
            ActionOutcome {
                hide_window: ok,
                toast: if ok { None } else { Some(format!("Failed to open URL: {url}")) },
                ..Default::default()
            }
        }
        ActionKind::CopyText {
            text,
            paste_immediately,
        } => {
            let ok = set_clipboard_text(text);
            ActionOutcome {
                hide_window: true,
                paste_after_hide: ok && *paste_immediately,
                toast: Some(if ok { "已复制到剪贴板".into() } else { "复制失败".into() }),
                ..Default::default()
            }
        }
        ActionKind::CopyImage {
            path,
            paste_immediately,
        } => {
            let ok = set_clipboard_image_from_png(path);
            ActionOutcome {
                hide_window: ok,
                paste_after_hide: ok && *paste_immediately,
                toast: Some(if ok {
                    "已复制图片到剪贴板".into()
                } else {
                    "复制图片失败（文件可能已被删除）".into()
                }),
                ..Default::default()
            }
        }
        ActionKind::FocusWindow { hwnd } => {
            focus_window(*hwnd);
            ActionOutcome { hide_window: true, ..Default::default() }
        }
        ActionKind::FillQuery { query } => ActionOutcome {
            hide_window: false,
            fill_query: Some(query.clone()),
            ..Default::default()
        },
        ActionKind::SystemCommand { command } => {
            run_system_command(*command);
            ActionOutcome { hide_window: true, ..Default::default() }
        }
        ActionKind::InternalCommand { command } => ActionOutcome {
            hide_window: false,
            internal: Some(command.clone()),
            ..Default::default()
        },
        ActionKind::PluginExecute {
            plugin_id,
            action_id,
            payload,
        } => {
            let req = ExecuteRequest::new(action_id.clone(), item_id.to_string(), payload.clone());
            match plugin_host.execute_action(plugin_id, req) {
                Ok(resp) => {
                    if let Some(txt) = &resp.copy_text {
                        let _ = set_clipboard_text(txt);
                    }
                    if let Some(url) = &resp.open_url {
                        let _ = shell_open(url, None, None);
                    }
                    if let Some(wv_req) = &resp.open_webview
                        && let Some(wv_act) = plugin_host.resolve_webview_open(plugin_id, wv_req)
                    {
                        return execute_action(&wv_act, item_id, plugin_host, is_dark_theme);
                    }
                    ActionOutcome {
                        hide_window: resp.hide_window && resp.fill_query.is_none(),
                        paste_after_hide: false,
                        refresh_query: resp.refresh_query,
                        toast: resp.toast,
                        fill_query: resp.fill_query,
                        internal: None,
                    }
                }
                Err(e) => {
                    let tid = tomoyo_core::next_trace_id("ACT");
                    tracing::error!(trace_id = %tid, plugin_id = %plugin_id, action_id = %action_id, "PluginExecute failed: {e}");
                    ActionOutcome {
                        hide_window: false,
                        toast: Some(format!("插件执行异常 #{tid}: {e}")),
                        ..Default::default()
                    }
                }
            }
        }
        ActionKind::OpenWebView {
            plugin_id,
            title,
            entry_path,
            utools_compat,
            initial_query,
            width,
            height,
            max_memory_mb,
            icon_path,
            ..
        } => match spawn_ephemeral_webview_ex(
            plugin_id,
            title,
            entry_path,
            *utools_compat,
            initial_query,
            *width,
            *height,
            *max_memory_mb,
            is_dark_theme,
            icon_path.as_deref(),
            None,
            None,
        ) {
            Ok(_) => ActionOutcome { hide_window: true, ..Default::default() },
            Err(e) => {
                let tid = tomoyo_core::next_trace_id("WV");
                tracing::error!(trace_id = %tid, plugin_id = %plugin_id, entry = %entry_path, "OpenWebView failed: {e}");
                ActionOutcome {
                    hide_window: false,
                    toast: Some(format!("WebView 启动失败 #{tid}: {e}")),
                    ..Default::default()
                }
            }
        },
    }
}
