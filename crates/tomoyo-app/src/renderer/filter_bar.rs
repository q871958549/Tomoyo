//! Search result category filter bar: responsive multi-row wrap, single-row `+N` overflow & live counts.
#![cfg_attr(windows, allow(dead_code))]

use super::canvas::Canvas;
#[cfg(windows)]
use super::gdi::{dip_rect, draw_text_gdi, FontPack};
use super::theme::ThemePalette;
#[cfg(not(windows))]
use std::time::Instant;
use tomoyo_protocol::{
    extension_short_tag, IconSpec, ResultCategory, ResultItem, SearchFilter, VisibleFilterTab,
};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{DT_CENTER, HDC};

pub const FILTER_BAR_HEIGHT_DIP: i32 = 36;
const CHIP_H_DIP: f32 = 22.0;
const ROW_PITCH_DIP: f32 = 27.0;
const MAX_WRAP_ROWS: usize = 3;

#[derive(Debug, Clone)]
pub struct FilterChipHitbox {
    pub filter: SearchFilter,
    pub label: String,
    pub x0: f32, pub y0: f32, pub x1: f32, pub y1: f32,
}

#[derive(Debug, Clone)]
pub struct FilterOverflowHitbox {
    pub label: String,
    pub x0: f32, pub y0: f32, pub x1: f32, pub y1: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterHit { Tab(SearchFilter), Overflow }

#[derive(Debug, Clone)]
pub struct FilterBarLayout {
    pub visible: Vec<FilterChipHitbox>,
    pub overflow_pill: Option<FilterOverflowHitbox>,
    pub overflow_tabs: Vec<VisibleFilterTab>,
    pub active_in_overflow: bool,
    #[cfg_attr(not(test), allow(dead_code))]
    pub rows: usize,
    pub height_dip: i32,
}

impl FilterBarLayout {
    pub fn hit_test(&self, x_dip: f32, y_dip: f32) -> Option<FilterHit> {
        if let Some(ov) = &self.overflow_pill
            && (ov.x0..=ov.x1).contains(&x_dip) && (ov.y0..=ov.y1).contains(&y_dip) {
            return Some(FilterHit::Overflow);
        }
        self.visible.iter()
            .find(|b| (b.x0..=b.x1).contains(&x_dip) && (b.y0..=b.y1).contains(&y_dip))
            .map(|hb| FilterHit::Tab(hb.filter))
    }
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct FilterPillAnim {
    from_x0: f32,
    from_x1: f32,
    to_x0: f32,
    to_x1: f32,
    started: Instant,
}

#[cfg(not(windows))]
impl FilterPillAnim {
    const DURATION_MS: f32 = 150.0;
    pub fn from_spans(from: Option<(f32, f32)>, to: Option<(f32, f32)>) -> Option<Self> {
        let ((fx0, fx1), (tx0, tx1)) = (from?, to?);
        ((fx0 - tx0).abs() > 1.0 || (fx1 - tx1).abs() > 1.0).then(|| Self {
            from_x0: fx0,
            from_x1: fx1,
            to_x0: tx0,
            to_x1: tx1,
            started: Instant::now(),
        })
    }
    pub fn span_now(&self) -> (f32, f32, bool) {
        let raw = (self.started.elapsed().as_secs_f32() * 1000.0 / Self::DURATION_MS).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - raw).powi(3);
        (
            self.from_x0 + (self.to_x0 - self.from_x0) * ease,
            self.from_x1 + (self.to_x1 - self.from_x1) * ease,
            raw >= 1.0,
        )
    }
}

pub type FilterCounts = Vec<(SearchFilter, usize)>;

pub fn item_matches_filter(
    item: &ResultItem,
    filter: SearchFilter,
    custom_tabs: &[VisibleFilterTab],
) -> bool {
    if filter == SearchFilter::All {
        return true;
    }
    if let SearchFilter::Custom(_) = filter {
        if item.category != ResultCategory::File {
            return false;
        }
        let exts = custom_tabs
            .iter()
            .find(|t| t.filter == filter)
            .map(|t| t.extensions.as_slice())
            .unwrap_or(&[]);
        if exts.is_empty() {
            return true;
        }
        let path = match &item.icon {
            IconSpec::ShellPath(p) => p.as_str(),
            _ => return false,
        };
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        return !ext.is_empty() && exts.iter().any(|candidate| ext.eq_ignore_ascii_case(candidate));
    }
    match filter {
        SearchFilter::All => true,
        SearchFilter::App => item.category == ResultCategory::App,
        SearchFilter::Folder => item.id.starts_with("ev_dir:"),
        SearchFilter::Custom(_) => false,
        other => {
            if item.category != ResultCategory::File {
                return false;
            }
            let path = match &item.icon {
                IconSpec::ShellPath(p) => p.as_str(),
                _ => return false,
            };
            let ext = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if ext.is_empty() {
                return false;
            }
            let short = extension_short_tag(&ext.to_ascii_lowercase());
            match other {
                SearchFilter::Pdf => short == "PDF",
                SearchFilter::Excel => short == "XLS",
                SearchFilter::Word => short == "DOC",
                SearchFilter::Ppt => short == "PPT",
                SearchFilter::Image => short == "IMG",
                SearchFilter::Archive => short == "ZIP",
                _ => false,
            }
        }
    }
}

pub fn compute_filter_counts(items: &[ResultItem], tabs: &[VisibleFilterTab]) -> FilterCounts {
    tabs.iter()
        .map(|tab| {
            let n = if tab.filter == SearchFilter::All {
                items.len()
            } else {
                items.iter().filter(|it| item_matches_filter(it, tab.filter, tabs)).count()
            };
            (tab.filter, n)
        })
        .collect()
}

pub fn chip_display_label(
    tab: &VisibleFilterTab,
    tab_idx: usize,
    counts: Option<&FilterCounts>,
) -> String {
    let n = counts
        .and_then(|list| list.iter().find(|(f, _)| *f == tab.filter).map(|(_, c)| *c))
        .unwrap_or(0);
    if n > 0 && (tab_idx < 3 || tab.filter.is_file_only()) {
        format!("{} {n}", tab.label)
    } else {
        tab.label.clone()
    }
}

fn measure_chip_w(label: &str, min_w: f32, max_w: f32) -> f32 {
    let text_w: f32 = label
        .chars()
        .map(|c| if c.is_ascii() { 6.8 } else { 12.2 })
        .sum();
    (text_w + 16.0).clamp(min_w, max_w)
}

/// Computes multi-row wrapped OR single-row `+N` overflow chip layout inside `[14 .. list_w_dip - 14]`.
pub fn compute_filter_bar_layout(
    list_w_dip: f32,
    header_h: f32,
    tabs: &[VisibleFilterTab],
    active_filter: SearchFilter,
    counts: Option<&FilterCounts>,
    wrap_rows: bool,
) -> FilterBarLayout {
    if tabs.is_empty() {
        return FilterBarLayout {
            visible: Vec::new(),
            overflow_pill: None,
            overflow_tabs: Vec::new(),
            active_in_overflow: false,
            rows: 1,
            height_dip: FILTER_BAR_HEIGHT_DIP,
        };
    }

    let min_x = 14.0f32;
    let max_x = (list_w_dip - 14.0).max(200.0);
    let max_rows = if wrap_rows { MAX_WRAP_ROWS } else { 1 };
    let gap = 5.0f32;

    let mut placed: Vec<(usize, FilterChipHitbox)> = Vec::with_capacity(tabs.len());
    let mut row: usize = 0;
    let mut x = min_x;
    let mut first_overflow_idx: Option<usize> = None;

    for (idx, tab) in tabs.iter().enumerate() {
        let label = chip_display_label(tab, idx, counts);
        let pill_w = measure_chip_w(&label, 38.0, 118.0);
        if x + pill_w > max_x && x > min_x {
            if row + 1 < max_rows {
                row += 1;
                x = min_x;
            } else {
                first_overflow_idx = Some(idx);
                break;
            }
        }
        let y0 = header_h + 7.0 + (row as f32) * ROW_PITCH_DIP;
        placed.push((row, FilterChipHitbox { filter: tab.filter, label, x0: x, y0, x1: x + pill_w, y1: y0 + CHIP_H_DIP }));
        x += pill_w + gap;
    }

    let mut overflow_tabs = Vec::new();
    let mut overflow_pill = None;
    let mut active_in_overflow = false;

    if let Some(split_idx) = first_overflow_idx {
        let mut kept_overflow = tabs[split_idx..].to_vec();
        let last_row = row;
        let est_active = tabs.iter().skip(split_idx).find(|t| t.filter == active_filter);
        let probe_lbl = est_active
            .map(|act| format!("+{} · {} ▾", kept_overflow.len(), act.label))
            .unwrap_or_else(|| format!("更多 +{} ▾", kept_overflow.len()));
        let reserve_w = measure_chip_w(&probe_lbl, 64.0, 132.0);
        while let Some((r, last)) = placed.last()
            && *r == last_row
            && placed.iter().filter(|(rr, _)| *rr == last_row).count() > 1
            && last.x1 + gap + reserve_w > max_x
        {
            let (_, popped) = placed.pop().unwrap();
            if let Some(orig) = tabs.iter().find(|t| t.filter == popped.filter) {
                kept_overflow.insert(0, orig.clone());
            }
        }
        active_in_overflow = kept_overflow.iter().any(|t| t.filter == active_filter);
        let final_lbl = kept_overflow
            .iter()
            .find(|t| t.filter == active_filter)
            .map(|act| format!("{} (+{}) ▾", act.label, kept_overflow.len()))
            .unwrap_or_else(|| format!("+{} ▾", kept_overflow.len()));
        let pill_w = measure_chip_w(&final_lbl, 52.0, 136.0);
        let row_end_x = placed.iter().filter(|(r, _)| *r == last_row).map(|(_, b)| b.x1 + gap).next_back().unwrap_or(min_x);
        let ox0 = row_end_x.min((max_x - pill_w).max(min_x));
        let oy0 = header_h + 7.0 + (last_row as f32) * ROW_PITCH_DIP;
        overflow_pill = Some(FilterOverflowHitbox { label: final_lbl, x0: ox0, y0: oy0, x1: (ox0 + pill_w).min(max_x), y1: oy0 + CHIP_H_DIP });
        overflow_tabs = kept_overflow;
    }

    let used_rows = (placed.iter().map(|(r, _)| *r + 1).max().unwrap_or(1)).max(1);
    let height_dip = if used_rows <= 1 {
        FILTER_BAR_HEIGHT_DIP
    } else {
        (14 + used_rows * (ROW_PITCH_DIP as usize) - 5) as i32
    };

    FilterBarLayout {
        visible: placed.into_iter().map(|(_, b)| b).collect(),
        overflow_pill,
        overflow_tabs,
        active_in_overflow,
        rows: used_rows,
        height_dip,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn paint_filter_bar_sdf(
    canvas: &mut Canvas<'_>,
    pal: ThemePalette,
    list_w_dip: f32,
    header_h: f32,
    tabs: &[VisibleFilterTab],
    active: SearchFilter,
    hovered: Option<SearchFilter>,
    hovered_overflow: bool,
    counts: Option<&FilterCounts>,
    wrap_rows: bool,
    pill_override: Option<(f32, f32)>,
) {
    let layout = compute_filter_bar_layout(list_w_dip, header_h, tabs, active, counts, wrap_rows);
    let bar_h = layout.height_dip as f32;
    canvas.fill_rect_dip(12.0, header_h, list_w_dip - 12.0, header_h + 1.0, pal.divider);
    canvas.fill_rect_dip(12.0, header_h + bar_h - 1.0, list_w_dip - 12.0, header_h + bar_h, pal.divider);

    for hb in &layout.visible {
        let is_act = hb.filter == active && pill_override.is_none();
        let is_hov = hovered == Some(hb.filter);
        let (bg, bd) = if is_act {
            (pal.chip_active_bg, pal.chip_active_border)
        } else if is_hov {
            (pal.chip_hover_bg, pal.chip_hover_border)
        } else {
            (pal.chip_bg, pal.chip_border)
        };
        canvas.fill_rounded_rect(hb.x0, hb.y0, hb.x1, hb.y1, 6.0, bg);
        canvas.stroke_rounded_rect(hb.x0, hb.y0, hb.x1, hb.y1, 6.0, bd);
    }

    if let Some((px0, px1)) = pill_override {
        canvas.fill_rounded_rect(px0, header_h + 7.0, px1, header_h + 29.0, 6.0, pal.chip_active_bg);
        canvas.stroke_rounded_rect(px0, header_h + 7.0, px1, header_h + 29.0, 6.0, pal.chip_active_border);
    }

    if let Some(ov) = &layout.overflow_pill {
        let (bg, bd) = if layout.active_in_overflow {
            (pal.chip_active_bg, pal.chip_active_border)
        } else if hovered_overflow {
            (pal.chip_hover_bg, pal.chip_hover_border)
        } else {
            (pal.pill_bg, pal.chip_border)
        };
        canvas.fill_rounded_rect(ov.x0, ov.y0, ov.x1, ov.y1, 6.0, bg);
        canvas.stroke_rounded_rect(ov.x0, ov.y0, ov.x1, ov.y1, 6.0, bd);
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub fn paint_filter_bar_gdi(
    dc: HDC,
    fonts: &FontPack,
    pal: ThemePalette,
    list_w_dip: f32,
    header_h: f32,
    scale: f32,
    tabs: &[VisibleFilterTab],
    active: SearchFilter,
    hovered: Option<SearchFilter>,
    hovered_overflow: bool,
    counts: Option<&FilterCounts>,
    wrap_rows: bool,
) {
    let layout = compute_filter_bar_layout(list_w_dip, header_h, tabs, active, counts, wrap_rows);
    unsafe {
        for hb in &layout.visible {
            let is_act = hb.filter == active;
            let is_hov = hovered == Some(hb.filter);
            let (fnt, col) = if is_act {
                (fonts.chip_active, pal.chip_active_text)
            } else if is_hov {
                (fonts.chip, pal.chip_hover_text)
            } else {
                (fonts.chip, pal.chip_text)
            };
            draw_text_gdi(dc, fnt, col, &hb.label, dip_rect(hb.x0, hb.y0, hb.x1, hb.y1, scale), DT_CENTER);
        }
        if let Some(ov) = &layout.overflow_pill {
            let (fnt, col) = if layout.active_in_overflow {
                (fonts.chip_active, pal.chip_active_text)
            } else if hovered_overflow {
                (fonts.chip, pal.chip_hover_text)
            } else {
                (fonts.chip, pal.text_secondary)
            };
            draw_text_gdi(dc, fnt, col, &ov.label, dip_rect(ov.x0, ov.y0, ov.x1, ov.y1, scale), DT_CENTER);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_many_tabs_up_to_three_rows_and_collapses_rest_to_overflow() {
        let tabs: Vec<VisibleFilterTab> = (0..32)
            .map(|i| VisibleFilterTab {
                filter: SearchFilter::Custom(i as u8),
                label: format!("自定义标签{i}"),
                extensions: vec!["rs".into()],
            })
            .collect();
        let wrap = compute_filter_bar_layout(540.0, 62.0, &tabs, SearchFilter::Custom(30), None, true);
        assert_eq!(wrap.rows, 3);
        assert!(wrap.height_dip > FILTER_BAR_HEIGHT_DIP);
        assert!(wrap.overflow_pill.is_some());
        assert!(!wrap.overflow_tabs.is_empty());
        assert!(wrap.active_in_overflow);

        let single = compute_filter_bar_layout(540.0, 62.0, &tabs, SearchFilter::Custom(0), None, false);
        assert_eq!(single.rows, 1);
        assert_eq!(single.height_dip, FILTER_BAR_HEIGHT_DIP);
        assert!(single.overflow_pill.is_some());
        assert!(!single.active_in_overflow);
    }
}
