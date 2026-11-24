//! ClearType GDI typography, shell icon blit, preview pane, action menu, and footer pass.

use super::canvas::Canvas;
use super::filter_bar::paint_filter_bar_gdi;
use super::gdi::{
    dip_rect, draw_multiline_gdi, draw_text_gdi, draw_text_highlighted_gdi, DibSurface, FontPack,
};
use super::theme::{category_rgb, Rgb};
use super::{FrameLayout, RenderState};
use crate::icons::IconCache;
use crate::image_icons::ImageIconCache;
use std::path::Path;
use tomoyo_core::{current_process_stats, highlight_title_ranges, Lang};
use tomoyo_protocol::{extension_short_tag, IconSpec, ResultCategory, ResultItem};
use windows::Win32::Graphics::Gdi::{BitBlt, GdiFlush, HDC, SRCCOPY, DT_CENTER, DT_LEFT, DT_RIGHT};
use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_NORMAL};

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_gdi_pass(
    surface: &mut DibSurface,
    fonts: &FontPack,
    icons: &mut IconCache,
    image_icons: &mut ImageIconCache,
    width: i32,
    height: i32,
    scale: f32,
    lay: &FrameLayout,
    state: &RenderState<'_>,
    screen_hdc: HDC,
) {
    let pal = state.theme;
    let dc = surface.mem_dc;
    unsafe {
        let _ = GdiFlush();

        let mut header_right_x = lay.w_dip - 16.0;
        if let Some(mode) = state.mode_badge {
            let pill_w = (mode.chars().count() as f32 * 9.5 + 22.0).clamp(58.0, 130.0);
            let px0 = header_right_x - pill_w;
            draw_text_gdi(dc, fonts.chip_active, pal.chip_active_text, mode, dip_rect(px0, 19.0, header_right_x, 43.0, scale), DT_CENTER);
            header_right_x = px0 - 8.0;
        }
        if let Some(cap) = state.attached_capsule {
            let cap_w = (cap.chars().count() as f32 * 8.5 + 24.0).clamp(96.0, 210.0);
            let cx0 = header_right_x - cap_w;
            draw_text_gdi(dc, fonts.chip, pal.text_primary, cap, dip_rect(cx0 + 6.0, 18.0, header_right_x - 6.0, 44.0, scale), DT_CENTER);
        }

        if state.filter_bar_visible {
            paint_filter_bar_gdi(
                dc,
                fonts,
                pal,
                lay.list_w_dip,
                lay.header_h,
                scale,
                state.filter_tabs,
                state.active_filter,
                state.hovered_filter,
                state.hovered_overflow,
                state.filter_counts,
                state.filter_bar_wrap,
            );
        }

        for slot in 0..lay.visible_count {
            let idx = state.scroll_offset + slot;
            let Some(item) = state.items.get(idx) else { break };
            let y0 = lay.list_top + (slot as f32) * lay.row_h;
            let is_sel = idx == state.selected;
            let icon_top = y0 + (lay.row_h - 30.0) * 0.5;
            let badge_top = y0 + (lay.row_h - 21.0) * 0.5;
            let key_top = y0 + (lay.row_h - 20.0) * 0.5;

            let mut drew_shell_icon = false;
            let icon_px = (30.0 * scale).round() as u32;
            if let IconSpec::ImageFile(path) = &item.icon
                && image_icons.get_cached(path, icon_px).is_some()
            {
                drew_shell_icon = true;
            } else if let IconSpec::ShellPath(path) = &item.icon
                && let Some(hicon) = icons.get_or_enqueue(path)
            {
                let isize = (30.0 * scale).round() as i32;
                let _ = DrawIconEx(dc, (22.0 * scale).round() as i32, (icon_top * scale).round() as i32, hicon, isize, isize, 0, None, DI_NORMAL);
                drew_shell_icon = true;
            }
            if !drew_shell_icon {
                if matches!(item.icon, IconSpec::ShellPath(_)) {
                    let mut c = Canvas { buf: surface.pixels_mut(), w: width, h: height, scale };
                    c.fill_rounded_rect(22.0, icon_top, 52.0, icon_top + 30.0, 7.0, category_rgb(item.category));
                    let _ = GdiFlush();
                }
                let label = fallback_badge_label(item);
                draw_text_gdi(dc, fonts.badge, Rgb(0xFF, 0xFF, 0xFF), label, dip_rect(22.0, icon_top, 52.0, icon_top + 30.0, scale), DT_CENTER);
            }

            let mut text_right = lay.list_w_dip - 20.0;
            if let Some((_, done)) = state.launching_row.filter(|(li, _)| *li == idx) {
                let lbl = if done {
                    state.lang.pick("✓ 已启动", "✓ Opened")
                } else {
                    state.lang.pick("启动中…", "Opening…")
                };
                let pw = 74.0;
                draw_text_gdi(dc, fonts.badge, Rgb(0xFF, 0xFF, 0xFF), lbl, dip_rect(text_right - pw, badge_top - 1.0, text_right, badge_top + 22.0, scale), DT_CENTER);
                text_right -= pw + 8.0;
            } else if is_sel {
                draw_text_gdi(dc, fonts.badge, pal.chip_active_text, "Enter", dip_rect(text_right - 44.0, key_top, text_right, key_top + 20.0, scale), DT_CENTER);
                text_right -= 50.0;
            } else if slot < 8 {
                let sc = format!("Alt+{}", slot + 1);
                draw_text_gdi(dc, fonts.footer, pal.text_muted, &sc, dip_rect(text_right - 42.0, key_top, text_right, key_top + 20.0, scale), DT_CENTER);
                text_right -= 48.0;
            }
            if let Some(badge) = &item.badge {
                let bw = (badge.chars().count() as f32 * 6.8 + 18.0).clamp(44.0, 120.0);
                let bx0 = text_right - bw;
                draw_text_gdi(dc, fonts.badge, pal.text_secondary, badge, dip_rect(bx0, badge_top, text_right, badge_top + 21.0, scale), DT_CENTER);
                text_right = bx0 - 10.0;
            }
            let hl_spans = highlight_title_ranges(state.query, &item.title);
            draw_text_highlighted_gdi(
                dc,
                fonts.title,
                pal.text_primary,
                pal.accent,
                &item.title,
                &hl_spans,
                dip_rect(62.0, y0 + 5.0, text_right, y0 + lay.row_h * 0.52, scale),
            );
            draw_text_gdi(dc, fonts.subtitle, pal.text_secondary, &item.subtitle, dip_rect(62.0, y0 + lay.row_h * 0.50, text_right, y0 + lay.row_h - 5.0, scale), DT_LEFT);
        }

        if lay.has_preview
            && let Some(prev) = state.preview {
                let (px0, px1) = (lay.list_w_dip + 14.0, lay.w_dip - 14.0);
                draw_text_gdi(dc, fonts.title, pal.text_primary, &prev.title, dip_rect(px0, lay.header_h + 10.0, px1, lay.header_h + 32.0, scale), DT_LEFT);
                draw_text_gdi(dc, fonts.footer, pal.text_secondary, &prev.meta_line, dip_rect(px0, lay.header_h + 32.0, px1, lay.header_h + 50.0, scale), DT_LEFT);
                let mut badge_shift = 0.0f32;
                if !prev.badges.is_empty() {
                    let mut bx = px0;
                    let by = lay.header_h + 52.0;
                    for b in prev.badges.iter().take(5) {
                        let bw = (b.chars().count() as f32 * 6.5 + 14.0).clamp(36.0, 110.0);
                        if bx + bw > px1 { break; }
                        draw_text_gdi(dc, fonts.badge, pal.accent, b, dip_rect(bx, by, bx + bw, by + 18.0, scale), DT_CENTER);
                        bx += bw + 6.0;
                    }
                    badge_shift = 24.0;
                }
                let body_font = if prev.is_monospace { fonts.preview_mono } else { fonts.preview_body };
                let body_top = if let Some((iw, ih, _)) = &prev.image_rgba {
                    let avail_w = (px1 - px0).max(40.0);
                    let avail_h = (lay.fy0 - (lay.header_h + 60.0) - 52.0).clamp(60.0, 210.0);
                    let dh = (*ih as f32 * (avail_w / (*iw as f32)).min(avail_h / (*ih as f32)).min(1.0)).max(16.0);
                    lay.header_h + 66.0 + dh
                } else {
                    lay.header_h + 58.0 + badge_shift
                };
                draw_multiline_gdi(dc, body_font, pal.text_primary, &prev.body, dip_rect(px0, body_top, px1, lay.fy0 - 10.0, scale));
            }

        if state.action_menu_open
            && let Some(sel) = state.items.get(state.selected) {
                let n = sel.actions.len().min(6);
                let (mx1, my1) = (lay.list_w_dip - 14.0, lay.fy0 - 10.0);
                let (mx0, my0) = (mx1 - 240.0, (my1 - (n as f32 * 30.0 + 16.0)).max(lay.list_top + 4.0));
                for (ai, act) in sel.actions.iter().take(n).enumerate() {
                    let ay0 = my0 + 8.0 + (ai as f32) * 30.0;
                    let is_act_sel = ai == state.selected_action;
                    let (fnt, col) = if is_act_sel {
                        (fonts.chip_active, pal.chip_active_text)
                    } else {
                        (fonts.chip, pal.text_primary)
                    };
                    draw_text_gdi(dc, fnt, col, &act.label, dip_rect(mx0 + 14.0, ay0, mx1 - 14.0, ay0 + 28.0, scale), DT_LEFT);
                }
            }

        let left_text = if let Some(toast) = state.toast {
            toast.to_string()
        } else if state.action_menu_open {
            state.lang.pick("动作菜单：↑↓ 选择 · 回车执行 · Esc 返回", "Actions: ↑↓ Select · Enter Execute · Esc Close").to_string()
        } else {
            let ev = state.everything_status.unwrap_or_else(|| state.lang.pick("Everything 未运行", "Everything offline"));
            match state.lang {
                Lang::Zh => format!("{ev} · Ctrl+Tab 分组 · 双击打开 · Ctrl+, 设置 · Ctrl+P 预览"),
                Lang::En => format!("{ev} · Ctrl+Tab Filter · DblClick Open · Ctrl+, Settings"),
            }
        };

        let left_right = if let Some(ver) = state.update_badge {
            let uw = (ver.chars().count() as f32 * 7.2 + 18.0).clamp(64.0, 120.0);
            let ux0 = lay.w_dip - 190.0 - uw;
            if ux0 > 220.0 {
                draw_text_gdi(dc, fonts.badge, pal.chip_active_text, ver, dip_rect(ux0, lay.fy0 + 5.0, lay.w_dip - 190.0, lay.fy0 + 25.0, scale), DT_CENTER);
                ux0 - 10.0
            } else {
                lay.w_dip - 186.0
            }
        } else {
            lay.w_dip - 186.0
        };
        let footer_col = if let Some(t) = state.toast {
            if t.starts_with('✗') {
                if pal.is_dark { Rgb(0xF8, 0x71, 0x71) } else { Rgb(0xDC, 0x26, 0x26) }
            } else if t.starts_with('✓') {
                if pal.is_dark { Rgb(0x34, 0xD3, 0x99) } else { Rgb(0x05, 0x96, 0x69) }
            } else if state.launching_row.is_some() {
                pal.accent
            } else {
                pal.text_secondary
            }
        } else {
            pal.text_secondary
        };
        draw_text_gdi(dc, fonts.footer, footer_col, &left_text, dip_rect(28.0, lay.fy0, left_right, lay.h_dip, scale), DT_LEFT);

        let stats = current_process_stats();
        let mem_text = match state.lang {
            Lang::Zh => format!("{} 项 · 内存 {:.1} MB", state.items.len(), stats.working_set_mb()),
            Lang::En => format!("{} hits · RAM {:.1} MB", state.items.len(), stats.working_set_mb()),
        };
        draw_text_gdi(dc, fonts.footer, pal.text_muted, &mem_text, dip_rect(lay.w_dip - 186.0, lay.fy0, lay.w_dip - 18.0, lay.h_dip, scale), DT_RIGHT);

        let _ = BitBlt(screen_hdc, 0, 0, width, height, Some(dc), 0, 0, SRCCOPY);
    }
}

fn fallback_badge_label(item: &ResultItem) -> &str {
    match &item.icon {
        IconSpec::Monogram(m) => m.as_str(),
        IconSpec::Symbol(s) => s.as_str(),
        IconSpec::ImageFile(_) => item.category.short_label(),
        IconSpec::ShellPath(p) => {
            if item.category == ResultCategory::File {
                let ext = Path::new(p).extension().and_then(|e| e.to_str()).unwrap_or("");
                if !ext.is_empty() {
                    return extension_short_tag(&ext.to_ascii_lowercase());
                }
            }
            item.category.short_label()
        }
    }
}
