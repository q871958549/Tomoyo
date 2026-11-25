//! Pure-Rust SDF anti-aliased shape, surface, card, scrollbar, and overlay pass.

use super::canvas::Canvas;
use super::filter_bar::paint_filter_bar_sdf;
use super::scrollbar::{compute_scrollbar_metrics, ScrollbarVisual};
use super::theme::{category_rgb, Rgb};
use super::{FrameLayout, RenderState};
use crate::brand::paint_modern_white_emblem;
#[cfg(windows)]
use crate::icons::IconCache;
#[cfg(windows)]
use crate::image_icons::ImageIconCache;
use tomoyo_protocol::IconSpec;

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_sdf_pass(
    pixels: &mut [u8],
    shell_icons: &mut IconCache,
    image_icons: &mut ImageIconCache,
    width: i32,
    height: i32,
    scale: f32,
    lay: &FrameLayout,
    state: &RenderState<'_>,
) {
    let icon_px = (32.0 * scale).round() as u32;
    paint_sdf_pass_inner(
        pixels,
        Some((shell_icons, image_icons, icon_px)),
        width,
        height,
        scale,
        lay,
        state,
    );
}

#[cfg(not(windows))]
pub(crate) fn paint_sdf_pass(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    scale: f32,
    lay: &FrameLayout,
    state: &RenderState<'_>,
) {
    paint_sdf_pass_inner(pixels, None, width, height, scale, lay, state);
}

fn paint_sdf_pass_inner(
    pixels: &mut [u8],
    #[cfg(windows)] mut icon_caches: Option<(&mut IconCache, &mut ImageIconCache, u32)>,
    #[cfg(not(windows))] _icon_caches: Option<()>,
    width: i32,
    height: i32,
    scale: f32,
    lay: &FrameLayout,
    state: &RenderState<'_>,
) {
    let pal = state.theme;
    let mut canvas = Canvas {
        buf: pixels,
        w: width,
        h: height,
        scale,
    };

    canvas.clear(pal.bg);
    if lay.has_preview {
        canvas.fill_rect_dip(lay.list_w_dip, lay.header_h, lay.w_dip, lay.fy0, pal.preview_bg);
        canvas.fill_rect_dip(lay.list_w_dip, lay.header_h, lay.list_w_dip + 1.0, lay.fy0, pal.divider);
    }
    canvas.fill_rect_dip(0.0, lay.fy0, lay.w_dip, lay.h_dip, pal.footer_bg);
    canvas.fill_rect_dip(0.0, lay.fy0, lay.w_dip, lay.fy0 + 1.0, pal.divider);

    if state.hovered_logo {
        canvas.fill_rounded_rect(10.0, 9.0, 54.0, 53.0, 10.0, pal.card_sel);
        canvas.stroke_rounded_rect(10.0, 9.0, 54.0, 53.0, 10.0, pal.sel_border);
    }
    paint_modern_white_emblem(
        canvas.buf,
        width,
        height,
        (14.0 * scale).round() as i32,
        (13.0 * scale).round() as i32,
        (36.0 * scale).round() as i32,
    );

    let mut header_right_x = lay.w_dip - 16.0;
    if let Some(mode) = state.mode_badge {
        let pill_w = (mode.chars().count() as f32 * 9.5 + 22.0).clamp(58.0, 130.0);
        let px0 = header_right_x - pill_w;
        canvas.fill_rounded_rect(px0, 19.0, header_right_x, 43.0, 7.0, pal.chip_active_bg);
        canvas.stroke_rounded_rect(px0, 19.0, header_right_x, 43.0, 7.0, pal.chip_active_border);
        header_right_x = px0 - 8.0;
    }
    if let Some(cap) = state.attached_capsule {
        let cap_w = (cap.chars().count() as f32 * 8.5 + 24.0).clamp(96.0, 210.0);
        let cx0 = header_right_x - cap_w;
        canvas.fill_rounded_rect(cx0, 18.0, header_right_x, 44.0, 8.0, pal.card_sel);
        canvas.stroke_rounded_rect(cx0, 18.0, header_right_x, 44.0, 8.0, pal.sel_border);
    }

    if state.filter_bar_visible {
        paint_filter_bar_sdf(
            &mut canvas,
            pal,
            lay.list_w_dip,
            lay.header_h,
            state.filter_tabs,
            state.active_filter,
            state.hovered_filter,
            state.hovered_overflow,
            state.filter_counts,
            state.filter_bar_wrap,
            state.filter_pill_override,
        );
    } else if !state.items.is_empty() {
        canvas.fill_rect_dip(12.0, lay.header_h, lay.w_dip - 12.0, lay.header_h + 1.0, pal.divider);
    }

    if lay.has_preview
        && let Some(prev) = state.preview
    {
        let (px0, px1) = (lay.list_w_dip + 14.0, lay.w_dip - 14.0);
        if !prev.badges.is_empty() {
            let mut bx = px0;
            let by = lay.header_h + 52.0;
            for b in prev.badges.iter().take(5) {
                let bw = (b.chars().count() as f32 * 6.5 + 14.0).clamp(36.0, 110.0);
                if bx + bw > px1 {
                    break;
                }
                canvas.fill_rounded_rect(bx, by, bx + bw, by + 18.0, 9.0, pal.pill_bg);
                canvas.stroke_rounded_rect(bx, by, bx + bw, by + 18.0, 9.0, pal.border);
                bx += bw + 6.0;
            }
        }
        if let Some((iw, ih, rgba)) = &prev.image_rgba {
            let avail_w = (px1 - px0).max(40.0);
            let avail_h = (lay.fy0 - (lay.header_h + 60.0) - 52.0).clamp(60.0, 210.0);
            let img_scale = (avail_w / (*iw as f32)).min(avail_h / (*ih as f32)).min(1.0);
            let (dw, dh) = ((*iw as f32 * img_scale).max(16.0), (*ih as f32 * img_scale).max(16.0));
            let (dx, dy) = (px0 + (avail_w - dw) * 0.5, lay.header_h + 56.0);
            canvas.fill_rounded_rect(dx - 3.0, dy - 3.0, dx + dw + 3.0, dy + dh + 3.0, 6.0, pal.pill_bg);
            canvas.draw_rgba_image(dx, dy, dw, dh, *iw, *ih, rgba);
            canvas.stroke_rounded_rect(dx - 3.0, dy - 3.0, dx + dw + 3.0, dy + dh + 3.0, 6.0, pal.border);
        }
    }

    for slot in 0..lay.visible_count {
        let idx = state.scroll_offset + slot;
        let Some(item) = state.items.get(idx) else { break };
        let y0 = lay.list_top + (slot as f32) * lay.row_h;
        let y1 = y0 + lay.row_h - 2.0;
        let is_sel = idx == state.selected;
        let icon_top = y0 + (lay.row_h - 32.0) * 0.5;
        let badge_top = y0 + (lay.row_h - 21.0) * 0.5;
        let key_top = y0 + (lay.row_h - 20.0) * 0.5;
        if is_sel {
            canvas.fill_rounded_rect(10.0, y0 + 2.0, lay.list_w_dip - 13.0, y1 - 2.0, 8.0, pal.card_sel);
            canvas.stroke_rounded_rect(10.0, y0 + 2.0, lay.list_w_dip - 13.0, y1 - 2.0, 8.0, pal.sel_border);
            canvas.fill_rounded_rect(10.5, y0 + 11.0, 13.5, y1 - 11.0, 1.5, pal.accent);
        } else if state.hovered == Some(idx) {
            canvas.fill_rounded_rect(10.0, y0 + 2.0, lay.list_w_dip - 13.0, y1 - 2.0, 8.0, pal.card_hover);
        }
        #[cfg(windows)]
        let mut drew_rgba = false;
        #[cfg(windows)]
        if let Some((ref mut sh_ic, ref mut img_ic, sz)) = icon_caches {
            match &item.icon {
                IconSpec::ImageFile(path) => {
                    if let Some((iw, ih, rgba)) = img_ic.get_cached(path, sz) {
                        canvas.draw_rgba_image(21.0, icon_top, 32.0, 32.0, *iw, *ih, rgba);
                        drew_rgba = true;
                    }
                }
                IconSpec::ShellPath(path) => {
                    if let Some((iw, ih, rgba)) = sh_ic.get_cached(path, sz) {
                        canvas.draw_rgba_image(21.0, icon_top, 32.0, 32.0, *iw, *ih, rgba);
                        drew_rgba = true;
                    }
                }
                _ => {}
            }
        }
        #[cfg(windows)]
        let need_tile = !drew_rgba;
        #[cfg(not(windows))]
        let need_tile = !matches!(item.icon, IconSpec::ShellPath(_) | IconSpec::ImageFile(_));
        if need_tile {
            canvas.fill_rounded_rect(21.0, icon_top, 53.0, icon_top + 32.0, 7.0, category_rgb(item.category));
        }
        let mut text_right = lay.list_w_dip - 20.0;
        let row_launch = state.launching_row.filter(|(li, _)| *li == idx);
        if let Some((_, done)) = row_launch {
            let pill_col = if done { Rgb(0x10, 0xB9, 0x81) } else { pal.accent };
            let pw = 74.0;
            canvas.fill_rounded_rect(text_right - pw, badge_top - 1.0, text_right, badge_top + 22.0, 6.0, pill_col);
            text_right -= pw + 8.0;
        } else if is_sel {
            canvas.fill_rounded_rect(text_right - 44.0, key_top, text_right, key_top + 20.0, 5.0, pal.chip_active_bg);
            canvas.stroke_rounded_rect(text_right - 44.0, key_top, text_right, key_top + 20.0, 5.0, pal.chip_active_border);
            text_right -= 50.0;
        } else if slot < 8 {
            canvas.fill_rounded_rect(text_right - 42.0, key_top, text_right, key_top + 20.0, 5.0, pal.pill_bg);
            canvas.stroke_rounded_rect(text_right - 42.0, key_top, text_right, key_top + 20.0, 5.0, pal.divider);
            text_right -= 48.0;
        }
        if let Some(badge) = &item.badge {
            let bw = (badge.chars().count() as f32 * 6.8 + 18.0).clamp(44.0, 120.0);
            canvas.fill_rounded_rect(text_right - bw, badge_top, text_right, badge_top + 21.0, 6.0, pal.chip_bg);
            canvas.stroke_rounded_rect(text_right - bw, badge_top, text_right, badge_top + 21.0, 6.0, pal.chip_border);
        }
    }

    if let Some(sb) = compute_scrollbar_metrics(
        lay.list_w_dip,
        lay.list_top,
        lay.fy0,
        state.items.len(),
        lay.visible_cap,
        state.scroll_offset,
        lay.has_preview,
    ) {
        let active = !matches!(state.scrollbar_visual, ScrollbarVisual::Idle);
        if active {
            let track_col = if pal.is_dark { Rgb(0x23, 0x27, 0x35) } else { Rgb(0xEA, 0xEE, 0xF5) };
            canvas.fill_rounded_rect(sb.track_x0 - 0.5, sb.track_y0, sb.track_x1 + 0.5, sb.track_y1, 4.0, track_col);
        }
        let (x0, x1, radius, thumb_col) = match state.scrollbar_visual {
            ScrollbarVisual::Dragging => (sb.track_x0, sb.track_x1, 3.75, pal.accent),
            ScrollbarVisual::HoveredThumb => {
                let col = if pal.is_dark { Rgb(0x94, 0xA3, 0xB8) } else { Rgb(0x64, 0x74, 0x8B) };
                (sb.track_x0 + 0.3, sb.track_x1 - 0.3, 3.4, col)
            }
            ScrollbarVisual::HoveredTrack => {
                let col = if pal.is_dark { Rgb(0x64, 0x74, 0x8B) } else { Rgb(0x94, 0xA3, 0xB8) };
                (sb.track_x0 + 0.6, sb.track_x1 - 0.6, 3.1, col)
            }
            ScrollbarVisual::Idle => (sb.track_x1 - 4.0, sb.track_x1, 2.0, pal.scroll_thumb),
        };
        canvas.fill_rounded_rect(x0, sb.thumb_y0, x1, sb.thumb_y1, radius, thumb_col);
    }

    let dot_col = if let Some(t) = state.toast {
        if t.starts_with('✗') || t.starts_with('⚠') {
            Rgb(0xEF, 0x44, 0x44)
        } else if t.starts_with('✓') {
            Rgb(0x10, 0xB9, 0x81)
        } else if t.starts_with('⏳') || state.launching_row.is_some() {
            pal.accent
        } else if state.everything_status.is_some() {
            Rgb(0x10, 0xB9, 0x81)
        } else {
            Rgb(0x6B, 0x72, 0x80)
        }
    } else if state.everything_status.is_some() {
        Rgb(0x10, 0xB9, 0x81)
    } else {
        Rgb(0x6B, 0x72, 0x80)
    };
    canvas.fill_rounded_rect(14.0, lay.fy0 + 11.5, 21.0, lay.fy0 + 18.5, 3.5, dot_col);

    if let Some(ver) = state.update_badge {
        let uw = (ver.chars().count() as f32 * 7.2 + 18.0).clamp(64.0, 120.0);
        let ux0 = lay.w_dip - 190.0 - uw;
        if ux0 > 220.0 {
            canvas.fill_rounded_rect(ux0, lay.fy0 + 5.0, lay.w_dip - 190.0, lay.fy0 + 25.0, 6.0, pal.chip_active_bg);
            canvas.stroke_rounded_rect(ux0, lay.fy0 + 5.0, lay.w_dip - 190.0, lay.fy0 + 25.0, 6.0, pal.chip_active_border);
        }
    }

    if state.action_menu_open
        && let Some(sel) = state.items.get(state.selected)
            && !sel.actions.is_empty() {
                let n = sel.actions.len().min(6);
                let (mx1, my1) = (lay.list_w_dip - 14.0, lay.fy0 - 10.0);
                let (mx0, my0) = (mx1 - 240.0, (my1 - (n as f32 * 30.0 + 16.0)).max(lay.list_top + 4.0));
                canvas.fill_rounded_rect(mx0, my0, mx1, my1, 8.0, pal.pop_bg);
                canvas.stroke_rounded_rect(mx0, my0, mx1, my1, 8.0, pal.border);
                if state.selected_action < n {
                    let ay0 = my0 + 8.0 + (state.selected_action as f32) * 30.0;
                    canvas.fill_rounded_rect(mx0 + 6.0, ay0, mx1 - 6.0, ay0 + 28.0, 6.0, pal.chip_active_bg);
                    canvas.stroke_rounded_rect(mx0 + 6.0, ay0, mx1 - 6.0, ay0 + 28.0, 6.0, pal.sel_border);
                }
            }

    canvas.fill_rect_dip(0.0, 0.0, lay.w_dip, 1.0, pal.border);
    canvas.fill_rect_dip(0.0, lay.h_dip - 1.0, lay.w_dip, lay.h_dip, pal.border);
    canvas.fill_rect_dip(0.0, 0.0, 1.0, lay.h_dip, pal.border);
    canvas.fill_rect_dip(lay.w_dip - 1.0, 0.0, lay.w_dip, lay.h_dip, pal.border);

    // Subtle bottom-right resize corner indicator separated cleanly from footer stats
    for &(dx, dy) in &[(4.5f32, 4.5f32), (8.0, 4.5), (4.5, 8.0)] {
        canvas.fill_rect_dip(
            lay.w_dip - dx - 1.5,
            lay.h_dip - dy - 1.5,
            lay.w_dip - dx,
            lay.h_dip - dy,
            pal.scroll_thumb,
        );
    }
}
