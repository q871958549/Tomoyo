//! Pure-Rust SDF background + ClearType GDI popup menu painting (Minimalist Modern spec).

use super::panel::PanelState;
use super::PopupEntry;
use crate::renderer::{dip_rect, draw_text_gdi, Canvas, DibSurface, FontPack, Rgb, ThemePalette};
use windows::Win32::Graphics::Gdi::{BitBlt, GdiFlush, HDC, DT_CENTER, DT_LEFT, DT_RIGHT, SRCCOPY};

pub(super) fn paint_panel(
    screen_hdc: HDC,
    width: i32,
    height: i32,
    pal: ThemePalette,
    scale: f32,
    fonts: &FontPack,
    panel: &mut PanelState,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    if panel.surface.as_ref().map(|s| (s.width, s.height)) != Some((width, height)) {
        panel.surface = None;
        panel.surface = DibSurface::create(screen_hdc, width, height);
    }
    let Some(surface) = panel.surface.as_mut() else {
        return;
    };
    let danger_fg = if pal.is_dark { Rgb(0xF8, 0x71, 0x71) } else { Rgb(0xDC, 0x26, 0x26) };
    let danger_bg = if pal.is_dark { Rgb(0x35, 0x19, 0x1D) } else { Rgb(0xFE, 0xF2, 0xF2) };

    {
        let mut canvas = Canvas { buf: surface.pixels_mut(), w: width, h: height, scale };
        canvas.clear(pal.pop_bg);
        for (idx, (ent, met)) in panel.entries.iter().zip(panel.metrics.iter()).enumerate() {
            let is_active_sub = !panel.is_child && panel.active_sub_idx == Some(idx);
            let is_hov = panel.hovered == Some(idx) || is_active_sub;
            match ent {
                PopupEntry::Separator => {
                    let sy = (met.y0 + met.y1) * 0.5;
                    canvas.fill_rect_dip(10.0, sy, panel.w_dip - 10.0, sy + 1.0, pal.divider);
                }
                PopupEntry::Submenu { .. } if is_hov => {
                    canvas.fill_rounded_rect(5.0, met.y0 + 1.0, panel.w_dip - 5.0, met.y1 - 1.0, 6.0, pal.chip_active_bg);
                }
                PopupEntry::Item { danger, disabled, .. } if !*disabled && is_hov => {
                    let fill = if *danger { danger_bg } else { pal.chip_active_bg };
                    canvas.fill_rounded_rect(5.0, met.y0 + 1.0, panel.w_dip - 5.0, met.y1 - 1.0, 6.0, fill);
                }
                _ => {}
            }
        }
        canvas.fill_rect_dip(0.0, 0.0, panel.w_dip, 1.0, pal.border);
        canvas.fill_rect_dip(0.0, panel.h_dip - 1.0, panel.w_dip, panel.h_dip, pal.border);
        canvas.fill_rect_dip(0.0, 0.0, 1.0, panel.h_dip, pal.border);
        canvas.fill_rect_dip(panel.w_dip - 1.0, 0.0, panel.w_dip, panel.h_dip, pal.border);
    }

    let dc = surface.mem_dc;
    unsafe {
        let _ = GdiFlush();
        for (idx, (ent, met)) in panel.entries.iter().zip(panel.metrics.iter()).enumerate() {
            let is_active_sub = !panel.is_child && panel.active_sub_idx == Some(idx);
            let is_hov = panel.hovered == Some(idx) || is_active_sub;
            match ent {
                PopupEntry::Separator => continue,
                PopupEntry::Submenu { label, .. } => {
                    let fg = if is_hov { pal.chip_active_text } else { pal.text_primary };
                    let chev_col = if is_hov { pal.chip_active_text } else { pal.text_muted };
                    draw_text_gdi(
                        dc,
                        if is_hov { fonts.chip_active } else { fonts.chip },
                        fg,
                        label,
                        dip_rect(28.0, met.y0, panel.w_dip - 28.0, met.y1, scale),
                        DT_LEFT,
                    );
                    draw_text_gdi(
                        dc,
                        fonts.chip,
                        chev_col,
                        "▸",
                        dip_rect(panel.w_dip - 26.0, met.y0, panel.w_dip - 10.0, met.y1, scale),
                        DT_RIGHT,
                    );
                }
                PopupEntry::Item { label, shortcut, checked, danger, disabled, .. } => {
                    let is_item_hov = !*disabled && is_hov;
                    let fg = if *disabled {
                        pal.text_muted
                    } else if *danger {
                        danger_fg
                    } else if is_item_hov {
                        pal.chip_active_text
                    } else {
                        pal.text_primary
                    };
                    let mut right_x = panel.w_dip - 12.0;

                    if *checked {
                        let mark_col = if *disabled {
                            pal.text_muted
                        } else if is_item_hov {
                            pal.chip_active_text
                        } else {
                            pal.accent
                        };
                        draw_text_gdi(dc, fonts.chip_active, mark_col, "✓", dip_rect(8.0, met.y0, 26.0, met.y1, scale), DT_CENTER);
                    }
                    if let Some(sc) = shortcut {
                        let sc_col = if *disabled {
                            pal.text_muted
                        } else if *danger {
                            danger_fg
                        } else if is_item_hov {
                            pal.chip_active_text
                        } else {
                            pal.text_muted
                        };
                        let sc_w = (sc.chars().count() as f32 * 7.2 + 8.0).clamp(32.0, 96.0);
                        draw_text_gdi(dc, fonts.footer, sc_col, sc, dip_rect(right_x - sc_w, met.y0, right_x, met.y1, scale), DT_RIGHT);
                        right_x -= sc_w + 8.0;
                    }
                    draw_text_gdi(
                        dc,
                        if is_item_hov { fonts.chip_active } else { fonts.chip },
                        fg,
                        label,
                        dip_rect(28.0, met.y0, right_x, met.y1, scale),
                        DT_LEFT,
                    );
                }
            }
        }
        let _ = BitBlt(screen_hdc, 0, 0, width, height, Some(dc), 0, 0, SRCCOPY);
    }
}
