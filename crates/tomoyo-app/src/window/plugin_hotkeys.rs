//! Protocol v2 declarative plugin & workflow global hotkeys + non-blocking event dispatch.

use super::input::register_optional_hotkey;
use super::AppShell;
use std::sync::Arc;
use tomoyo_core::{load_or_init_flows, parse_hotkey, TomoyoConfig};
use tomoyo_plugin_host::{ActivePluginHotkey, PluginHost};
use tomoyo_protocol::{PluginEventKind, PluginEventPayload};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindowVisible};

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

/// Handles a triggered Protocol v2 plugin hotkey (`id >= PLUGIN_HOTKEY_BASE`).
pub(super) fn handle_plugin_hotkey(app: &mut AppShell, id: i32) {
    let idx = (id - PLUGIN_HOTKEY_BASE) as usize;
    let Some(hk) = app.plugin_hotkeys.get(idx).cloned() else {
        return;
    };
    if !hk.trigger_query.trim().is_empty() {
        app.show_launcher_with_query(Some(&hk.trigger_query));
        return;
    }
    let payload = PluginEventPayload {
        event: PluginEventKind::HotkeyTriggered,
        data: serde_json::json!({
            "hotkey_id": hk.hotkey_id,
            "spec": hk.spec,
        }),
    };
    let resps = app.plugin_host.dispatch_event(Some(&hk.plugin_id), &payload);
    if let Some(resp) = resps.into_iter().next() {
        if let Some(q) = resp.fill_query {
            app.show_launcher_with_query(Some(&q));
        }
        if let Some(toast) = resp.toast {
            app.show_toast(&toast);
        }
    }
}

/// Dispatches a `ClipboardText` Protocol v2 event off the UI thread ONLY when a subscriber exists.
pub(super) fn dispatch_clipboard_text_event(app: &AppShell, text: &str, source_app: &str) {
    if !app.plugin_host.has_event_subscriber(PluginEventKind::ClipboardText) {
        return;
    }
    let host = Arc::clone(&app.plugin_host);
    let payload = PluginEventPayload {
        event: PluginEventKind::ClipboardText,
        data: serde_json::json!({
            "text": text,
            "source_app": source_app,
        }),
    };
    let _ = std::thread::Builder::new()
        .name("tomoyo-plugin-event".into())
        .spawn(move || {
            let _ = host.dispatch_event(None, &payload);
        });
}
