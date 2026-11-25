//! Protocol v2 declarative plugin & workflow global hotkeys + non-blocking event dispatch.

use super::input::register_optional_hotkey;
use super::translate_ctrl::{clipboard_seq_num, send_ctrl_c_cleanly};
use super::{AppShell, WM_APP_PLUGIN_EVENT_DONE};
use crate::actions::{execute_action, read_clipboard_text, set_clipboard_text, shell_open};
use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tomoyo_core::{is_safe_http_url, load_or_init_flows, parse_hotkey, TomoyoConfig};
use tomoyo_plugin_host::{ActivePluginHotkey, PluginHost};
use tomoyo_protocol::{
    ActionKind, ExecuteResponse, Permission, PluginEventKind, PluginEventPayload,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindowVisible, PostMessageW};

pub(super) const PLUGIN_HOTKEY_BASE: i32 = 100;
const MAX_PLUGIN_HOTKEYS: usize = 16;
pub(super) const FLOW_HOTKEY_BASE: i32 = 200;
const MAX_FLOW_HOTKEYS: usize = 16;

/// Registers active global hotkeys declared by enabled Protocol v2 plugins.
/// Conflicted slots are automatically appended to `override_entries` when auto-rescue is enabled.
pub(super) fn register_plugin_hotkeys(
    hwnd: HWND,
    cfg: &TomoyoConfig,
    plugin_host: &PluginHost,
    override_entries: &mut Vec<(i32, u32, u32)>,
) -> Vec<ActivePluginHotkey> {
    let list = plugin_host.active_plugin_hotkeys(cfg);
    let mut registered = Vec::with_capacity(list.len().min(MAX_PLUGIN_HOTKEYS));
    for (idx, hk) in list.into_iter().take(MAX_PLUGIN_HOTKEYS).enumerate() {
        let id = PLUGIN_HOTKEY_BASE + idx as i32;
        if register_optional_hotkey(hwnd, id, &hk.spec) {
            tracing::info!(plugin = %hk.plugin_id, hotkey = %hk.spec, id = id, "Registered plugin hotkey");
        } else if cfg.override_conflict_hotkeys
            && let Some(p) = parse_hotkey(&hk.spec) {
                override_entries.push((id, p.modifiers, p.vk));
                tracing::info!(plugin = %hk.plugin_id, hotkey = %hk.spec, id = id, "Queued plugin hotkey for conflict rescue");
            } else {
                tracing::warn!(plugin = %hk.plugin_id, hotkey = %hk.spec, id = id, "Plugin hotkey registration failed (already in use)");
            }
        registered.push(hk);
    }
    registered
}

/// Registers active global hotkeys bound to enabled declarative workflows (`flows.toml`).
pub(super) fn register_flow_hotkeys(
    hwnd: HWND,
    cfg: &TomoyoConfig,
    override_entries: &mut Vec<(i32, u32, u32)>,
) -> Vec<String> {
    let mut registered = Vec::new();
    for flow in load_or_init_flows().into_iter().filter(|f| f.enabled && !f.hotkey.trim().is_empty()) {
        if registered.len() >= MAX_FLOW_HOTKEYS {
            break;
        }
        let id = FLOW_HOTKEY_BASE + registered.len() as i32;
        let spec = flow.hotkey.trim();
        if register_optional_hotkey(hwnd, id, spec) {
            tracing::info!(flow = %flow.id, hotkey = %spec, id = id, "Registered workflow hotkey");
        } else if cfg.override_conflict_hotkeys
            && let Some(p) = parse_hotkey(spec) {
                override_entries.push((id, p.modifiers, p.vk));
                tracing::info!(flow = %flow.id, hotkey = %spec, id = id, "Queued workflow hotkey for conflict rescue");
            } else {
                tracing::warn!(flow = %flow.id, hotkey = %spec, id = id, "Workflow hotkey registration failed (already in use)");
            }
        registered.push(flow.id);
    }
    registered
}

pub(super) fn unregister_plugin_hotkeys(hwnd: HWND) {
    unsafe {
        for i in 0..MAX_PLUGIN_HOTKEYS as i32 {
            let _ = UnregisterHotKey(Some(hwnd), PLUGIN_HOTKEY_BASE + i);
        }
        for i in 0..MAX_FLOW_HOTKEYS as i32 {
            let _ = UnregisterHotKey(Some(hwnd), FLOW_HOTKEY_BASE + i);
        }
    }
}

/// Handles a triggered declarative workflow global hotkey (`id >= FLOW_HOTKEY_BASE`).
pub(super) fn handle_flow_hotkey(app: &mut AppShell, id: i32) {
    let idx = (id - FLOW_HOTKEY_BASE) as usize;
    let Some(flow_id) = app.flow_hotkeys.get(idx).cloned() else {
        return;
    };
    if !unsafe { IsWindowVisible(app.hwnd).as_bool() } {
        let fg = unsafe { GetForegroundWindow() };
        if !fg.is_invalid() && fg != app.hwnd {
            app.inter.prev_foreground_hwnd = fg.0 as isize;
        }
    }
    app.execute_workflow(&flow_id, "");
}

fn apply_plugin_exec_response(app: &mut AppShell, plugin_id: &str, resp: ExecuteResponse) {
    if let Some(txt) = resp.copy_text.as_deref() {
        let _ = set_clipboard_text(txt);
    }
    if let Some(url) = resp.open_url.as_deref() && is_safe_http_url(url) {
        let _ = shell_open(url, None, None);
    }
    if let Some(wv_req) = &resp.open_webview
        && let Some(wv_act) = app.plugin_host.resolve_webview_open(plugin_id, wv_req)
    {
        let out = execute_action(&wv_act, plugin_id, &app.plugin_host, app.theme.is_dark);
        if let Some(t) = out.toast { app.show_toast(&t); }
        if out.hide_window { app.hide_launcher(); }
        return;
    }
    if let Some(q) = resp.fill_query {
        app.show_launcher_with_query(Some(&q));
    }
    if let Some(toast) = resp.toast {
        app.show_toast(&toast);
    }
}

pub(super) fn on_async_plugin_event_done(app: &mut AppShell) {
    let taken = app.pending_plugin_event.lock().ok().and_then(|mut g| g.take());
    if let Some((plugin_id, resp)) = taken {
        apply_plugin_exec_response(app, &plugin_id, resp);
    }
}

pub(super) fn execute_plugin_tray_action(app: &mut AppShell, idx: usize) {
    let Some(act) = app.tray_plugin_actions.get(idx).cloned() else {
        return;
    };
    if !act.trigger_query.trim().is_empty() {
        app.show_launcher_with_query(Some(&act.trigger_query));
        return;
    }
    let kind = if let Some(ui_act) = act.ui_action {
        ui_act
    } else {
        ActionKind::PluginExecute {
            plugin_id: act.plugin_id.clone(),
            action_id: act.action_id,
            payload: serde_json::json!({ "source": "tray", "tray_action_id": act.id }),
        }
    };
    let out = execute_action(&kind, &format!("tray:{}", act.id), &app.plugin_host, app.theme.is_dark);
    if let Some(q) = out.fill_query {
        app.show_launcher_with_query(Some(&q));
    }
    if let Some(toast) = out.toast {
        app.show_toast(&toast);
    }
}

/// Handles a triggered Protocol v2/v3 plugin hotkey (`id >= PLUGIN_HOTKEY_BASE`).
pub(super) fn handle_plugin_hotkey(app: &mut AppShell, id: i32) {
    let idx = (id - PLUGIN_HOTKEY_BASE) as usize;
    let Some(hk) = app.plugin_hotkeys.get(idx).cloned() else {
        return;
    };
    if !hk.trigger_query.trim().is_empty() {
        app.show_launcher_with_query(Some(&hk.trigger_query));
        return;
    }
    if hk.capture_selection {
        if !app.plugin_host.plugin_has_permission(&hk.plugin_id, Permission::SelectionRead) {
            app.show_toast(&format!("插件 {} 未获授「划词读取 (SelectionRead)」权限", hk.plugin_id));
            return;
        }
        let suppress_flag = Arc::clone(&app.suppress_clipboard_capture);
        let host = Arc::clone(&app.plugin_host);
        let slot = Arc::clone(&app.pending_plugin_event);
        let hwnd_raw = app.hwnd.0 as usize;
        let _ = std::thread::Builder::new()
            .name("tomoyo-sel-plugin".into())
            .spawn(move || {
                suppress_flag.store(true, Ordering::Release);
                let prev_text = read_clipboard_text();
                let seq_before = clipboard_seq_num();
                send_ctrl_c_cleanly();
                let mut captured = String::new();
                let mut changed = false;
                for _ in 0..12 {
                    std::thread::sleep(Duration::from_millis(15));
                    let mut cur_seq = clipboard_seq_num();
                    if cur_seq != seq_before {
                        changed = true;
                        for _ in 0..3 {
                            std::thread::sleep(Duration::from_millis(30));
                            let next_seq = clipboard_seq_num();
                            if next_seq == cur_seq {
                                break;
                            }
                            cur_seq = next_seq;
                        }
                        if let Some(txt) = read_clipboard_text() {
                            captured = txt;
                        }
                        break;
                    }
                }
                if changed && let Some(old) = prev_text {
                    std::thread::sleep(Duration::from_millis(25));
                    let _ = set_clipboard_text(&old);
                }
                std::thread::sleep(Duration::from_millis(160));
                suppress_flag.store(false, Ordering::Release);
                let payload = PluginEventPayload::new(
                    PluginEventKind::HotkeyTriggered,
                    serde_json::json!({
                        "hotkey_id": hk.hotkey_id,
                        "spec": hk.spec,
                        "selected_text": captured.trim(),
                    }),
                );
                if let Some(resp) = host.dispatch_event(Some(&hk.plugin_id), &payload).into_iter().next() {
                    if let Ok(mut g) = slot.lock() {
                        *g = Some((hk.plugin_id.clone(), resp));
                    }
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(hwnd_raw as *mut c_void)),
                            WM_APP_PLUGIN_EVENT_DONE,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            });
        return;
    }
    let payload = PluginEventPayload::new(
        PluginEventKind::HotkeyTriggered,
        serde_json::json!({
            "hotkey_id": hk.hotkey_id,
            "spec": hk.spec,
        }),
    );
    let resps = app.plugin_host.dispatch_event(Some(&hk.plugin_id), &payload);
    if let Some(resp) = resps.into_iter().next() {
        apply_plugin_exec_response(app, &hk.plugin_id, resp);
    }
}

/// Dispatches a clipboard Protocol v2/v3 event off the UI thread ONLY when a subscriber exists.
pub(super) fn dispatch_clipboard_event(
    app: &AppShell,
    kind: PluginEventKind,
    data: serde_json::Value,
) {
    if !app.plugin_host.has_event_subscriber(kind) {
        return;
    }
    let host = Arc::clone(&app.plugin_host);
    let payload = PluginEventPayload::new(kind, data);
    let _ = std::thread::Builder::new()
        .name("tomoyo-plugin-event".into())
        .spawn(move || {
            let _ = host.dispatch_event(None, &payload);
        });
}
