//! Interactive Fluent scrollbar state, geometry hit-testing & pointer drag handling on Linux.

use crate::linux::shell_state::AppCore;
use crate::renderer::{
    compute_scrollbar_metrics, ScrollbarHit, ScrollbarMetrics, ScrollbarVisual,
    FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP,
};

#[derive(Default)]
pub(super) struct ScrollbarState {
    pub dragging: Option<f32>,
    pub hover: Option<ScrollbarHit>,
}

impl AppCore {
    pub(super) fn current_scrollbar_metrics(&self) -> Option<ScrollbarMetrics> {
        let (_, list_x1) = self.list_bounds_dip(self.desired_width_dip() as f32);
        let list_top = HEADER_HEIGHT_DIP as f32 + 4.0;
        let fy0 = (self.desired_height_dip() - FOOTER_HEIGHT_DIP) as f32;
        compute_scrollbar_metrics(
            list_x1,
            list_top,
            fy0,
            self.results.len(),
            self.visible_row_capacity(),
            self.scroll_offset,
            self.is_preview_visible(),
        )
    }

    pub(super) fn scrollbar_visual(&self) -> ScrollbarVisual {
        if self.scrollbar.dragging.is_some() {
            ScrollbarVisual::Dragging
        } else {
            match self.scrollbar.hover {
                Some(ScrollbarHit::Thumb) => ScrollbarVisual::HoveredThumb,
                Some(ScrollbarHit::Track) => ScrollbarVisual::HoveredTrack,
                None => ScrollbarVisual::Idle,
            }
        }
    }

    pub(super) fn handle_scrollbar_press(&mut self, xd: f32, yd: f32) -> bool {
        let Some(sb) = self.current_scrollbar_metrics() else { return false; };
        let Some(hit) = sb.hit_test(xd, yd) else { return false; };
        self.hovered = None;
        let grab_off = match hit {
            ScrollbarHit::Thumb => (yd - sb.thumb_y0).clamp(0.0, sb.thumb_height()),
            ScrollbarHit::Track => {
                let target_ty0 = sb.centered_thumb_top(yd);
                self.scroll_offset = sb.offset_from_thumb_top(target_ty0);
                sb.thumb_height() * 0.5
            }
        };
        self.scrollbar.dragging = Some(grab_off);
        self.scrollbar.hover = Some(ScrollbarHit::Thumb);
        self.invalidate();
        true
    }

    pub(super) fn handle_scrollbar_motion(&mut self, _xd: f32, yd: f32) -> bool {
        let Some(grab_off) = self.scrollbar.dragging else { return false; };
        if let Some(sb) = self.current_scrollbar_metrics() {
            let next = sb.offset_from_thumb_top(yd - grab_off);
            if next != self.scroll_offset {
                self.scroll_offset = next;
                self.invalidate();
            }
        }
        true
    }

    pub(super) fn handle_scrollbar_release(&mut self, xd: f32, yd: f32) -> bool {
        if self.scrollbar.dragging.take().is_none() {
            return false;
        }
        self.scrollbar.hover = self
            .current_scrollbar_metrics()
            .and_then(|sb| sb.hit_test(xd, yd));
        self.invalidate();
        true
    }

    pub(super) fn handle_mouse_leave(&mut self) {
        let mut changed = self.hovered.take().is_some()
            | self.hovered_filter.take().is_some()
            | std::mem::replace(&mut self.hovered_logo, false)
            | std::mem::replace(&mut self.sidebar.hovered_add_tag, false);
        if self.scrollbar.dragging.is_none() && self.scrollbar.hover.take().is_some() {
            changed = true;
        }
        if changed {
            self.invalidate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::AppShell;
    use std::sync::{Arc, RwLock};
    use tomoyo_core::{FrecencyStore, PrivateFonts, QueryRouter, TomoyoConfig, UpdateManager};
    use tomoyo_plugin_host::PluginHost;
    use tomoyo_protocol::{IconSpec, ResultCategory, ResultItem};

    fn make_results(n: usize) -> Vec<ResultItem> {
        (0..n)
            .map(|i| ResultItem {
                id: format!("item:{i}"),
                provider_id: "test".into(),
                title: format!("Item {i}"),
                subtitle: String::new(),
                category: ResultCategory::App,
                score: 1000 - i as i64,
                icon: IconSpec::Monogram("T".into()),
                badge: None,
                copy_text: None,
                actions: Vec::new(),
                preview: None,
            })
            .collect()
    }

    #[test]
    fn scrollbar_visual_transitions_and_drag_track_click_update_scroll_offset() {
        let cfg = Arc::new(RwLock::new(TomoyoConfig::default()));
        let host = PluginHost::new(Vec::new());
        let frecency = Arc::new(RwLock::new(FrecencyStore::load_or_default(
            std::env::temp_dir().join("tomoyo_sb_test_frecency.json"),
        )));
        let router = Arc::new(QueryRouter::new(Vec::new(), None, frecency));
        let updater = UpdateManager::with_cached("q871958549/Tomoyo");
        let fonts = PrivateFonts::load_default();
        let sh = AppShell::create(cfg, router, host, updater, fonts.regular_family(), fonts.medium_family(), false)
            .expect("headless shell");
        let mut c = sh.core.borrow_mut();
        c.results = make_results(32);
        assert_eq!(c.scrollbar_visual(), ScrollbarVisual::Idle);

        let sb = c.current_scrollbar_metrics().expect("scrollable metrics");
        let mid_x = (sb.hit_x0 + sb.hit_x1) * 0.5;
        c.scrollbar.hover = sb.hit_test(mid_x, sb.track_y1 - 2.0);
        assert_eq!(c.scrollbar_visual(), ScrollbarVisual::HoveredTrack);
        c.scrollbar.hover = sb.hit_test(mid_x, (sb.thumb_y0 + sb.thumb_y1) * 0.5);
        assert_eq!(c.scrollbar_visual(), ScrollbarVisual::HoveredThumb);

        assert!(c.handle_scrollbar_press(mid_x, sb.track_y1 - 4.0));
        assert_eq!(c.scrollbar_visual(), ScrollbarVisual::Dragging);
        assert!(c.scroll_offset > 0);
        let jumped = c.scroll_offset;

        assert!(c.handle_scrollbar_motion(mid_x, sb.track_y0 + 10.0));
        assert!(c.scroll_offset < jumped);

        assert!(c.handle_scrollbar_release(mid_x, sb.thumb_y0 + 4.0));
        assert_ne!(c.scrollbar_visual(), ScrollbarVisual::Dragging);
        c.handle_mouse_leave();
        assert_eq!(c.scrollbar_visual(), ScrollbarVisual::Idle);
    }
}
