//! Zero-overhead debounced OS clipboard event dispatcher & generic result-action shortcuts.
//!
//! Coalesces multi-step clipboard bursts (OneCommander, Explorer, Excel, Word `OleSetClipboard`
//! and `Preferred DropEffect` / `OleFlushClipboard`) with a 120 ms settle timer and reads all
//! required formats in ONE `< 0.3 ms` lock so Tomoyo never triggers `CLIPBRD_E_CANT_OPEN`.

use super::AppShell;
use crate::actions::{
    clipboard_source_app_name, decode_and_cache_dib_bytes, execute_action,
    read_clipboard_snapshot_once, ClipboardCaptureSnapshot,
};
use std::sync::Arc;
use tomoyo_protocol::{ClipboardSnapshot, OpenWebViewRequest, PluginEventKind, PluginEventPayload};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, KillTimer, SetTimer};

pub(super) const TIMER_CLIP_DEBOUNCE: usize = 1009;
const CLIP_DEBOUNCE_MS: u32 = 120;

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
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_CLIP_DEBOUNCE);
                let _ = RemoveClipboardFormatListener(self.hwnd);
            }
            self.clip_listener_active = false;
        }
    }

    pub(super) fn on_clipboard_db_external_change(&mut self) {
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.dispatch_query();
            self.invalidate();
        }
    }

    /// Defers OS clipboard inspection by `CLIP_DEBOUNCE_MS` (reset on every burst notification)
    /// so multi-step writers like OneCommander / Explorer / Office finish before Tomoyo opens it.
    pub(super) fn on_clipboard_update(&mut self) {
        if self.suppress_clipboard_capture.load(std::sync::atomic::Ordering::Acquire) {
            unsafe { let _ = KillTimer(Some(self.hwnd), TIMER_CLIP_DEBOUNCE); }
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CLIP_DEBOUNCE);
            let _ = SetTimer(Some(self.hwnd), TIMER_CLIP_DEBOUNCE, CLIP_DEBOUNCE_MS, None);
        }
    }

    pub(super) fn on_clipboard_debounce_timer(&mut self) {
        unsafe { let _ = KillTimer(Some(self.hwnd), TIMER_CLIP_DEBOUNCE); }
        let sub_text = self.plugin_host.has_event_subscriber(PluginEventKind::ClipboardText);
        let sub_img = self.plugin_host.has_event_subscriber(PluginEventKind::ClipboardImage);
        if (!sub_text && !sub_img)
            || self.suppress_clipboard_capture.load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }

        let src_app = clipboard_source_app_name();
        let want_img = sub_img && !self.inter.suppress_next_clip_img;
        match read_clipboard_snapshot_once(sub_text, want_img) {
            ClipboardCaptureSnapshot::Confidential | ClipboardCaptureSnapshot::Empty => {
                if self.inter.suppress_next_clip_img {
                    self.inter.suppress_next_clip_img = false;
                }
            }
            ClipboardCaptureSnapshot::Files(files) => {
                tracing::debug!(source_app = %src_app, count = files.len(), "Captured debounced clipboard file list");
                self.inter.suppress_next_clip_img = false;
                super::plugin_hotkeys::dispatch_clipboard_event(
                    self,
                    PluginEventKind::ClipboardText,
                    serde_json::json!({ "kind": "files", "paths": files, "source_app": src_app }),
                );
            }
            ClipboardCaptureSnapshot::Text(txt) => {
                tracing::debug!(source_app = %src_app, chars = txt.chars().count(), "Captured debounced clipboard text");
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
            }
            ClipboardCaptureSnapshot::ImageDib(raw_dib) => {
                let host = Arc::clone(&self.plugin_host);
                let _ = std::thread::Builder::new()
                    .name("tomoyo-clip-img".into())
                    .spawn(move || {
                        if let Some((path, w, h, digest)) = decode_and_cache_dib_bytes(&raw_dib) {
                            let payload = PluginEventPayload::new(
                                PluginEventKind::ClipboardImage,
                                serde_json::json!({
                                    "path": path, "width": w, "height": h, "digest": digest,
                                    "source_app": src_app
                                }),
                            );
                            let _ = host.dispatch_event(None, &payload);
                        }
                    });
            }
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
