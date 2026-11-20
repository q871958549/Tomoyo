//! Clipboard Center Win32 event handling: privacy filtering, confidential format guard,
//! dedicated `Alt+V` launcher toggle, and ephemeral Clipboard Workbench spawning.

use super::AppShell;
use crate::actions::{
    clipboard_source_app_name, is_clipboard_confidential, read_clipboard_files,
    read_clipboard_image_png, read_clipboard_text, spawn_clipboard_manager_window,
};
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

impl AppShell {
    pub fn open_clipboard_workbench(&mut self, focus_key: Option<&str>) {
        match spawn_clipboard_manager_window(self.hwnd.0 as isize, &self.theme_name, focus_key) {
            Ok(()) => self.hide_launcher(),
            Err(e) => self.show_toast(&format!("剪贴板工作台启动失败: {e}")),
        }
    }

    pub fn toggle_clipboard_launcher(&mut self) {
        let visible = unsafe { IsWindowVisible(self.hwnd).as_bool() };
        let in_cb = self
            .current_query
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("cb");
        if visible && in_cb {
            self.hide_launcher();
        } else {
            if !visible {
                self.show_launcher();
            }
            self.set_query_text("cb ");
        }
    }

    pub(super) fn on_clipboard_db_external_change(&mut self) {
        self.clipboard.reload_from_db();
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.dispatch_query();
            self.invalidate();
        }
    }

    pub(super) fn on_clipboard_update(&mut self) {
        let (capture_enabled, capture_images, excluded) = self
            .config
            .read()
            .map(|c| {
                (
                    c.clipboard.capture_enabled,
                    c.clipboard.capture_images,
                    c.clipboard.excluded_apps.clone(),
                )
            })
            .unwrap_or((true, true, Vec::new()));

        if !capture_enabled {
            return;
        }
        if is_clipboard_confidential() {
            tracing::debug!("Skipped confidential clipboard item (password manager marker)");
            return;
        }
        let src_app = clipboard_source_app_name();
        if !src_app.is_empty() && excluded.iter().any(|e| e.eq_ignore_ascii_case(&src_app)) {
            tracing::debug!(source_app = %src_app, "Skipped clipboard from privacy-excluded app");
            return;
        }

        if let Some(files) = read_clipboard_files() {
            self.inter.suppress_next_clip_img = false;
            self.clipboard.push_files_with_source(files, &src_app);
        } else if let Some(txt) = read_clipboard_text() {
            self.inter.suppress_next_clip_img = false;
            self.clipboard.push_text_with_source(txt, &src_app);
        } else if self.inter.suppress_next_clip_img {
            self.inter.suppress_next_clip_img = false;
        } else if capture_images {
            if let Some((path, w, h, digest)) = read_clipboard_image_png() {
                self.clipboard.push_image_with_source(path, w, h, digest, &src_app);
            }
        }
    }

    pub(super) fn trigger_selected_clipboard_shortcut(&mut self, action_id: &str) -> bool {
        let Some(item) = self.results.get(self.selected) else {
            return false;
        };
        if item.provider_id != "builtin.clipboard" {
            return false;
        }
        if let Some(idx) = item.actions.iter().position(|a| a.id == action_id) {
            self.execute_selected(idx);
            return true;
        }
        false
    }
}
