//! SDF + ClearType GDI Filter Chips Bar with optional live category counts
//! (`全部 · 应用 · 文件夹 · PDF · Excel · Word · PPT · 图片 · 压缩`).

use super::canvas::Canvas;
use super::gdi::{dip_rect, draw_text_gdi, FontPack};
use super::theme::ThemePalette;
use std::path::Path;
use tomoyo_core::Lang;
use tomoyo_protocol::{ActionKind, ResultCategory, ResultItem, SearchFilter};
use windows::Win32::Graphics::Gdi::{GdiFlush, HDC, DT_CENTER};

pub const FILTER_BAR_HEIGHT_DIP: i32 = 38;

/// Live hit counts corresponding positionally to `SearchFilter::ALL` (9 chips).
pub type FilterCounts = [u16; 9];

#[derive(Debug, Clone)]
pub struct FilterChipHitbox {
    pub filter: SearchFilter,
    pub label: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// Computes category/file-type counts across a result list (ignoring zero-result fallback cards).
pub fn compute_filter_counts(items: &[ResultItem]) -> FilterCounts {
    let mut counts = [0u16; 9];
    for item in items {
        if item.provider_id == "builtin.fallback" {
            continue;
        }
        counts[0] = counts[0].saturating_add(1);
        match item.category {
            ResultCategory::App => counts[1] = counts[1].saturating_add(1),
            ResultCategory::Folder => counts[2] = counts[2].saturating_add(1),
            ResultCategory::File => {
                if let Some(ext) = item_extension(item) {
                    let slot = match ext.as_str() {
                        "pdf" => Some(3),
                        "xls" | "xlsx" | "xlsm" | "csv" => Some(4),
                        "doc" | "docx" | "rtf" => Some(5),
                        "ppt" | "pptx" => Some(6),
                        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "svg" | "ico" => Some(7),
                        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" => Some(8),
                        _ => None,
                    };
                    if let Some(i) = slot {
                        counts[i] = counts[i].saturating_add(1);
                    }
                }
            }
            _ => {}
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

fn chip_display_label(filter: SearchFilter, idx: usize, zh: bool, counts: Option<&FilterCounts>) -> String {
    let base = filter.label(zh);
    match counts.map(|c| c[idx]).filter(|&n| n > 0) {
        Some(n) if n >= 100 => format!("{base} 99+"),
        Some(n) => format!("{base} {n}"),
        None => base.to_string(),
    }
}

/// Computes deterministic horizontal chip layout inside `[14 .. list_w_dip - 12]` at `header_h`.
pub fn compute_filter_hitboxes(
    list_w_dip: f32,
    header_h: f32,
    lang: Lang,
    counts: Option<&FilterCounts>,
) -> Vec<FilterChipHitbox> {
    let zh = lang == Lang::Zh;
    let max_x = list_w_dip - 12.0;
    let y0 = header_h + 7.0;
    let y1 = header_h + FILTER_BAR_HEIGHT_DIP as f32 - 7.0;
    let mut x = 14.0f32;
    let mut out = Vec::with_capacity(SearchFilter::ALL.len());

    for (idx, &filter) in SearchFilter::ALL.iter().enumerate() {
        let label = chip_display_label(filter, idx, zh, counts);
        let text_w: f32 = label
            .chars()
            .map(|c| if c.is_ascii() { 7.2 } else { 12.8 })
            .sum();
        let pill_w = (text_w + 20.0).clamp(44.0, 96.0);
        if x + pill_w > max_x {
            break;
        }
        out.push(FilterChipHitbox {
            filter,
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
    lang: Lang,
    active: SearchFilter,
    hovered: Option<SearchFilter>,
    counts: Option<&FilterCounts>,
) {
    canvas.fill_rect_dip(12.0, header_h, list_w_dip - 12.0, header_h + 1.0, pal.divider);
    let boxes = compute_filter_hitboxes(list_w_dip, header_h, lang, counts);
    for hb in boxes {
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
    lang: Lang,
    active: SearchFilter,
    hovered: Option<SearchFilter>,
    counts: Option<&FilterCounts>,
) {
    let boxes = compute_filter_hitboxes(list_w_dip, header_h, lang, counts);
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
