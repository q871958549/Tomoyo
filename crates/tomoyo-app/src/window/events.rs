//! Async query dispatch, result/icon/preview callbacks, and main frame rendering bridge.

use super::bootstrap::read_edit_text;
use super::{AppShell, QueryJob};
use crate::renderer::{compute_filter_counts, RenderState};
use tomoyo_core::Lang;
use tomoyo_protocol::{build_visible_filter_tabs, SearchFilter, VisibleFilterTab};
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

impl AppShell {
    pub(super) fn dispatch_query(&mut self) {
        let generation = self.router.next_generation();
        let _ = self.query_tx.send(QueryJob {
            generation,
            query: self.current_query.clone(),
            clipboard: self.last_clip_snapshot.clone(),
            attached: self.attached.clone(),
            filter: self.active_filter,
        });
    }

    pub fn visible_filter_tabs(&self) -> Vec<VisibleFilterTab> {
        let zh = self.lang == Lang::Zh;
        self.config
            .read()
            .map(|c| build_visible_filter_tabs(zh, &c.hidden_builtin_filters, &c.custom_filters))
            .unwrap_or_else(|_| build_visible_filter_tabs(zh, &[], &[]))
    }

    pub(super) fn on_results_ready(&mut self) {
        let mut latest = None;
        while let Ok(out) = self.result_rx.try_recv() { latest = Some(out); }
        let Some(out) = latest else { return };
        if self.router.is_stale(out.generation) { return; }
        let prev_sel_id = if self.last_result_gen == out.generation && !out.is_partial {
            self.results.get(self.selected).map(|i| i.id.clone())
        } else { None };
        self.last_result_gen = out.generation;
        self.results = out.items;
        self.inject_context_clipboard_card();
        self.inject_active_launches_into_empty_results();
        if self.active_filter == SearchFilter::All {
            let tabs = self.visible_filter_tabs();
            self.inter.filter_counts = Some(compute_filter_counts(&self.results, &tabs));
        }
        self.selected = prev_sel_id.and_then(|id| self.results.iter().position(|r| r.id == id)).unwrap_or(0);
        self.inter.hovered = None;
        self.scroll_offset = 0;
        self.action_menu_open = false;
        self.selected_action = 0;
        self.cached_preview_id = None;
        self.refresh_selected_preview();
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.resize_to_fit_results();
            self.invalidate();
        }
    }

    pub(super) fn on_icons_ready(&mut self) {
        if self.renderer.drain_ready_icons() && unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.invalidate();
        }
    }

    pub(super) fn on_preview_ready(&mut self) {
        let Some(out) = self.preview_loader.drain_latest() else {
            return;
        };
        if self.results.get(self.selected).map(|i| i.id.as_str()) != Some(out.item_id.as_str()) {
            return;
        }
        self.cached_preview_id = Some(out.item_id);
        self.cached_preview = Some(out.preview);
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.resize_to_fit_results();
            self.invalidate();
        }
    }

    pub(super) fn on_edit_changed(&mut self) {
        if super::is_edit_change_suppressed() {
            return;
        }
        self.cancel_pending_launch_hide();
        self.current_query = read_edit_text(self.edit_hwnd);
        self.inter.filter_counts = None;
        self.dispatch_query();
    }

    pub(super) fn render_into_hdc(&mut self, hdc: HDC, width: i32, height: i32) {
        let mode = self.active_mode_badge();
        let attached_label = self.attached_capsule_label();
        let update_label = self.updater.available_release().map(|r| format!("↑ v{}", r.version));
        let filter_vis = self.is_filter_bar_visible();
        let filter_tabs = self.visible_filter_tabs();
        let launching_row = self.resolve_launching_row();
        let scrollbar_visual = if self.inter.dragging_scrollbar.is_some() {
            crate::renderer::ScrollbarVisual::Dragging
        } else {
            match self.inter.scrollbar_hover {
                Some(crate::renderer::ScrollbarHit::Thumb) => crate::renderer::ScrollbarVisual::HoveredThumb,
                Some(crate::renderer::ScrollbarHit::Track) => crate::renderer::ScrollbarVisual::HoveredTrack,
                None => crate::renderer::ScrollbarVisual::Idle,
            }
        };
        let state = RenderState {
            lang: self.lang, theme: self.theme, query: &self.current_query, hovered_logo: self.inter.hovered_logo,
            mode_badge: mode.as_deref(), attached_capsule: attached_label.as_deref(),
            update_badge: update_label.as_deref(), filter_bar_visible: filter_vis,
            filter_bar_wrap: self.filter_bar_wrap,
            filter_tabs: &filter_tabs, active_filter: self.active_filter,
            hovered_filter: self.inter.hovered_filter, hovered_overflow: self.inter.hovered_overflow,
            filter_counts: self.inter.filter_counts.as_ref(),
            filter_pill_override: None, typography: &self.typography,
            items: &self.results, selected: self.selected, hovered: self.inter.hovered,
            launching_row,
            scroll_offset: self.scroll_offset, scrollbar_visual, max_visible_rows: self.geom.max_visible_rows,
            preview_width_dip: self.geom.preview_width_dip, action_menu_open: self.action_menu_open,
            selected_action: self.selected_action, preview: self.cached_preview.as_ref(),
            everything_status: self.everything_status, toast: self.toast.as_deref(),
            dpi_scale: self.dpi_scale,
        };
        self.renderer.paint(hdc, width, height, &state);
    }
}
