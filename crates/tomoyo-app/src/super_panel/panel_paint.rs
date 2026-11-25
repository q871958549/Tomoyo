//! Minimalist pure-Rust SDF + ClearType GDI painter for Tomoyo Super Panel.

use super::panel_layout::{SuperHit, SuperPanelLayout, PAD_X};
use super::{InlineTransState, SuperAction, SuperContext};
use crate::renderer::{
    dip_rect, draw_multiline_gdi, draw_text_gdi, Canvas, DibSurface, FontPack, ThemePalette,
};
use std::path::Path;
use tomoyo_core::Lang;
use windows::Win32::Graphics::Gdi::{BitBlt, GdiFlush, HDC, DT_CENTER, DT_LEFT, SRCCOPY};

#[allow(clippy::too_many_arguments)]
pub fn paint_super_panel(
    screen_hdc: HDC,
    width: i32,
    height: i32,
    scale: f32,
    pal: ThemePalette,
    lang: Lang,
    fonts: &FontPack,
    surface: &mut Option<DibSurface>,
    ctx: &SuperContext,
    trans: &InlineTransState,
    actions: &[SuperAction],
    layout: &SuperPanelLayout,
    hovered: Option<SuperHit>,
    selected_row: Option<usize>,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    if surface.as_ref().map(|s| (s.width, s.height)) != Some((width, height)) {
        *surface = None;
        *surface = DibSurface::create(screen_hdc, width, height);
    }
    let Some(surf) = surface.as_mut() else {
        return;
    };

    {
        let mut canvas = Canvas { buf: surf.pixels_mut(), w: width, h: height, scale };
        canvas.clear(pal.pop_bg);

        if let Some(area) = layout.trans_area
            && hovered == Some(SuperHit::CopyTranslation)
        {
            canvas.fill_rounded_rect(area.x0, area.y0, area.x1, area.y1, 6.0, pal.card_hover);
        }
        if let Some(rc) = layout.copy_btn
            && hovered == Some(SuperHit::CopyTranslation)
        {
            canvas.fill_rounded_rect(rc.x0, rc.y0, rc.x1, rc.y1, 4.0, pal.chip_active_bg);
        }
        if let Some(rc) = layout.open_tr_btn
            && hovered == Some(SuperHit::OpenTranslator)
        {
            canvas.fill_rounded_rect(rc.x0, rc.y0, rc.x1, rc.y1, 4.0, pal.chip_active_bg);
        }

        if let Some(dy) = layout.divider_y {
            canvas.fill_rect_dip(10.0, dy, layout.w_dip - 10.0, dy + 1.0, pal.divider);
        }

        for (idx, rc) in layout.row_rects.iter().enumerate() {
            if hovered == Some(SuperHit::Row(idx)) || selected_row == Some(idx) {
                canvas.fill_rounded_rect(rc.x0, rc.y0 + 1.0, rc.x1, rc.y1 - 1.0, 6.0, pal.chip_active_bg);
            }
        }

        canvas.fill_rect_dip(0.0, 0.0, layout.w_dip, 1.0, pal.border);
        canvas.fill_rect_dip(0.0, layout.h_dip - 1.0, layout.w_dip, layout.h_dip, pal.border);
        canvas.fill_rect_dip(0.0, 0.0, 1.0, layout.h_dip, pal.border);
        canvas.fill_rect_dip(layout.w_dip - 1.0, 0.0, layout.w_dip, layout.h_dip, pal.border);
    }

    let dc = surf.mem_dc;
    unsafe {
        let _ = GdiFlush();

        if let Some(cy) = layout.caption_y {
            let cap = format_caption(ctx, lang);
            draw_text_gdi(
                dc, fonts.footer, pal.text_muted, &cap,
                dip_rect(PAD_X + 6.0, cy + 1.0, layout.w_dip - PAD_X - 6.0, cy + 20.0, scale), DT_LEFT,
            );
        }

        if let Some(area) = layout.trans_area {
            paint_trans_block(dc, fonts, pal, lang, scale, trans, layout, area, hovered);
        }

        for (idx, (act, rc)) in actions.iter().zip(layout.row_rects.iter()).enumerate() {
            let is_hov = hovered == Some(SuperHit::Row(idx)) || selected_row == Some(idx);
            let fg = if is_hov { pal.chip_active_text } else { pal.text_primary };
            let glyph_col = if is_hov { pal.accent } else { pal.text_muted };
            draw_text_gdi(
                dc, fonts.footer, glyph_col, &act.glyph,
                dip_rect(rc.x0 + 4.0, rc.y0, rc.x0 + 24.0, rc.y1, scale), DT_CENTER,
            );
            draw_text_gdi(
                dc, if is_hov { fonts.chip_active } else { fonts.chip }, fg, &act.label,
                dip_rect(rc.x0 + 26.0, rc.y0, rc.x1 - 8.0, rc.y1, scale), DT_LEFT,
            );
        }

        let _ = BitBlt(screen_hdc, 0, 0, width, height, Some(dc), 0, 0, SRCCOPY);
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn paint_trans_block(
    dc: HDC,
    fonts: &FontPack,
    pal: ThemePalette,
    lang: Lang,
    scale: f32,
    trans: &InlineTransState,
    layout: &SuperPanelLayout,
    area: super::panel_layout::DipRect,
    hovered: Option<SuperHit>,
) {
    unsafe {
        match trans {
            InlineTransState::Loading => {
                draw_text_gdi(
                    dc, fonts.footer, pal.accent, lang.pick("翻译中…", "Translating…"),
                    dip_rect(area.x0 + 6.0, area.y0 + 10.0, area.x1 - 6.0, area.y1 - 6.0, scale), DT_LEFT,
                );
            }
            InlineTransState::Error { message } => {
                draw_text_gdi(
                    dc, fonts.footer, pal.text_muted, lang.pick("翻译暂不可用", "Translation unavailable"),
                    dip_rect(area.x0 + 6.0, area.y0 + 2.0, area.x1 - 64.0, area.y0 + 19.0, scale), DT_LEFT,
                );
                draw_text_gdi(
                    dc, fonts.footer, pal.text_secondary, message,
                    dip_rect(area.x0 + 6.0, area.y0 + 20.0, area.x1 - 6.0, area.y1 - 2.0, scale), DT_LEFT,
                );
            }
            InlineTransState::Ready { translated, engine_name, phonetic, .. } => {
                let meta = match phonetic.as_deref().filter(|p| !p.is_empty()) {
                    Some(ph) => format!("{engine_name} · [{ph}]"),
                    None => engine_name.clone(),
                };
                let meta_r = layout.copy_btn.map(|b| b.x0 - 4.0).unwrap_or(area.x1 - 6.0);
                draw_text_gdi(
                    dc, fonts.footer, pal.text_muted, &meta,
                    dip_rect(area.x0 + 6.0, area.y0 + 1.0, meta_r, area.y0 + 19.0, scale), DT_LEFT,
                );
                draw_multiline_gdi(
                    dc, fonts.chip, pal.text_primary, translated,
                    dip_rect(area.x0 + 6.0, area.y0 + 21.0, area.x1 - 6.0, area.y1 - 3.0, scale),
                );
            }
            InlineTransState::Hidden => {}
        }

        if let Some(rc) = layout.copy_btn {
            let fg = if hovered == Some(SuperHit::CopyTranslation) { pal.accent } else { pal.text_muted };
            draw_text_gdi(dc, fonts.footer, fg, lang.pick("复制", "Copy"), dip_rect(rc.x0, rc.y0, rc.x1, rc.y1, scale), DT_CENTER);
        }
        if let Some(rc) = layout.open_tr_btn {
            let fg = if hovered == Some(SuperHit::OpenTranslator) { pal.accent } else { pal.text_muted };
            draw_text_gdi(dc, fonts.footer, fg, lang.pick("对照 ↗", "More ↗"), dip_rect(rc.x0, rc.y0, rc.x1, rc.y1, scale), DT_CENTER);
        }
    }
}

fn format_caption(ctx: &SuperContext, lang: Lang) -> String {
    match ctx {
        SuperContext::Empty => String::new(),
        SuperContext::Text { raw, .. } => {
            raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(24).collect()
        }
        SuperContext::Files { paths, .. } => {
            if paths.len() == 1 {
                paths[0]
                    .as_str()
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or(&paths[0])
                    .to_string()
            } else {
                let first = Path::new(&paths[0]).file_name().and_then(|s| s.to_str()).unwrap_or("");
                match lang {
                    Lang::Zh => format!("{first} 等 {} 项", paths.len()),
                    Lang::En => format!("{first} +{}", paths.len() - 1),
                }
            }
        }
    }
}
