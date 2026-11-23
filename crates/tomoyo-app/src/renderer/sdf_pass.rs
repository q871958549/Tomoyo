//! Pure-Rust SDF anti-aliased shape, surface, card, scrollbar, and overlay pass.

use super::canvas::Canvas;
use super::filter_bar::paint_filter_bar_sdf;
use super::theme::{category_rgb, Rgb};
use super::{FrameLayout, RenderState};
use crate::brand::paint_modern_white_emblem;
#[cfg(windows)]
use crate::image_icons::ImageIconCache;
use tomoyo_protocol::IconSpec;

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_sdf_pass(
    pixels: &mut [u8],
    image_icons: &mut ImageIconCache,
    width: i32,
    height: i32,
    scale: f32,
    lay: &FrameLayout,
    state: &RenderState<'_>,
) {
    let icon_px = (30.0 * scale).round() as u32;
    paint_sdf_pass_inner(pixels, Some((image_icons, icon_px)), width, height, scale, lay, state);
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
    #[cfg(windows)] mut image_icons: Option<(&mut ImageIconCache, u32)>,
    #[cfg(not(windows))] _image_icons: Option<()>,
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
            && let Some((iw, ih, rgba)) = &prev.image_rgba {
                let (px0, px1) = (lay.list_w_dip + 14.0, lay.w_dip - 14.0);
                let avail_w = (px1 - px0).max(40.0);
                let avail_h = (lay.fy0 - (lay.header_h + 60.0) - 52.0).clamp(60.0, 210.0);
                let img_scale = (avail_w / (*iw as f32)).min(avail_h / (*ih as f32)).min(1.0);
                let (dw, dh) = ((*iw as f32 * img_scale).max(16.0), (*ih as f32 * img_scale).max(16.0));
                let (dx, dy) = (px0 + (avail_w - dw) * 0.5, lay.header_h + 56.0);
                canvas.fill_rounded_rect(dx - 3.0, dy - 3.0, dx + dw + 3.0, dy + dh + 3.0, 6.0, pal.pill_bg);
                canvas.draw_rgba_image(dx, dy, dw, dh, *iw, *ih, rgba);
                canvas.stroke_rounded_rect(dx - 3.0, dy - 3.0, dx + dw + 3.0, dy + dh + 3.0, 6.0, pal.border);
            }

    for slot in 0..lay.visible_count {
        let idx = state.scroll_offset + slot;
        let Some(item) = state.items.get(idx) else { break };
        let y0 = lay.list_top + (slot as f32) * lay.row_h;
        let y1 = y0 + lay.row_h - 2.0;
        let is_sel = idx == state.selected;
        let icon_top = y0 + (lay.row_h - 30.0) * 0.5;
        let badge_top = y0 + (lay.row_h - 21.0) * 0.5;
        let key_top = y0 + (lay.row_h - 20.0) * 0.5;
        if is_sel {
            canvas.fill_rounded_rect(10.0, y0 + 2.0, lay.list_w_dip - 12.0, y1 - 2.0, 8.0, pal.card_sel);
            canvas.stroke_rounded_rect(10.0, y0 + 2.0, lay.list_w_dip - 12.0, y1 - 2.0, 8.0, pal.sel_border);
            canvas.fill_rounded_rect(10.5, y0 + 11.0, 13.5, y1 - 11.0, 1.5, pal.accent);
        } else if state.hovered == Some(idx) {
            canvas.fill_rounded_rect(10.0, y0 + 2.0, lay.list_w_dip - 12.0, y1 - 2.0, 8.0, pal.card_hover);
        }
        #[cfg(windows)]
        let mut drew_img = false;
        #[cfg(windows)]
        if let IconSpec::ImageFile(path) = &item.icon
            && let Some((ref mut ic, sz)) = image_icons
            && let Some((iw, ih, rgba)) = ic.get_cached(path, sz)
        {
            canvas.draw_rgba_image(22.0, icon_top, 30.0, 30.0, *iw, *ih, rgba);
            drew_img = true;
        }
        #[cfg(windows)]
        let need_tile = !drew_img && !matches!(item.icon, IconSpec::ShellPath(_));
        #[cfg(not(windows))]
        let need_tile = !matches!(item.icon, IconSpec::ShellPath(_) | IconSpec::ImageFile(_));
        if need_tile {
            canvas.fill_rounded_rect(22.0, icon_top, 52.0, icon_top + 30.0, 7.0, category_rgb(item.category));
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

    if state.items.len() > lay.visible_cap && lay.visible_cap > 0 {
        let track_top = lay.list_top + 4.0;
        let track_h = (lay.fy0 - track_top - 6.0).max(20.0);
        let thumb_h = (track_h * (lay.visible_cap as f32 / state.items.len() as f32).clamp(0.12, 1.0)).max(24.0);
        let frac = (state.scroll_offset as f32 / state.items.len().saturating_sub(lay.visible_cap).max(1) as f32).clamp(0.0, 1.0);
        let ty0 = track_top + frac * (track_h - thumb_h);
        canvas.fill_rounded_rect(lay.list_w_dip - 7.0, ty0, lay.list_w_dip - 3.0, ty0 + thumb_h, 2.0, pal.scroll_thumb);
    }

    let dot_col = if let Some(t) = state.toast {
        if t.starts_with('✗') || t.starts_with('⚠') {
            Rgb(0xEF, 0x44, 0x44)
        } else if t.starts_with('✓') {
            Rgb(0x10, 0xB9, 0x81)
        } else if state.launching_row.is_some() {
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
