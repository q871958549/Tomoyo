//! Pango text pass for the scrollable left filter sidebar rail and floating drag ghost.

use crate::linux::render_pango::{draw_pango, PangoFont};
use crate::renderer::filter_sidebar::{
    blend_rgb, compute_sidebar_layout, count_for_filter, SidebarItemRect, SIDEBAR_WIDTH_DIP,
    TAB_ROW_H,
};
use crate::renderer::{FilterCounts, FrameLayout, RenderState, ThemePalette};
use std::ffi::c_void;
use tomoyo_core::Lang;
use tomoyo_protocol::SearchFilter;

#[allow(clippy::too_many_arguments)]
fn paint_tab_pango_row(
    cr: *mut c_void,
    scale: f32,
    pal: ThemePalette,
    item: &SidebarItemRect,
    y0: f32,
    y1: f32,
    is_active: bool,
    is_hover: bool,
    is_recessed: bool,
    x_left: f32,
    counts: Option<&FilterCounts>,
    f_chip: PangoFont<'_>,
    f_chip_active: PangoFont<'_>,
    f_sub_num: PangoFont<'_>,
) {
    let n = count_for_filter(counts, item.filter);
    let muted = if is_recessed {
        blend_rgb(pal.text_muted, pal.preview_bg, 0.5)
    } else {
        pal.text_muted
    };
    let label_right = if n > 0 {
        let cnt = n.to_string();
        let col = if is_active && !is_recessed { pal.accent } else { muted };
        draw_pango(
            cr,
            f_sub_num,
            col,
            2,
            true,
            &cnt,
            SIDEBAR_WIDTH_DIP - 36.0,
            y0,
            23.0,
            y1 - y0,
            scale,
            &[],
            pal.accent,
        );
        SIDEBAR_WIDTH_DIP - 38.0
    } else if item.is_custom {
        let col = if is_active && !is_recessed { pal.accent } else { muted };
        draw_pango(
            cr,
            f_sub_num,
            col,
            2,
            true,
            "#",
            SIDEBAR_WIDTH_DIP - 24.0,
            y0,
            12.0,
            y1 - y0,
            scale,
            &[],
            pal.accent,
        );
        SIDEBAR_WIDTH_DIP - 24.0
    } else {
        SIDEBAR_WIDTH_DIP - 13.0
    };
    let (fnt, col) = if is_recessed {
        (f_chip, muted)
    } else if is_active {
        (f_chip_active, pal.chip_active_text)
    } else if is_hover {
        (f_chip, pal.chip_hover_text)
    } else {
        (f_chip, pal.chip_text)
    };
    draw_pango(
        cr,
        fnt,
        col,
        0,
        true,
        &item.label,
        x_left,
        y0,
        (label_right - x_left).max(16.0),
        y1 - y0,
        scale,
        &[],
        pal.accent,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_sidebar_pango(
    cr: *mut c_void,
    scale: f32,
    lay: &FrameLayout,
    state: &RenderState<'_>,
    f_chip: PangoFont<'_>,
    f_chip_active: PangoFont<'_>,
    f_sub_num: PangoFont<'_>,
) {
    let pal = state.theme;
    let sl = compute_sidebar_layout(lay.header_h, lay.fy0, state.filter_tabs, state.sidebar_scroll);
    let drag = state.sidebar_drag;
    for item in &sl.items {
        if item.y0 < sl.list_top - 1.0 || item.y1 > sl.list_bot + 1.0 {
            continue;
        }
        let is_rec = drag.is_some_and(|d| d.dragged == item.filter);
        let is_act = item.filter == state.active_filter;
        let is_hov = drag.is_none() && state.hovered_filter == Some(item.filter);
        let x_left = if is_hov && item.filter != SearchFilter::All { 19.0 } else { 16.0 };
        paint_tab_pango_row(
            cr,
            scale,
            pal,
            item,
            item.y0,
            item.y1,
            is_act,
            is_hov,
            is_rec,
            x_left,
            state.filter_counts,
            f_chip,
            f_chip_active,
            f_sub_num,
        );
    }
    if let Some(d) = drag
        && let Some(item) = sl.items.iter().find(|it| it.filter == d.dragged)
    {
        let gy0 = d.ghost_y0.clamp(sl.list_top, (sl.list_bot - TAB_ROW_H).max(sl.list_top));
        paint_tab_pango_row(
            cr,
            scale,
            pal,
            item,
            gy0,
            gy0 + TAB_ROW_H,
            true,
            false,
            false,
            20.0,
            state.filter_counts,
            f_chip,
            f_chip_active,
            f_sub_num,
        );
    }
    let (ax0, ay0, ax1, ay1) = sl.add_btn;
    let add_lbl = match state.lang {
        Lang::Zh => "+ 添加标签",
        Lang::En => "+ Add Tag",
    };
    let (add_fnt, add_col) = if state.hovered_add_tag && drag.is_none() {
        (f_chip_active, pal.accent)
    } else {
        (f_chip, pal.text_primary)
    };
    draw_pango(cr, add_fnt, add_col, 1, true, add_lbl, ax0, ay0, ax1 - ax0, ay1 - ay0, scale, &[], pal.accent);
}
