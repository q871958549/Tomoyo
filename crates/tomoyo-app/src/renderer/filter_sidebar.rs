//! Scrollable left category/tag sidebar rail (`SIDEBAR_WIDTH_DIP`), drag-reorder visuals, SDF & GDI.

use super::canvas::Canvas;
use super::filter_bar::FilterCounts;
use super::gdi::{dip_rect, draw_text_gdi, FontPack};
use super::theme::{Rgb, ThemePalette};
use super::{FrameLayout, RenderState};
use tomoyo_core::Lang;
use tomoyo_protocol::{SearchFilter, VisibleFilterTab};
use windows::Win32::Graphics::Gdi::{HDC, DT_CENTER, DT_LEFT, DT_RIGHT};

pub const SIDEBAR_WIDTH_DIP: f32 = 118.0;
const TAB_ROW_H: f32 = 30.0;
const TAB_GAP: f32 = 2.0;
const ADD_AREA_H: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarDragVisual {
    pub dragged: SearchFilter,
    pub ghost_y0: f32,
    pub insert_slot: usize,
}

pub fn should_show_filter_sidebar(
    show_pref: bool, has_attached: bool, query: &str, active_filter: SearchFilter, result_count: usize,
) -> bool {
    if !show_pref || has_attached { return false; }
    let t = query.trim();
    if t.starts_with(':') { return false; }
    let low = t.to_ascii_lowercase();
    let is_ded = matches!(low.as_str(), "cb" | "w" | "kill" | "ps" | "port")
        || ["cb ", "w ", "kill ", "ps ", "port ", "= ", "b64 ", "b64d "].iter().any(|p| low.starts_with(p));
    if is_ded { return false; }
    if active_filter != SearchFilter::All { return true; }
    !t.is_empty() && result_count > 6
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarHit { Tab(SearchFilter), AddTag, Empty }

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

pub fn compute_sidebar_layout(header_h: f32, fy0: f32, tabs: &[VisibleFilterTab], scroll: f32) -> SidebarLayout {
    let list_top = header_h + 6.0;
    let list_bot = (fy0 - ADD_AREA_H).max(list_top + 24.0);
    let step = TAB_ROW_H + TAB_GAP;
    let total_h = if tabs.is_empty() { 0.0 } else { tabs.len() as f32 * step - TAB_GAP };
    let max_scroll = (total_h - (list_bot - list_top).max(1.0)).max(0.0);
    let clamped_scroll = scroll.clamp(0.0, max_scroll);
    let items = tabs.iter().enumerate().map(|(i, tab)| {
        let y0 = list_top + (i as f32) * step - clamped_scroll;
        SidebarItemRect {
            filter: tab.filter, label: tab.label.clone(),
            is_custom: matches!(tab.filter, SearchFilter::Custom(_)), y0, y1: y0 + TAB_ROW_H,
        }
    }).collect();
    SidebarLayout {
        items, list_top, list_bot, max_scroll,
        add_btn: (8.0, (fy0 - 34.0).max(list_bot + 4.0), SIDEBAR_WIDTH_DIP - 8.0, fy0 - 6.0),
    }
}

pub fn compute_drag_drop_slot(y_dip: f32, sl: &SidebarLayout) -> usize {
    if sl.items.len() <= 1 { return 1; }
    for (i, item) in sl.items.iter().enumerate().skip(1) {
        if y_dip < (item.y0 + item.y1) * 0.5 { return i; }
    }
    sl.items.len()
}

pub fn hit_test_sidebar(
    x_dip: f32, y_dip: f32, header_h: f32, fy0: f32, tabs: &[VisibleFilterTab], scroll: f32,
) -> Option<SidebarHit> {
    if !(0.0..SIDEBAR_WIDTH_DIP).contains(&x_dip) || y_dip < header_h || y_dip >= fy0 { return None; }
    let sl = compute_sidebar_layout(header_h, fy0, tabs, scroll);
    let (ax0, ay0, ax1, ay1) = sl.add_btn;
    if x_dip >= ax0 && x_dip <= ax1 && y_dip >= ay0 && y_dip <= ay1 { return Some(SidebarHit::AddTag); }
    if y_dip >= sl.list_top && y_dip <= sl.list_bot {
        for item in &sl.items {
            if item.y0 >= sl.list_top - 1.0 && item.y1 <= sl.list_bot + 1.0
                && y_dip >= item.y0 && y_dip <= item.y1 && (6.0..=SIDEBAR_WIDTH_DIP - 6.0).contains(&x_dip)
            {
                return Some(SidebarHit::Tab(item.filter));
            }
        }
    }
    Some(SidebarHit::Empty)
}

fn count_for_filter(counts: Option<&FilterCounts>, filter: SearchFilter) -> usize {
    counts.and_then(|c| c.iter().find(|(f, _)| *f == filter).map(|(_, n)| *n)).unwrap_or(0)
}

fn blend_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let mix = |x: u8, y: u8| ((x as f32) * (1.0 - t) + (y as f32) * t).round() as u8;
    Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

fn draw_grip_dots(canvas: &mut Canvas<'_>, x0: f32, mid_y: f32, col: Rgb) {
    for row in 0..3 {
        let dy = mid_y - 3.8 + (row as f32) * 3.1;
        for col_i in 0..2 {
            let dx = x0 + (col_i as f32) * 2.6;
            canvas.fill_rect_dip(dx, dy, dx + 1.4, dy + 1.4, col);
        }
    }
}

fn drag_line_y(drag: SidebarDragVisual, sl: &SidebarLayout) -> Option<f32> {
    let orig_idx = sl.items.iter().position(|it| it.filter == drag.dragged)?;
    if drag.insert_slot == 0 || drag.insert_slot == orig_idx || drag.insert_slot == orig_idx + 1 {
        return None;
    }
    let raw = if let Some(target) = sl.items.get(drag.insert_slot) {
        target.y0 - 1.0
    } else if let Some(last) = sl.items.last() {
        last.y1 + 1.0
    } else {
        sl.list_top
    };
    Some(raw.clamp(sl.list_top, sl.list_bot))
}

pub(super) fn paint_sidebar_sdf(canvas: &mut Canvas<'_>, lay: &FrameLayout, state: &RenderState<'_>) {
    let pal = state.theme;
    canvas.fill_rect_dip(0.0, lay.header_h, SIDEBAR_WIDTH_DIP, lay.fy0, pal.preview_bg);
    canvas.fill_rect_dip(0.0, lay.header_h, lay.w_dip, lay.header_h + 1.0, pal.divider);
    canvas.fill_rect_dip(SIDEBAR_WIDTH_DIP - 1.0, lay.header_h, SIDEBAR_WIDTH_DIP, lay.fy0, pal.divider);

    let sl = compute_sidebar_layout(lay.header_h, lay.fy0, state.filter_tabs, state.sidebar_scroll);
    let drag = state.sidebar_drag;
    for item in &sl.items {
        if item.y0 < sl.list_top - 1.0 || item.y1 > sl.list_bot + 1.0 { continue; }
        let is_dragged_src = drag.is_some_and(|d| d.dragged == item.filter);
        let is_active = item.filter == state.active_filter;
        let is_hover = drag.is_none() && state.hovered_filter == Some(item.filter);
        if is_dragged_src {
            canvas.fill_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.footer_bg);
            canvas.stroke_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.divider);
        } else if is_active {
            canvas.fill_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.chip_active_bg);
            canvas.stroke_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.chip_active_border);
            canvas.fill_rounded_rect(7.5, item.y0 + 7.0, 10.5, item.y1 - 7.0, 1.5, pal.accent);
        } else if is_hover {
            canvas.fill_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.chip_hover_bg);
            canvas.stroke_rounded_rect(7.0, item.y0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, item.y1 - 1.0, 7.0, pal.chip_hover_border);
        }
        if is_hover && item.filter != SearchFilter::All {
            draw_grip_dots(canvas, 11.0, (item.y0 + item.y1) * 0.5, blend_rgb(pal.text_muted, pal.preview_bg, 0.25));
        }
    }

    if let Some(d) = drag {
        if let Some(ly) = drag_line_y(d, &sl) {
            canvas.fill_rect_dip(12.0, ly - 1.1, SIDEBAR_WIDTH_DIP - 9.0, ly + 1.1, pal.accent);
            canvas.fill_rounded_rect(7.5, ly - 2.75, 13.0, ly + 2.75, 2.75, pal.accent);
        }
        let gy0 = d.ghost_y0.clamp(sl.list_top, (sl.list_bot - TAB_ROW_H).max(sl.list_top));
        let gy1 = gy0 + TAB_ROW_H;
        let sh1 = blend_rgb(Rgb(0, 0, 0), pal.preview_bg, if pal.is_dark { 0.55 } else { 0.85 });
        let sh2 = blend_rgb(Rgb(0, 0, 0), pal.preview_bg, if pal.is_dark { 0.72 } else { 0.91 });
        canvas.fill_rounded_rect(8.5, gy0 + 2.8, SIDEBAR_WIDTH_DIP - 6.5, gy1 + 2.0, 7.5, sh2);
        canvas.fill_rounded_rect(7.5, gy0 + 1.5, SIDEBAR_WIDTH_DIP - 7.5, gy1 + 0.5, 7.0, sh1);
        canvas.fill_rounded_rect(7.0, gy0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, gy1 - 1.0, 7.0, pal.pop_bg);
        canvas.stroke_rounded_rect(7.0, gy0 + 1.0, SIDEBAR_WIDTH_DIP - 8.0, gy1 - 1.0, 7.0, pal.accent);
        canvas.fill_rounded_rect(7.5, gy0 + 6.5, 10.5, gy1 - 6.5, 1.5, pal.accent);
        draw_grip_dots(canvas, 12.5, (gy0 + gy1) * 0.5, pal.accent);
    }

    canvas.fill_rect_dip(8.0, lay.fy0 - ADD_AREA_H, SIDEBAR_WIDTH_DIP - 8.0, lay.fy0 - ADD_AREA_H + 1.0, pal.divider);
    let (ax0, ay0, ax1, ay1) = sl.add_btn;
    let (btn_bg, btn_bd) = if state.hovered_add_tag && drag.is_none() {
        (pal.chip_active_bg, pal.accent)
    } else {
        (pal.pill_bg, pal.border)
    };
    canvas.fill_rounded_rect(ax0, ay0, ax1, ay1, 7.0, btn_bg);
    canvas.stroke_rounded_rect(ax0, ay0, ax1, ay1, 7.0, btn_bd);
}

#[allow(clippy::too_many_arguments)]
unsafe fn paint_tab_gdi_row(
    dc: HDC, fonts: &FontPack, pal: ThemePalette, item: &SidebarItemRect, y0: f32, y1: f32,
    is_active: bool, is_hover: bool, is_recessed: bool, x_left: f32, counts: Option<&FilterCounts>, scale: f32,
) {
    let n = count_for_filter(counts, item.filter);
    let muted = if is_recessed { blend_rgb(pal.text_muted, pal.preview_bg, 0.5) } else { pal.text_muted };
    let label_right = if n > 0 {
        let cnt = n.to_string();
        let col = if is_active && !is_recessed { pal.accent } else { muted };
        unsafe {
            draw_text_gdi(dc, fonts.footer, col, &cnt, dip_rect(SIDEBAR_WIDTH_DIP - 36.0, y0, SIDEBAR_WIDTH_DIP - 13.0, y1, scale), DT_RIGHT);
        }
        SIDEBAR_WIDTH_DIP - 38.0
    } else if item.is_custom {
        let col = if is_active && !is_recessed { pal.accent } else { muted };
        unsafe {
            draw_text_gdi(dc, fonts.footer, col, "#", dip_rect(SIDEBAR_WIDTH_DIP - 24.0, y0, SIDEBAR_WIDTH_DIP - 12.0, y1, scale), DT_RIGHT);
        }
        SIDEBAR_WIDTH_DIP - 24.0
    } else {
        SIDEBAR_WIDTH_DIP - 13.0
    };
    let (fnt, col) = if is_recessed {
        (fonts.chip, muted)
    } else if is_active {
        (fonts.chip_active, pal.chip_active_text)
    } else if is_hover {
        (fonts.chip, pal.chip_hover_text)
    } else {
        (fonts.chip, pal.chip_text)
    };
    unsafe { draw_text_gdi(dc, fnt, col, &item.label, dip_rect(x_left, y0, label_right, y1, scale), DT_LEFT); }
}

pub(super) fn paint_sidebar_gdi(dc: HDC, fonts: &FontPack, lay: &FrameLayout, state: &RenderState<'_>, scale: f32) {
    let pal = state.theme;
    let sl = compute_sidebar_layout(lay.header_h, lay.fy0, state.filter_tabs, state.sidebar_scroll);
    let drag = state.sidebar_drag;
    unsafe {
        for item in &sl.items {
            if item.y0 < sl.list_top - 1.0 || item.y1 > sl.list_bot + 1.0 { continue; }
            let is_rec = drag.is_some_and(|d| d.dragged == item.filter);
            let is_act = item.filter == state.active_filter;
            let is_hov = drag.is_none() && state.hovered_filter == Some(item.filter);
            let x_left = if is_hov && item.filter != SearchFilter::All { 19.0 } else { 16.0 };
            paint_tab_gdi_row(dc, fonts, pal, item, item.y0, item.y1, is_act, is_hov, is_rec, x_left, state.filter_counts, scale);
        }
        if let Some(d) = drag
            && let Some(item) = sl.items.iter().find(|it| it.filter == d.dragged)
        {
            let gy0 = d.ghost_y0.clamp(sl.list_top, (sl.list_bot - TAB_ROW_H).max(sl.list_top));
            paint_tab_gdi_row(dc, fonts, pal, item, gy0, gy0 + TAB_ROW_H, true, false, false, 20.0, state.filter_counts, scale);
        }
        let (ax0, ay0, ax1, ay1) = sl.add_btn;
        let add_lbl = match state.lang { Lang::Zh => "+ 添加标签", Lang::En => "+ Add Tag" };
        let add_col = if state.hovered_add_tag && drag.is_none() { pal.accent } else { pal.text_primary };
        let add_fnt = if state.hovered_add_tag && drag.is_none() { fonts.chip_active } else { fonts.chip };
        draw_text_gdi(dc, add_fnt, add_col, add_lbl, dip_rect(ax0, ay0, ax1, ay1, scale), DT_CENTER);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tabs(n: usize) -> Vec<VisibleFilterTab> {
        (0..n).map(|i| VisibleFilterTab {
            filter: if i == 0 { SearchFilter::All } else { SearchFilter::Custom(i as u8) },
            label: format!("Tag{i}"), extensions: vec!["rs".into()],
        }).collect()
    }

    #[test]
    fn hit_tests_tabs_add_button_and_scroll_bounds() {
        let tabs = sample_tabs(12);
        let sl = compute_sidebar_layout(62.0, 312.0, &tabs, 0.0);
        assert!(sl.max_scroll > 0.0);
        assert_eq!(hit_test_sidebar(50.0, sl.items[0].y0 + 10.0, 62.0, 312.0, &tabs, 0.0), Some(SidebarHit::Tab(SearchFilter::All)));
        let (_, ay0, _, ay1) = sl.add_btn;
        assert_eq!(hit_test_sidebar(50.0, (ay0 + ay1) * 0.5, 62.0, 312.0, &tabs, 0.0), Some(SidebarHit::AddTag));
        assert_eq!(hit_test_sidebar(150.0, 120.0, 62.0, 312.0, &tabs, 0.0), None);
        let hidden = sl.items.iter().find(|it| it.y1 > sl.list_bot + 1.0).unwrap();
        assert_eq!(hit_test_sidebar(50.0, sl.list_bot - 1.0, 62.0, 312.0, &tabs, 0.0).filter(|h| *h == SidebarHit::Tab(hidden.filter)), None);
    }

    #[test]
    fn computes_drag_drop_slot_never_displacing_pinned_all() {
        let tabs = sample_tabs(5);
        let sl = compute_sidebar_layout(62.0, 312.0, &tabs, 0.0);
        assert_eq!(compute_drag_drop_slot(sl.items[0].y0 + 5.0, &sl), 1);
        assert_eq!(compute_drag_drop_slot(sl.items[2].y1 - 2.0, &sl), 3);
        assert_eq!(compute_drag_drop_slot(sl.items[4].y1 + 20.0, &sl), 5);
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
