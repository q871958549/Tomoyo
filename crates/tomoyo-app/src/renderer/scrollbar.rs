//! Shared single-source-of-truth geometry & pointer hit-testing for the interactive Fluent scrollbar.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum ScrollbarVisual {
    #[default]
    Idle,
    HoveredTrack,
    HoveredThumb,
    Dragging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum ScrollbarHit {
    Thumb,
    Track,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct ScrollbarMetrics {
    pub hit_x0: f32,
    pub hit_x1: f32,
    pub track_x0: f32,
    pub track_x1: f32,
    pub track_y0: f32,
    pub track_y1: f32,
    pub thumb_y0: f32,
    pub thumb_y1: f32,
    pub max_scroll: usize,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl ScrollbarMetrics {
    pub fn thumb_height(self) -> f32 {
        (self.thumb_y1 - self.thumb_y0).max(26.0)
    }

    pub fn hit_test(self, x_dip: f32, y_dip: f32) -> Option<ScrollbarHit> {
        if x_dip < self.hit_x0 || x_dip > self.hit_x1 || y_dip < self.track_y0 || y_dip > self.track_y1 {
            return None;
        }
        if y_dip >= self.thumb_y0 - 2.0 && y_dip <= self.thumb_y1 + 2.0 {
            Some(ScrollbarHit::Thumb)
        } else {
            Some(ScrollbarHit::Track)
        }
    }

    /// Maps a desired thumb top coordinate in DIPs back into a clamped `scroll_offset`.
    pub fn offset_from_thumb_top(self, desired_ty0: f32) -> usize {
        let travel = ((self.track_y1 - self.track_y0) - self.thumb_height()).max(1.0);
        let frac = ((desired_ty0 - self.track_y0) / travel).clamp(0.0, 1.0);
        (frac * self.max_scroll as f32).round() as usize
    }

    /// Computes thumb top centered around `click_y_dip` for track click-to-jump.
    pub fn centered_thumb_top(self, click_y_dip: f32) -> f32 {
        let th = self.thumb_height();
        (click_y_dip - th * 0.5).clamp(self.track_y0, (self.track_y1 - th).max(self.track_y0))
    }
}

/// Computes unified scrollbar geometry used identically by SDF rendering and Win32 mouse hit-testing.
pub fn compute_scrollbar_metrics(
    list_w_dip: f32,
    list_top: f32,
    fy0: f32,
    total_items: usize,
    visible_cap: usize,
    scroll_offset: usize,
    has_preview: bool,
) -> Option<ScrollbarMetrics> {
    if visible_cap == 0 || total_items <= visible_cap {
        return None;
    }
    let track_y0 = list_top + 4.0;
    let track_y1 = (fy0 - 6.0).max(track_y0 + 28.0);
    let track_h = track_y1 - track_y0;
    let max_scroll = total_items - visible_cap;
    let thumb_h = (track_h * (visible_cap as f32 / total_items as f32).clamp(0.10, 1.0))
        .clamp(26.0, track_h);
    let frac = (scroll_offset.min(max_scroll) as f32 / max_scroll.max(1) as f32).clamp(0.0, 1.0);
    let thumb_y0 = track_y0 + frac * (track_h - thumb_h);
    let thumb_y1 = thumb_y0 + thumb_h;

    // Leave clean breathing room before the preview split divider so cursor never fights IDC_SIZEWE.
    let right_edge = list_w_dip - if has_preview { 4.5 } else { 2.5 };
    let track_x1 = right_edge;
    let track_x0 = right_edge - 7.5;
    let hit_x1 = list_w_dip - if has_preview { 3.5 } else { 0.5 };
    let hit_x0 = right_edge - 12.0;

    Some(ScrollbarMetrics {
        hit_x0,
        hit_x1,
        track_x0,
        track_x1,
        track_y0,
        track_y1,
        thumb_y0,
        thumb_y1,
        max_scroll,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollbar_metrics_roundtrips_offset_and_hit_tests_thumb_vs_track() {
        assert!(compute_scrollbar_metrics(720.0, 66.0, 500.0, 8, 8, 0, false).is_none());

        let m = compute_scrollbar_metrics(720.0, 66.0, 500.0, 40, 8, 16, false).unwrap();
        assert_eq!(m.max_scroll, 32);
        assert_eq!(m.offset_from_thumb_top(m.track_y0), 0);
        assert_eq!(m.offset_from_thumb_top(m.track_y1), 32);
        assert_eq!(m.offset_from_thumb_top(m.thumb_y0), 16);

        let mid_x = (m.hit_x0 + m.hit_x1) * 0.5;
        let mid_thumb_y = (m.thumb_y0 + m.thumb_y1) * 0.5;
        assert_eq!(m.hit_test(mid_x, mid_thumb_y), Some(ScrollbarHit::Thumb));
        assert_eq!(m.hit_test(mid_x, m.track_y0 + 2.0), Some(ScrollbarHit::Track));
        assert_eq!(m.hit_test(100.0, mid_thumb_y), None);
    }
}
