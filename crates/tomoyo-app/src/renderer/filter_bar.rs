//! SDF + ClearType GDI User-Extensible Filter Chips Bar with live category/extension counts.

use super::canvas::Canvas;
use super::gdi::{dip_rect, draw_text_gdi, FontPack};
use super::theme::ThemePalette;
use std::path::Path;
use tomoyo_protocol::{ActionKind, ResultCategory, ResultItem, SearchFilter, VisibleFilterTab};
use windows::Win32::Graphics::Gdi::{GdiFlush, HDC, DT_CENTER};

pub const FILTER_BAR_HEIGHT_DIP: i32 = 38;

/// Live hit counts matching positionally with the active `VisibleFilterTab` slice.
pub type FilterCounts = Vec<u16>;

#[derive(Debug, Clone)]
pub struct FilterChipHitbox {
    pub filter: SearchFilter,
    pub label: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// Computes category/file-type counts across a result list for all visible built-in & custom tabs.
pub fn compute_filter_counts(items: &[ResultItem], tabs: &[VisibleFilterTab]) -> FilterCounts {
    let mut counts = vec![0u16; tabs.len()];
    for item in items {
        if item.provider_id == "builtin.fallback" {
            continue;
        }
        let ext = if item.category == ResultCategory::File {
            item_extension(item)
        } else {
            None
        };
        for (idx, tab) in tabs.iter().enumerate() {
            let matched = match tab.filter {
                SearchFilter::All => true,
                SearchFilter::App => item.category == ResultCategory::App,
                SearchFilter::Folder => item.category == ResultCategory::Folder,
                _ => ext
                    .as_deref()
                    .is_some_and(|e| tab.extensions.iter().any(|te| te.eq_ignore_ascii_case(e))),
            };
            if matched {
                counts[idx] = counts[idx].saturating_add(1);
            }
        }
    }
    counts
}

fn item_extension(item: &ResultItem) -> Option<String> {
    let path = item.actions.first().and_then(|a| match &a.kind {
        ActionKind::LaunchPath { path, .. } => Some(path.as_str()),
        _ => None,
    });
    let candidate = path.unwrap_or(item.title.as_str());
    Path::new(candidate)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

fn chip_display_label(tab: &VisibleFilterTab, idx: usize, counts: Option<&FilterCounts>) -> String {
    match counts.and_then(|c| c.get(idx).copied()).filter(|&n| n > 0) {
        Some(n) if n >= 100 => format!("{} 99+", tab.label),
        Some(n) => format!("{} {n}", tab.label),
        None => tab.label.clone(),
    }
}

/// Computes deterministic horizontal chip layout inside `[14 .. list_w_dip - 12]` at `header_h`.
pub fn compute_filter_hitboxes(
    list_w_dip: f32,
    header_h: f32,
    tabs: &[VisibleFilterTab],
    counts: Option<&FilterCounts>,
) -> Vec<FilterChipHitbox> {
    let max_x = list_w_dip - 12.0;
    let y0 = header_h + 7.0;
    let y1 = header_h + FILTER_BAR_HEIGHT_DIP as f32 - 7.0;
    let mut x = 14.0f32;
    let mut out = Vec::with_capacity(tabs.len());

    for (idx, tab) in tabs.iter().enumerate() {
        let label = chip_display_label(tab, idx, counts);
        let text_w: f32 = label
            .chars()
            .map(|c| if c.is_ascii() { 7.2 } else { 12.8 })
            .sum();
        let pill_w = (text_w + 20.0).clamp(44.0, 108.0);
        if x + pill_w > max_x {
            break;
        }
        out.push(FilterChipHitbox {
            filter: tab.filter,
            label,
            x0: x,
            y0,
            x1: x + pill_w,
            y1,
        });
        x += pill_w + 6.0;
    }
    out
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
    counts: Option<&FilterCounts>,
) {
    canvas.fill_rect_dip(12.0, header_h, list_w_dip - 12.0, header_h + 1.0, pal.divider);
    for hb in compute_filter_hitboxes(list_w_dip, header_h, tabs, counts) {
        let is_active = hb.filter == active;
        let is_hover = hovered == Some(hb.filter);
        let (bg, border) = if is_active {
            (pal.chip_active_bg, pal.chip_active_border)
        } else if is_hover {
            (pal.chip_hover_bg, pal.chip_hover_border)
        } else {
            (pal.chip_bg, pal.chip_border)
        };
        canvas.fill_rounded_rect(hb.x0, hb.y0, hb.x1, hb.y1, 7.0, bg);
        canvas.stroke_rounded_rect(hb.x0, hb.y0, hb.x1, hb.y1, 7.0, border);
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn paint_filter_bar_gdi(
    dc: HDC,
    fonts: &FontPack,
    pal: ThemePalette,
    list_w_dip: f32,
    header_h: f32,
    scale: f32,
    tabs: &[VisibleFilterTab],
    active: SearchFilter,
    hovered: Option<SearchFilter>,
    counts: Option<&FilterCounts>,
) {
    let boxes = compute_filter_hitboxes(list_w_dip, header_h, tabs, counts);
    unsafe {
        let _ = GdiFlush();
        for hb in boxes {
            let is_active = hb.filter == active;
            let (font, col) = if is_active {
                (fonts.chip_active, pal.chip_active_text)
            } else if hovered == Some(hb.filter) {
                (fonts.chip, pal.chip_hover_text)
            } else {
                (fonts.chip, pal.chip_text)
            };
            draw_text_gdi(
                dc,
                font,
                col,
                &hb.label,
                dip_rect(hb.x0, hb.y0, hb.x1, hb.y1, scale),
                DT_CENTER,
            );
        }
    }
}
