//! Zero-overhead conditional OS clipboard event dispatcher & generic result-action shortcuts.

use super::AppShell;
use crate::actions::{
    clipboard_source_app_name, execute_action, is_clipboard_confidential, read_clipboard_files,
    read_clipboard_image_png, read_clipboard_text,
};
use tomoyo_protocol::{ClipboardSnapshot, OpenWebViewRequest, PluginEventKind};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

impl AppShell {
    pub(super) fn sync_clipboard_listener(&mut self) {
        if self.tray.is_none() {
            return;
        }
        let need = self.plugin_host.has_event_subscriber(PluginEventKind::ClipboardText)
            || self.plugin_host.has_event_subscriber(PluginEventKind::ClipboardImage);
        if need && !self.clip_listener_active {
            unsafe { let _ = AddClipboardFormatListener(self.hwnd); }
            self.clip_listener_active = true;
        } else if !need && self.clip_listener_active {
            unsafe { let _ = RemoveClipboardFormatListener(self.hwnd); }
            self.clip_listener_active = false;
        }
    }

    pub(super) fn on_clipboard_db_external_change(&mut self) {
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.dispatch_query();
            self.invalidate();
        }
    }

    pub(super) fn on_clipboard_update(&mut self) {
        let sub_text = self.plugin_host.has_event_subscriber(PluginEventKind::ClipboardText);
        let sub_img = self.plugin_host.has_event_subscriber(PluginEventKind::ClipboardImage);
        if (!sub_text && !sub_img)
            || self.suppress_clipboard_capture.load(std::sync::atomic::Ordering::Acquire)
            || is_clipboard_confidential()
        {
            return;
        }
        let src_app = clipboard_source_app_name();
        if sub_text && let Some(files) = read_clipboard_files() {
            self.inter.suppress_next_clip_img = false;
            super::plugin_hotkeys::dispatch_clipboard_event(
                self,
                PluginEventKind::ClipboardText,
                serde_json::json!({ "kind": "files", "paths": files, "source_app": src_app }),
            );
        } else if let Some(txt) = read_clipboard_text() {
            self.inter.suppress_next_clip_img = false;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            self.last_clip_snapshot = Some(ClipboardSnapshot {
                text: txt.chars().take(2048).collect(),
                captured_at_unix: now,
            });
            if sub_text {
                super::plugin_hotkeys::dispatch_clipboard_event(
                    self,
                    PluginEventKind::ClipboardText,
                    serde_json::json!({ "kind": "text", "text": txt, "source_app": src_app }),
                );
            }
        } else if self.inter.suppress_next_clip_img {
            self.inter.suppress_next_clip_img = false;
        } else if sub_img && let Some((path, w, h, digest)) = read_clipboard_image_png() {
            super::plugin_hotkeys::dispatch_clipboard_event(
                self,
                PluginEventKind::ClipboardImage,
                serde_json::json!({
                    "path": path, "width": w, "height": h, "digest": digest, "source_app": src_app
                }),
            );
        }
    }

    pub(super) fn trigger_selected_action_by_id(&mut self, action_id: &str) -> bool {
        let Some(item) = self.results.get(self.selected) else {
            return false;
        };
        if let Some(idx) = item.actions.iter().position(|a| a.id == action_id) {
            self.execute_selected(idx);
            return true;
        }
        false
    }

    pub(super) fn open_clipboard_plugin_ui_if_installed(&mut self) {
        let req = OpenWebViewRequest::default();
        if let Some(wv_act) = self.plugin_host.resolve_webview_open(tomoyo_core::PLUGIN_CLIPBOARD, &req) {
            let out = execute_action(&wv_act, "shortcut:ctrl_e", &self.plugin_host, self.theme.is_dark);
            if let Some(t) = out.toast { self.show_toast(&t); }
            if out.hide_window { self.hide_launcher(); }
        }
    }
}
