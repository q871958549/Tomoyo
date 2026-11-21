//! Native Windows execution adapter for declarative `flows.toml` pipelines.

use super::AppShell;
use crate::actions::{
    paste_into_previous_window_async, read_clipboard_text, set_clipboard_text, shell_open,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tomoyo_core::{is_safe_http_url, load_or_init_flows, run_flow_pipeline, sync_root_dir};

impl AppShell {
    pub(super) fn execute_workflow(&mut self, flow_id: &str, input: &str) {
        let flows = load_or_init_flows();
        let Some(flow) = flows.into_iter().find(|f| f.id.eq_ignore_ascii_case(flow_id)) else {
            self.show_toast(&format!("未找到工作流：{flow_id}"));
            return;
        };
        let clip_text = read_clipboard_text().unwrap_or_default();
        let plan = run_flow_pipeline(&flow, input, &clip_text);

        if let Some((title, body)) = plan.save_note_title_body.as_ref() {
            if let Err(e) = save_flow_markdown_note(title, body) {
                self.show_toast(&e);
                return;
            }
        }
        if let Some(txt) = plan.copy_text.as_deref() {
            let _ = set_clipboard_text(txt);
        }
        if let Some(url) = plan.open_url.as_deref() {
            if is_safe_http_url(url) {
                let _ = shell_open(url, None, None);
            }
        }
        if let Some(path) = plan.open_path.as_deref() {
            if std::path::Path::new(path).exists() {
                let _ = shell_open(path, None, None);
            }
        }
        if let Some(mode) = plan.capture_mode.as_deref() {
            self.trigger_capture(mode);
            return;
        }
        if let Some(tr_text) = plan.launch_translate.as_deref() {
            self.open_translator_window(tr_text, None);
            return;
        }
        if let Some(fill) = plan.fill_query.as_deref() {
            self.action_menu_open = false;
            self.set_query_text(fill);
            if let Some(toast) = plan.toast.as_deref() {
                self.show_toast(toast);
            }
            return;
        }

        if let Some(toast) = plan.toast.as_deref() {
            self.show_toast(toast);
        }
        if plan.hide_window {
            let prev = self.inter.prev_foreground_hwnd;
            self.hide_launcher();
            if plan.paste_after_hide {
                paste_into_previous_window_async(prev);
            }
        } else {
            self.action_menu_open = false;
            self.invalidate();
        }
    }
}

fn save_flow_markdown_note(title: &str, body: &str) -> Result<(), String> {
    let dir = sync_root_dir().join("notes");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建备忘录目录失败：{e}"))?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || ('\u{4e00}'..='\u{9fff}').contains(&c) {
                c
            } else {
                '-'
            }
        })
        .take(32)
        .collect();
    let slug_clean = slug.trim_matches('-');
    let name = if slug_clean.is_empty() { "flow-note" } else { slug_clean };
    let path = dir.join(format!("{ts}-{name}.md"));
    let md = format!("# {}\n\n{}\n", title.trim(), body.trim());
    std::fs::write(&path, md).map_err(|e| format!("写入 Markdown 备忘录失败：{e}"))
}
