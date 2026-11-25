//! Pure-Rust SDF + ClearType GDI painter for Tomoyo Native Super Panel.

use super::panel_layout::{SuperHit, SuperPanelLayout, PAD_X, PAD_Y};
use super::{InlineTransState, SuperAction, SuperContext, TextKind};
use crate::renderer::{
    dip_rect, draw_multiline_gdi, draw_text_gdi, Canvas, DibSurface, FontPack, Rgb, ThemePalette,
};
use std::path::Path;
use tomoyo_core::Lang;
use windows::Win32::Graphics::Gdi::{BitBlt, GdiFlush, HDC, DT_CENTER, DT_LEFT, DT_RIGHT, SRCCOPY};

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
    selected_tile: Option<usize>,
    copied_flash: bool,
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

    let tile_bg = if pal.is_dark { Rgb(0x23, 0x26, 0x33) } else { Rgb(0xF6, 0xF8, 0xFC) };
    let card_bg = if pal.is_dark { Rgb(0x1D, 0x21, 0x2E) } else { Rgb(0xF1, 0xF5, 0xFD) };
    let ok_fg = if pal.is_dark { Rgb(0x34, 0xD3, 0x99) } else { Rgb(0x05, 0x96, 0x69) };

    {
        let mut canvas = Canvas { buf: surf.pixels_mut(), w: width, h: height, scale };
        canvas.clear(pal.pop_bg);
        canvas.fill_rounded_rect(PAD_X, PAD_Y + 6.0, PAD_X + 7.0, PAD_Y + 13.0, 3.5, pal.accent);

        if let Some(card) = layout.trans_card {
            canvas.fill_rounded_rect(card.x0, card.y0, card.x1, card.y1, 8.0, card_bg);
            canvas.stroke_rounded_rect(card.x0, card.y0, card.x1, card.y1, 8.0, pal.border);
            canvas.fill_rounded_rect(card.x0 + 1.0, card.y0 + 7.0, card.x0 + 4.0, card.y1 - 7.0, 1.5, pal.accent);
            if let Some(rc) = layout.copy_btn {
                let fill = if hovered == Some(SuperHit::CopyTranslation) { pal.accent } else { pal.chip_active_bg };
                canvas.fill_rounded_rect(rc.x0, rc.y0, rc.x1, rc.y1, 5.0, fill);
            }
            if let Some(rc) = layout.open_tr_btn {
                let fill = if hovered == Some(SuperHit::OpenTranslator) { pal.chip_hover_bg } else { pal.chip_bg };
                canvas.fill_rounded_rect(rc.x0, rc.y0, rc.x1, rc.y1, 5.0, fill);
                canvas.stroke_rounded_rect(rc.x0, rc.y0, rc.x1, rc.y1, 5.0, pal.chip_border);
            }
        }

        for (idx, (act, rc)) in actions.iter().zip(layout.tile_rects.iter()).enumerate() {
            let is_hov = hovered == Some(SuperHit::Tile(idx)) || selected_tile == Some(idx);
            canvas.fill_rounded_rect(rc.x0, rc.y0, rc.x1, rc.y1, 7.0, if is_hov { pal.chip_active_bg } else { tile_bg });
            canvas.stroke_rounded_rect(rc.x0, rc.y0, rc.x1, rc.y1, 7.0, if is_hov { pal.chip_active_border } else { pal.border });
            let (bx0, by0) = (rc.x0 + 7.0, rc.y0 + 7.0);
            canvas.fill_rounded_rect(bx0, by0, bx0 + 24.0, by0 + 24.0, 6.0, act.badge_rgb);
        }

        canvas.fill_rect_dip(PAD_X, layout.footer_y0, layout.w_dip - PAD_X, layout.footer_y0 + 1.0, pal.divider);
        canvas.fill_rect_dip(0.0, 0.0, layout.w_dip, 1.0, pal.border);
        canvas.fill_rect_dip(0.0, layout.h_dip - 1.0, layout.w_dip, layout.h_dip, pal.border);
        canvas.fill_rect_dip(0.0, 0.0, 1.0, layout.h_dip, pal.border);
        canvas.fill_rect_dip(layout.w_dip - 1.0, 0.0, layout.w_dip, layout.h_dip, pal.border);
    }

    let dc = surf.mem_dc;
    unsafe {
        let _ = GdiFlush();
        let (title, preview) = format_header(ctx, lang);
        draw_text_gdi(
            dc, fonts.chip_active, pal.text_primary, &title,
            dip_rect(PAD_X + 13.0, PAD_Y, layout.w_dip - 72.0, PAD_Y + 20.0, scale), DT_LEFT,
        );
        draw_text_gdi(
            dc, fonts.footer, pal.text_muted, lang.pick("Esc 关闭", "Esc Close"),
            dip_rect(layout.w_dip - 72.0, PAD_Y, layout.w_dip - PAD_X, PAD_Y + 20.0, scale), DT_RIGHT,
        );
        if let Some(prev) = preview {
            draw_text_gdi(
                dc, fonts.footer, pal.text_secondary, &prev,
                dip_rect(PAD_X + 13.0, PAD_Y + 20.0, layout.w_dip - PAD_X, PAD_Y + 38.0, scale), DT_LEFT,
            );
        }

        if let Some(card) = layout.trans_card {
            paint_trans_text(dc, fonts, pal, ok_fg, lang, scale, trans, layout, card, hovered, copied_flash);
        }

        for (idx, (act, rc)) in actions.iter().zip(layout.tile_rects.iter()).enumerate() {
            let is_hov = hovered == Some(SuperHit::Tile(idx)) || selected_tile == Some(idx);
            draw_text_gdi(
                dc, fonts.footer, Rgb(0xFF, 0xFF, 0xFF), &act.badge,
                dip_rect(rc.x0 + 7.0, rc.y0 + 7.0, rc.x0 + 31.0, rc.y0 + 31.0, scale), DT_CENTER,
            );
            let right_lim = if idx < 9 { rc.x1 - 22.0 } else { rc.x1 - 8.0 };
            draw_text_gdi(
                dc, if is_hov { fonts.chip_active } else { fonts.chip },
                if is_hov { pal.chip_active_text } else { pal.text_primary },
                &act.label, dip_rect(rc.x0 + 37.0, rc.y0, right_lim, rc.y1, scale), DT_LEFT,
            );
            if idx < 9 {
                let num = (idx + 1).to_string();
                draw_text_gdi(
                    dc, fonts.footer, pal.text_muted, &num,
                    dip_rect(rc.x1 - 20.0, rc.y0, rc.x1 - 6.0, rc.y1, scale), DT_RIGHT,
                );
            }
        }

        let foot = match ctx {
            SuperContext::Text { .. } => lang.pick("1–9 快选 · Enter 执行 · Ctrl+C 复制译文", "1-9 Pick · Enter Run · Ctrl+C Copy"),
            _ => lang.pick("1–9 快选 · ↑↓←→ 选择 · 点击外部关闭", "1-9 Pick · Arrows Move · Click outside to close"),
        };
        draw_text_gdi(
            dc, fonts.footer, pal.text_muted, foot,
            dip_rect(PAD_X, layout.footer_y0 + 2.0, layout.w_dip - PAD_X, layout.h_dip - 2.0, scale), DT_LEFT,
        );
        let _ = BitBlt(screen_hdc, 0, 0, width, height, Some(dc), 0, 0, SRCCOPY);
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn paint_trans_text(
    dc: HDC,
    fonts: &FontPack,
    pal: ThemePalette,
    ok_fg: Rgb,
    lang: Lang,
    scale: f32,
    trans: &InlineTransState,
    layout: &SuperPanelLayout,
    card: super::panel_layout::DipRect,
    hovered: Option<SuperHit>,
    copied_flash: bool,
) {
    unsafe {
        match trans {
            InlineTransState::Loading => {
                draw_text_gdi(
                    dc, fonts.chip_active, pal.accent,
                    lang.pick("⏳ 正在并行翻译…", "⏳ Translating in parallel…"),
                    dip_rect(card.x0 + 11.0, card.y0 + 7.0, card.x1 - 10.0, card.y0 + 26.0, scale), DT_LEFT,
                );
                draw_text_gdi(
                    dc, fonts.footer, pal.text_muted,
                    lang.pick("直连多源翻译引擎（Google / 有道词典 / DeepL / AI）", "Using configured multi-engine translator"),
                    dip_rect(card.x0 + 11.0, card.y0 + 27.0, card.x1 - 10.0, card.y1 - 6.0, scale), DT_LEFT,
                );
            }
            InlineTransState::Error { message, .. } => {
                draw_text_gdi(
                    dc, fonts.chip_active, pal.text_secondary,
                    lang.pick("内联翻译暂不可用", "Inline translation unavailable"),
                    dip_rect(card.x0 + 11.0, card.y0 + 7.0, card.x1 - 10.0, card.y0 + 25.0, scale), DT_LEFT,
                );
                draw_text_gdi(
                    dc, fonts.footer, pal.text_muted, message,
                    dip_rect(card.x0 + 11.0, card.y0 + 25.0, card.x1 - 114.0, card.y1 - 8.0, scale), DT_LEFT,
                );
            }
            InlineTransState::Ready { translated, engine_name, phonetic, target_lang, .. } => {
                let meta = match phonetic.as_deref().filter(|p| !p.is_empty()) {
                    Some(ph) => format!("{engine_name} · {target_lang}  [{ph}]"),
                    None => format!("{engine_name} · {target_lang}"),
                };
                draw_text_gdi(
                    dc, fonts.footer, pal.accent, &meta,
                    dip_rect(card.x0 + 11.0, card.y0 + 5.0, card.x1 - 10.0, card.y0 + 22.0, scale), DT_LEFT,
                );
                let body_b = layout.copy_btn.map(|b| b.y0 - 4.0).unwrap_or(card.y1 - 8.0);
                draw_multiline_gdi(
                    dc, fonts.chip, pal.text_primary, translated,
                    dip_rect(card.x0 + 11.0, card.y0 + 23.0, card.x1 - 10.0, body_b, scale),
                );
            }
            InlineTransState::Hidden => {}
        }

        if let Some(rc) = layout.copy_btn {
            let hov = hovered == Some(SuperHit::CopyTranslation);
            let fg = if copied_flash { ok_fg } else if hov { Rgb(0xFF, 0xFF, 0xFF) } else { pal.chip_active_text };
            let lbl = if copied_flash { lang.pick("✓ 已复制", "✓ Copied") } else { lang.pick("复制译文", "Copy Result") };
            draw_text_gdi(dc, fonts.footer, fg, lbl, dip_rect(rc.x0, rc.y0, rc.x1, rc.y1, scale), DT_CENTER);
        }
        if let Some(rc) = layout.open_tr_btn {
            draw_text_gdi(
                dc, fonts.footer, pal.text_secondary, lang.pick("多源对照 ↗", "Full View ↗"),
                dip_rect(rc.x0, rc.y0, rc.x1, rc.y1, scale), DT_CENTER,
            );
        }
    }
}

fn format_header(ctx: &SuperContext, lang: Lang) -> (String, Option<String>) {
    match ctx {
        SuperContext::Empty => (lang.pick("Tomoyo 超级面板", "Tomoyo Super Panel").into(), None),
        SuperContext::Text { raw, kind } => {
            let chars = raw.chars().count();
            let title = match (lang, kind) {
                (Lang::Zh, TextKind::Url) => "网址链接 · 超级面板".into(),
                (Lang::Zh, TextKind::Json) => format!("JSON 数据 · {chars} 字"),
                (Lang::Zh, TextKind::Color) => "色值 · 超级面板".into(),
                (Lang::Zh, TextKind::Path) => "本地路径 · 超级面板".into(),
                (Lang::Zh, TextKind::Code) => format!("代码片段 · {chars} 字"),
                (Lang::Zh, TextKind::Plain) => format!("选中文本 · {chars} 字"),
                (Lang::En, TextKind::Url) => "URL · Super Panel".into(),
                (Lang::En, TextKind::Json) => format!("JSON · {chars} chars"),
                (Lang::En, TextKind::Color) => "Color · Super Panel".into(),
                (Lang::En, TextKind::Path) => "Local Path · Super Panel".into(),
                (Lang::En, TextKind::Code) => format!("Code Snippet · {chars} chars"),
                (Lang::En, TextKind::Plain) => format!("Selected Text · {chars} chars"),
            };
            let single: String = raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(46).collect();
            (title, Some(format!("“{single}”")))
        }
        SuperContext::Files { paths, all_images } => {
            let n = paths.len();
            let title = match (lang, *all_images) {
                (Lang::Zh, true) => format!("选中 {n} 张图片"),
                (Lang::Zh, false) => format!("选中 {n} 个文件"),
                (Lang::En, true) => format!("{n} Image(s) Selected"),
                (Lang::En, false) => format!("{n} File(s) Selected"),
            };
            let first_name = paths.first().and_then(|p| Path::new(p).file_name()).and_then(|s| s.to_str()).unwrap_or("");
            (title, Some(first_name.to_string()))
        }
    }
}
