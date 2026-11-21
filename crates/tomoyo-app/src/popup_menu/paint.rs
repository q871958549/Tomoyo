//! Pure-Rust SDF background + ClearType GDI popup menu painting (Minimalist Modern spec).

use super::{PopupEntry, PopupState};
use crate::renderer::{dip_rect, draw_text_gdi, Canvas, DibSurface, Rgb};
use windows::Win32::Graphics::Gdi::{BitBlt, GdiFlush, HDC, DT_CENTER, DT_LEFT, DT_RIGHT, SRCCOPY};

pub(super) fn paint_popup(
    screen_hdc: HDC,
    width: i32,
    height: i32,
    st: &mut PopupState<'_>,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    if st.surface.as_ref().map(|s| (s.width, s.height)) != Some((width, height)) {
        st.surface = None;
        st.surface = DibSurface::create(screen_hdc, width, height);
    }
    let Some(surface) = st.surface.as_mut() else {
        return;
    };
    let pal = st.pal;
    let scale = st.scale;
    let danger_fg = if pal.is_dark { Rgb(0xF8, 0x71, 0x71) } else { Rgb(0xDC, 0x26, 0x26) };
    let danger_bg = if pal.is_dark { Rgb(0x35, 0x19, 0x1D) } else { Rgb(0xFE, 0xF2, 0xF2) };

    {
        let mut canvas = Canvas {
            buf: surface.pixels_mut(),
            w: width,
            h: height,
            scale,
        };
        canvas.clear(pal.pop_bg);
        for (idx, (ent, met)) in st.entries.iter().zip(st.metrics.iter()).enumerate() {
            match ent {
                PopupEntry::Separator => {
                    let sy = (met.y0 + met.y1) * 0.5;
                    canvas.fill_rect_dip(10.0, sy, st.w_dip - 10.0, sy + 1.0, pal.divider);
                }
                PopupEntry::Item { danger, disabled, .. } if !*disabled && st.hovered == Some(idx) => {
                    let hover_fill = if *danger { danger_bg } else { pal.chip_active_bg };
                    canvas.fill_rounded_rect(
                        5.0,
                        met.y0 + 1.0,
                        st.w_dip - 5.0,
                        met.y1 - 1.0,
                        6.0,
                        hover_fill,
                    );
                }
                _ => {}
            }
        }
        canvas.fill_rect_dip(0.0, 0.0, st.w_dip, 1.0, pal.border);
        canvas.fill_rect_dip(0.0, st.h_dip - 1.0, st.w_dip, st.h_dip, pal.border);
        canvas.fill_rect_dip(0.0, 0.0, 1.0, st.h_dip, pal.border);
        canvas.fill_rect_dip(st.w_dip - 1.0, 0.0, st.w_dip, st.h_dip, pal.border);
    }

    let dc = surface.mem_dc;
    unsafe {
        let _ = GdiFlush();
        for (idx, (ent, met)) in st.entries.iter().zip(st.metrics.iter()).enumerate() {
            let PopupEntry::Item {
                label,
                shortcut,
                checked,
                danger,
                disabled,
                ..
            } = ent
            else {
                continue;
            };
            let is_hov = !*disabled && st.hovered == Some(idx);
            let fg: Rgb = if *disabled {
                pal.text_muted
            } else if *danger {
                danger_fg
            } else if is_hov {
                pal.chip_active_text
            } else {
                pal.text_primary
            };
            let mut right_x = st.w_dip - 12.0;

            if *checked {
                let mark_col = if *disabled {
                    pal.text_muted
                } else if is_hov {
                    pal.chip_active_text
                } else {
                    pal.accent
                };
                draw_text_gdi(
                    dc,
                    st.fonts.chip_active,
                    mark_col,
                    "✓",
                    dip_rect(8.0, met.y0, 26.0, met.y1, scale),
                    DT_CENTER,
                );
            }
            if let Some(sc) = shortcut {
                let sc_col = if *disabled {
                    pal.text_muted
                } else if *danger {
                    danger_fg
                } else if is_hov {
                    pal.chip_active_text
                } else {
                    pal.text_muted
                };
                let sc_w = (sc.chars().count() as f32 * 7.2 + 8.0).clamp(32.0, 96.0);
                draw_text_gdi(
                    dc,
                    st.fonts.footer,
                    sc_col,
                    sc,
                    dip_rect(right_x - sc_w, met.y0, right_x, met.y1, scale),
                    DT_RIGHT,
                );
                right_x -= sc_w + 8.0;
            }
            draw_text_gdi(
                dc,
                if is_hov { st.fonts.chip_active } else { st.fonts.chip },
                fg,
                label,
                dip_rect(28.0, met.y0, right_x, met.y1, scale),
                DT_LEFT,
            );
        }
        let _ = BitBlt(screen_hdc, 0, 0, width, height, Some(dc), 0, 0, SRCCOPY);
    }
}
