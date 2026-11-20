//! SDF + ClearType GDI Filter Chips Bar (`全部 · 应用 · 文件夹 · PDF · Excel · Word · PPT · 图片 · 压缩`).

use super::canvas::Canvas;
use super::gdi::{dip_rect, draw_text_gdi, FontPack};
use super::theme::ThemePalette;
use tomoyo_core::Lang;
use tomoyo_protocol::SearchFilter;
use windows::Win32::Graphics::Gdi::{GdiFlush, HDC, DT_CENTER};

pub const FILTER_BAR_HEIGHT_DIP: i32 = 34;

#[derive(Debug, Clone, Copy)]
pub struct FilterChipHitbox {
    pub filter: SearchFilter,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// Computes deterministic horizontal chip layout inside `[12 .. list_w_dip - 12]` at `header_h`.
pub fn compute_filter_hitboxes(
    list_w_dip: f32,
    header_h: f32,
    lang: Lang,
) -> Vec<FilterChipHitbox> {
    let zh = lang == Lang::Zh;
    let max_x = list_w_dip - 12.0;
    let y0 = header_h + 5.0;
    let y1 = header_h + FILTER_BAR_HEIGHT_DIP as f32 - 5.0;
    let mut x = 14.0f32;
    let mut out = Vec::with_capacity(SearchFilter::ALL.len());

    for &filter in &SearchFilter::ALL {
        let label = filter.label(zh);
        let text_w: f32 = label
            .chars()
            .map(|c| if c.is_ascii() { 7.4 } else { 12.5 })
            .sum();
        let pill_w = (text_w + 18.0).clamp(38.0, 74.0);
        if x + pill_w > max_x {
            break;
        }
        out.push(FilterChipHitbox {
            filter,
            x0: x,
            y0,
            x1: x + pill_w,
            y1,
        });
        x += pill_w + 6.0;
    }
    out
}

pub fn paint_filter_bar_sdf(
    canvas: &mut Canvas<'_>,
    pal: ThemePalette,
    list_w_dip: f32,
    header_h: f32,
    lang: Lang,
    active: SearchFilter,
    hovered: Option<SearchFilter>,
) {
    canvas.fill_rect_dip(12.0, header_h, list_w_dip - 12.0, header_h + 1.0, pal.divider);
    let boxes = compute_filter_hitboxes(list_w_dip, header_h, lang);
    for hb in boxes {
        let is_active = hb.filter == active;
        let is_hover = hovered == Some(hb.filter);
        let bg = if is_active {
            pal.accent
        } else if is_hover {
            pal.card_sel
        } else {
            pal.pill_bg
        };
        canvas.fill_rounded_rect(hb.x0, hb.y0, hb.x1, hb.y1, 6.0, bg);
    }
}

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
) {
    let zh = lang == Lang::Zh;
    let boxes = compute_filter_hitboxes(list_w_dip, header_h, lang);
    unsafe {
        let _ = GdiFlush();
        for hb in boxes {
            let is_active = hb.filter == active;
            let col = if is_active {
                pal.on_accent
            } else if hovered == Some(hb.filter) {
                pal.text_primary
            } else {
                pal.text_muted
            };
            draw_text_gdi(
                dc,
                fonts.badge,
                col,
                hb.filter.label(zh),
                dip_rect(hb.x0, hb.y0, hb.x1, hb.y1, scale),
                DT_CENTER,
            );
        }
    }
}
