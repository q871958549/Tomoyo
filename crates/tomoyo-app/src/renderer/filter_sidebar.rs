//! Scrollable left category/tag sidebar rail (`SIDEBAR_WIDTH_DIP`), SDF & GDI rendering, and hit-testing.

use super::canvas::Canvas;
use super::filter_bar::FilterCounts;
use super::gdi::{dip_rect, draw_text_gdi, FontPack};
use super::theme::Rgb;
use super::{FrameLayout, RenderState};
use tomoyo_core::Lang;
use tomoyo_protocol::{SearchFilter, VisibleFilterTab};
use windows::Win32::Graphics::Gdi::{HDC, DT_CENTER, DT_LEFT, DT_RIGHT};

pub const SIDEBAR_WIDTH_DIP: f32 = 118.0;
const TAB_ROW_H: f32 = 30.0;
const TAB_GAP: f32 = 2.0;
const ADD_AREA_H: f32 = 40.0;

pub fn should_show_filter_sidebar(
    show_pref: bool,
    has_attached: bool,
    query: &str,
    active_filter: SearchFilter,
    result_count: usize,
) -> bool {
    if !show_pref || has_attached {
        return false;
    }
    let t = query.trim();
    if t.starts_with(':') {
        return false;
    }
    let low = t.to_ascii_lowercase();
    let is_dedicated_non_file = low == "cb"
        || low.starts_with("cb ")
        || low == "w"
        || low.starts_with("w ")
        || low == "kill"
        || low.starts_with("kill ")
        || low == "ps"
        || low.starts_with("ps ")
        || low == "port"
        || low.starts_with("port ")
        || low.starts_with("= ")
        || low.starts_with("b64 ")
        || low.starts_with("b64d ");
    if is_dedicated_non_file {
        return false;
    }
    if active_filter != SearchFilter::All {
        return true;
    }
    !t.is_empty() && result_count > 6
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarHit {
    Tab(SearchFilter),
    AddTag,
    Empty,
}

#[derive(Debug, Clone)]
pub struct SidebarItemRect {
    pub filter: SearchFilter,
    pub label: String,
    pub is_custom: bool,
    pub y0: f32,
    pub y1: f32,
}

#[derive(Debug, Clone)]
pub struct SidebarLayout {
    pub items: Vec<SidebarItemRect>,
    pub list_top: f32,
    pub list_bot: f32,
    pub max_scroll: f32,
    pub add_btn: (f32, f32, f32, f32),
}

pub fn compute_sidebar_layout(
    header_h: f32,
    fy0: f32,
    tabs: &[VisibleFilterTab],
    scroll: f32,
) -> SidebarLayout {
    let list_top = header_h + 6.0;
    let list_bot = (fy0 - ADD_AREA_H).max(list_top + 24.0);
    let step = TAB_ROW_H + TAB_GAP;
    let total_h = if tabs.is_empty() {
        0.0
    } else {
        tabs.len() as f32 * step - TAB_GAP
    };
    let view_h = (list_bot - list_top).max(1.0);
    let max_scroll = (total_h - view_h).max(0.0);
    let clamped_scroll = scroll.clamp(0.0, max_scroll);

    let items = tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let y0 = list_top + (i as f32) * step - clamped_scroll;
            SidebarItemRect {
                filter: tab.filter,
                label: tab.label.clone(),
                is_custom: matches!(tab.filter, SearchFilter::Custom(_)),
                y0,
                y1: y0 + TAB_ROW_H,
            }
        })
        .collect();

    SidebarLayout {
        items,
        list_top,
        list_bot,
        max_scroll,
        add_btn: (8.0, (fy0 - 34.0).max(list_bot + 4.0), SIDEBAR_WIDTH_DIP - 8.0, fy0 - 6.0),
    }
}

pub fn hit_test_sidebar(
    x_dip: f32,
    y_dip: f32,
    header_h: f32,
    fy0: f32,
    tabs: &[VisibleFilterTab],
    scroll: f32,
) -> Option<SidebarHit> {
    if !(0.0..SIDEBAR_WIDTH_DIP).contains(&x_dip) || y_dip < header_h || y_dip >= fy0 {
        return None;
    }
    let sl = compute_sidebar_layout(header_h, fy0, tabs, scroll);
    let (ax0, ay0, ax1, ay1) = sl.add_btn;
    if x_dip >= ax0 && x_dip <= ax1 && y_dip >= ay0 && y_dip <= ay1 {
        return Some(SidebarHit::AddTag);
    }
    if y_dip >= sl.list_top && y_dip <= sl.list_bot {
        for item in &sl.items {
            if y_dip >= item.y0 && y_dip <= item.y1 && (6.0..=SIDEBAR_WIDTH_DIP - 6.0).contains(&x_dip) {
                return Some(SidebarHit::Tab(item.filter));
            }
        }
    }
    Some(SidebarHit::Empty)
}

fn count_for_filter(counts: Option<&FilterCounts>, filter: SearchFilter) -> usize {
    counts
        .and_then(|c| c.iter().find(|(f, _)| *f == filter).map(|(_, n)| *n))
        .unwrap_or(0)
}

pub(super) fn paint_sidebar_sdf(
    canvas: &mut Canvas<'_>,
    lay: &FrameLayout,
    state: &RenderState<'_>,
) {
    let pal = state.theme;
    canvas.fill_rect_dip(0.0, lay.header_h, SIDEBAR_WIDTH_DIP, lay.fy0, pal.preview_bg);
    canvas.fill_rect_dip(0.0, lay.header_h, lay.w_dip, lay.header_h + 1.0, pal.divider);
    canvas.fill_rect_dip(SIDEBAR_WIDTH_DIP - 1.0, lay.header_h, SIDEBAR_WIDTH_DIP, lay.fy0, pal.divider);

    let sl = compute_sidebar_layout(lay.header_h, lay.fy0, state.filter_tabs, state.sidebar_scroll);
    for item in &sl.items {
        if item.y1 < sl.list_top + 4.0 || item.y0 > sl.list_bot - 4.0 {
            continue;
        }
        let is_active = item.filter == state.active_filter;
        let is_hover = state.hovered_filter == Some(item.filter);
        if is_active {
            canvas.fill_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.chip_active_bg);
            canvas.stroke_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.chip_active_border);
            canvas.fill_rounded_rect(7.5, item.y0 + 7.0, 10.5, item.y1 - 7.0, 1.5, pal.accent);
        } else if is_hover {
            canvas.fill_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.chip_hover_bg);
            canvas.stroke_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.chip_hover_border);
        }
    }

    canvas.fill_rect_dip(8.0, lay.fy0 - ADD_AREA_H, SIDEBAR_WIDTH_DIP - 8.0, lay.fy0 - ADD_AREA_H + 1.0, pal.divider);
    let (ax0, ay0, ax1, ay1) = sl.add_btn;
    let (btn_bg, btn_bd) = if state.hovered_add_tag {
        (pal.chip_active_bg, pal.accent)
    } else {
        (pal.pill_bg, pal.border)
    };
    canvas.fill_rounded_rect(ax0, ay0, ax1, ay1, 7.0, btn_bg);
    canvas.stroke_rounded_rect(ax0, ay0, ax1, ay1, 7.0, btn_bd);
}

pub(super) fn paint_sidebar_gdi(
    dc: HDC,
    fonts: &FontPack,
    lay: &FrameLayout,
    state: &RenderState<'_>,
    scale: f32,
) {
    let pal = state.theme;
    let sl = compute_sidebar_layout(lay.header_h, lay.fy0, state.filter_tabs, state.sidebar_scroll);
    unsafe {
        for item in &sl.items {
            if item.y1 < sl.list_top + 4.0 || item.y0 > sl.list_bot - 4.0 {
                continue;
            }
            let is_active = item.filter == state.active_filter;
            let is_hover = state.hovered_filter == Some(item.filter);
            let n = count_for_filter(state.filter_counts, item.filter);
            let label_right = if n > 0 {
                let cnt = n.to_string();
                let cnt_col = if is_active { pal.accent } else { pal.text_muted };
                draw_text_gdi(
                    dc,
                    fonts.footer,
                    cnt_col,
                    &cnt,
                    dip_rect(SIDEBAR_WIDTH_DIP - 36.0, item.y0, SIDEBAR_WIDTH_DIP - 13.0, item.y1, scale),
                    DT_RIGHT,
                );
                SIDEBAR_WIDTH_DIP - 38.0
            } else if item.is_custom {
                draw_text_gdi(
                    dc,
                    fonts.footer,
                    if is_active { pal.accent } else { pal.text_muted },
                    "#",
                    dip_rect(SIDEBAR_WIDTH_DIP - 24.0, item.y0, SIDEBAR_WIDTH_DIP - 12.0, item.y1, scale),
                    DT_RIGHT,
                );
                SIDEBAR_WIDTH_DIP - 24.0
            } else {
                SIDEBAR_WIDTH_DIP - 13.0
            };

            let (fnt, col) = if is_active {
                (fonts.chip_active, pal.chip_active_text)
            } else if is_hover {
                (fonts.chip, pal.chip_hover_text)
            } else {
                (fonts.chip, pal.chip_text)
            };
            draw_text_gdi(
                dc,
                fnt,
                col,
                &item.label,
                dip_rect(16.0, item.y0, label_right, item.y1, scale),
                DT_LEFT,
            );
        }

        let (ax0, ay0, ax1, ay1) = sl.add_btn;
        let add_lbl = match state.lang {
            Lang::Zh => "+ 添加标签",
            Lang::En => "+ Add Tag",
        };
        let add_col = if state.hovered_add_tag { pal.accent } else { pal.text_primary };
        let add_fnt = if state.hovered_add_tag { fonts.chip_active } else { fonts.chip };
        draw_text_gdi(dc, add_fnt, add_col, add_lbl, dip_rect(ax0, ay0, ax1, ay1, scale), DT_CENTER);
    }
    let _ = Rgb(0, 0, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tabs(n: usize) -> Vec<VisibleFilterTab> {
        (0..n)
            .map(|i| VisibleFilterTab {
                filter: if i == 0 { SearchFilter::All } else { SearchFilter::Custom(i as u8) },
                label: format!("Tag{i}"),
                extensions: vec!["rs".into()],
            })
            .collect()
    }

    #[test]
    fn hit_tests_tabs_add_button_and_scroll_bounds() {
        let tabs = sample_tabs(12);
        let sl = compute_sidebar_layout(62.0, 312.0, &tabs, 0.0);
        assert!(sl.max_scroll > 0.0);
        assert_eq!(
            hit_test_sidebar(50.0, sl.items[0].y0 + 10.0, 62.0, 312.0, &tabs, 0.0),
            Some(SidebarHit::Tab(SearchFilter::All))
        );
        let (_, ay0, _, ay1) = sl.add_btn;
        assert_eq!(
            hit_test_sidebar(50.0, (ay0 + ay1) * 0.5, 62.0, 312.0, &tabs, 0.0),
            Some(SidebarHit::AddTag)
        );
        assert_eq!(hit_test_sidebar(150.0, 120.0, 62.0, 312.0, &tabs, 0.0), None);
    }

    #[test]
    fn sidebar_only_appears_for_abundant_results_or_active_filter() {
        assert!(!should_show_filter_sidebar(true, false, "", SearchFilter::All, 8));
        assert!(!should_show_filter_sidebar(true, false, "foo", SearchFilter::All, 6));
        assert!(should_show_filter_sidebar(true, false, "foo", SearchFilter::All, 7));
        assert!(!should_show_filter_sidebar(true, false, ":stats", SearchFilter::All, 12));
        assert!(!should_show_filter_sidebar(true, false, "cb ", SearchFilter::All, 15));
        assert!(!should_show_filter_sidebar(true, true, "foo", SearchFilter::All, 15));
        assert!(!should_show_filter_sidebar(false, false, "foo", SearchFilter::All, 15));
        assert!(should_show_filter_sidebar(true, false, "", SearchFilter::Pdf, 2));
    }
}
