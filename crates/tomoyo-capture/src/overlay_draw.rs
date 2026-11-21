//! Pure-Rust BGRA compositing + ClearType GDI overlay rendering for the capture backdrop,
//! bright cutout, 8-handle frame, pixel magnifier loupe, and floating action toolbar.

use crate::annotate::{render_annotations, render_single, AnnotColor, Annotation, ToolKind, COLOR_PRESETS};
use crate::screen::{DesktopSnapshot, ScreenRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarCmd {
    Tool(ToolKind),
    Color(usize),
    Undo,
    Pin,
    Ocr,
    Translate,
    Save,
    Copy,
    Cancel,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolbarHit {
    pub cmd: ToolbarCmd,
    pub rect: ScreenRect,
    pub label: &'static str,
    pub active: bool,
    pub accent: bool,
    pub swatch: Option<AnnotColor>,
}

pub fn build_toolbar_layout(
    sel: ScreenRect,
    screen_w: i32,
    screen_h: i32,
    active_tool: ToolKind,
    active_color: usize,
) -> (ScreenRect, Vec<ToolbarHit>) {
    let specs: &[(ToolbarCmd, &str, i32, bool, bool, Option<AnnotColor>)] = &[
        (ToolbarCmd::Tool(ToolKind::Rect), "框", 30, active_tool == ToolKind::Rect, false, None),
        (ToolbarCmd::Tool(ToolKind::Ellipse), "圆", 30, active_tool == ToolKind::Ellipse, false, None),
        (ToolbarCmd::Tool(ToolKind::Arrow), "箭", 30, active_tool == ToolKind::Arrow, false, None),
        (ToolbarCmd::Tool(ToolKind::Pen), "笔", 30, active_tool == ToolKind::Pen, false, None),
        (ToolbarCmd::Tool(ToolKind::Mosaic), "码", 30, active_tool == ToolKind::Mosaic, false, None),
        (ToolbarCmd::Tool(ToolKind::StepNumber), "序", 30, active_tool == ToolKind::StepNumber, false, None),
        (ToolbarCmd::Undo, "撤", 30, false, false, None),
        (ToolbarCmd::Color(0), "", 20, active_color == 0, false, Some(COLOR_PRESETS[0])),
        (ToolbarCmd::Color(1), "", 20, active_color == 1, false, Some(COLOR_PRESETS[1])),
        (ToolbarCmd::Color(2), "", 20, active_color == 2, false, Some(COLOR_PRESETS[2])),
        (ToolbarCmd::Color(3), "", 20, active_color == 3, false, Some(COLOR_PRESETS[3])),
        (ToolbarCmd::Pin, "贴图", 44, false, false, None),
        (ToolbarCmd::Ocr, "OCR", 44, false, false, None),
        (ToolbarCmd::Translate, "翻译", 44, false, false, None),
        (ToolbarCmd::Save, "保存", 44, false, false, None),
        (ToolbarCmd::Copy, "复制", 48, false, true, None),
        (ToolbarCmd::Cancel, "✕", 30, false, false, None),
    ];

    let total_w: i32 = specs.iter().map(|s| s.2 + 4).sum::<i32>() + 12;
    let bar_h = 36;
    let bar_x = (sel.right() - total_w).clamp(8, (screen_w - total_w - 8).max(8));
    let bar_y = if sel.bottom() + 8 + bar_h <= screen_h - 6 {
        sel.bottom() + 8
    } else if sel.y - 8 - bar_h >= 6 {
        sel.y - 8 - bar_h
    } else {
        (sel.bottom() - bar_h - 8).max(8)
    };

    let bar_rect = ScreenRect { x: bar_x, y: bar_y, w: total_w, h: bar_h };
    let mut hits = Vec::with_capacity(specs.len());
    let mut cx = bar_x + 8;
    for &(cmd, label, w, active, accent, swatch) in specs {
        hits.push(ToolbarHit {
            cmd,
            rect: ScreenRect { x: cx, y: bar_y + 4, w, h: 28 },
            label,
            active,
            accent,
            swatch,
        });
        cx += w + 4;
    }
    (bar_rect, hits)
}

#[allow(clippy::too_many_arguments)]
pub fn compose_overlay_pixels(
    dst: &mut [u8],
    snap: &DesktopSnapshot,
    dimmed: &[u8],
    sel: Option<ScreenRect>,
    hover_snap: Option<ScreenRect>,
    annotations: &[Annotation],
    live_ann: Option<&Annotation>,
    cursor: (i32, i32),
    show_loupe: bool,
    bar_rect: Option<ScreenRect>,
    hits: &[ToolbarHit],
) {
    let w = snap.width;
    let h = snap.height;
    let stride = (w as usize) * 4;
    dst.copy_from_slice(dimmed);

    let active_rect = sel.or(hover_snap);
    if let Some(r_raw) = active_rect {
        let r = r_raw.clamp_to(w, h);
        if r.w > 0 && r.h > 0 {
            for y in r.y..r.bottom() {
                let off = (y as usize) * stride + (r.x as usize) * 4;
                let len = (r.w as usize) * 4;
                dst[off..off + len].copy_from_slice(&snap.bgra[off..off + len]);
            }
            if sel.is_some() {
                render_annotations(dst, w, h, 0, 0, annotations);
                if let Some(a) = live_ann {
                    render_single(dst, w, h, 0, 0, a);
                }
            }
            let border_col = if sel.is_some() {
                AnnotColor(0x3B, 0x82, 0xF6)
            } else {
                AnnotColor(0x60, 0xA5, 0xFA)
            };
            stroke_box(dst, w, h, r, 2, border_col);
            if sel.is_some() {
                for (hx, hy) in handle_points(r) {
                    fill_box(dst, w, h, ScreenRect { x: hx - 3, y: hy - 3, w: 7, h: 7 }, AnnotColor(0xFF, 0xFF, 0xFF));
                    stroke_box(dst, w, h, ScreenRect { x: hx - 3, y: hy - 3, w: 7, h: 7 }, 1, border_col);
                }
            }
        }
    }

    if show_loupe {
        draw_magnifier_pixels(dst, snap, cursor);
    }

    if let Some(br) = bar_rect {
        fill_box_alpha(dst, w, h, br, AnnotColor(0x18, 0x1A, 0x22), 236);
        stroke_box(dst, w, h, br, 1, AnnotColor(0x3F, 0x44, 0x56));
        for btn in hits {
            if let Some(sw) = btn.swatch {
                let inner = ScreenRect { x: btn.rect.x + 3, y: btn.rect.y + 7, w: 14, h: 14 };
                if btn.active {
                    fill_box(dst, w, h, ScreenRect { x: inner.x - 2, y: inner.y - 2, w: 18, h: 18 }, AnnotColor(0xFF, 0xFF, 0xFF));
                }
                fill_box(dst, w, h, inner, sw);
            } else if btn.accent {
                fill_box(dst, w, h, btn.rect, AnnotColor(0x25, 0x63, 0xEB));
            } else if btn.active {
                fill_box(dst, w, h, btn.rect, AnnotColor(0x37, 0x41, 0x51));
                stroke_box(dst, w, h, btn.rect, 1, AnnotColor(0x60, 0xA5, 0xFA));
            }
        }
    }
}

pub fn precompute_dimmed(bgra: &[u8]) -> Vec<u8> {
    let mut out = bgra.to_vec();
    for px in out.chunks_exact_mut(4) {
        px[0] = ((px[0] as u16 * 112) >> 8) as u8;
        px[1] = ((px[1] as u16 * 112) >> 8) as u8;
        px[2] = ((px[2] as u16 * 112) >> 8) as u8;
        px[3] = 255;
    }
    out
}

pub fn handle_points(r: ScreenRect) -> [(i32, i32); 8] {
    let mx = r.x + r.w / 2;
    let my = r.y + r.h / 2;
    [
        (r.x, r.y), (mx, r.y), (r.right(), r.y),
        (r.x, my),             (r.right(), my),
        (r.x, r.bottom()), (mx, r.bottom()), (r.right(), r.bottom()),
    ]
}

pub fn loupe_rect(cursor: (i32, i32), screen_w: i32, screen_h: i32) -> ScreenRect {
    let lw = 148;
    let lh = 166;
    let x = if cursor.0 + 22 + lw < screen_w { cursor.0 + 22 } else { (cursor.0 - 22 - lw).max(6) };
    let y = if cursor.1 + 22 + lh < screen_h { cursor.1 + 22 } else { (cursor.1 - 22 - lh).max(6) };
    ScreenRect { x, y, w: lw, h: lh }
}

fn draw_magnifier_pixels(dst: &mut [u8], snap: &DesktopSnapshot, cursor: (i32, i32)) {
    let lr = loupe_rect(cursor, snap.width, snap.height);
    fill_box_alpha(dst, snap.width, snap.height, lr, AnnotColor(0x16, 0x18, 0x20), 240);
    stroke_box(dst, snap.width, snap.height, lr, 1, AnnotColor(0x4B, 0x55, 0x63));

    let grid = 11;
    let cell = 12;
    let gx0 = lr.x + (lr.w - grid * cell) / 2;
    let gy0 = lr.y + 8;
    for dy in 0..grid {
        for dx in 0..grid {
            let sx = cursor.0 + dx - grid / 2;
            let sy = cursor.1 + dy - grid / 2;
            let (r, g, b) = snap.pixel_rgb(sx, sy);
            let cell_rect = ScreenRect { x: gx0 + dx * cell, y: gy0 + dy * cell, w: cell, h: cell };
            fill_box(dst, snap.width, snap.height, cell_rect, AnnotColor(r, g, b));
        }
    }
    let mid = grid / 2;
    stroke_box(
        dst,
        snap.width,
        snap.height,
        ScreenRect { x: gx0 + mid * cell, y: gy0 + mid * cell, w: cell, h: cell },
        2,
        AnnotColor(0x3B, 0x82, 0xF6),
    );
}

#[cfg(windows)]
pub fn paint_overlay_gdi_labels(
    mem_dc: windows::Win32::Graphics::Gdi::HDC,
    font: windows::Win32::Graphics::Gdi::HFONT,
    snap: &DesktopSnapshot,
    sel: Option<ScreenRect>,
    cursor: (i32, i32),
    show_loupe: bool,
    hits: &[ToolbarHit],
    hex_lower: bool,
) {
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::{
        DrawTextW, SelectObject, SetBkMode, SetTextColor, DT_CENTER, DT_LEFT, DT_SINGLELINE,
        DT_VCENTER, TRANSPARENT,
    };

    unsafe {
        let old = SelectObject(mem_dc, font.into());
        SetBkMode(mem_dc, TRANSPARENT);

        for btn in hits {
            if btn.swatch.is_some() || btn.label.is_empty() {
                continue;
            }
            SetTextColor(mem_dc, COLORREF(0x00F8_FAFC));
            let mut rc = RECT {
                left: btn.rect.x,
                top: btn.rect.y,
                right: btn.rect.right(),
                bottom: btn.rect.bottom(),
            };
            let mut w: Vec<u16> = btn.label.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }

        if let Some(s) = sel {
            let dim = format!("{} × {}", s.w, s.h);
            let ty = if s.y >= 26 { s.y - 24 } else { s.y + 6 };
            let mut rc = RECT { left: s.x + 4, top: ty, right: s.x + 160, bottom: ty + 20 };
            SetTextColor(mem_dc, COLORREF(0x00FF_FFFF));
            let mut w: Vec<u16> = dim.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        }

        if show_loupe {
            let lr = loupe_rect(cursor, snap.width, snap.height);
            let (r, g, b) = snap.pixel_rgb(cursor.0, cursor.1);
            let hex = if hex_lower {
                format!("#{r:02x}{g:02x}{b:02x}")
            } else {
                format!("#{r:02X}{g:02X}{b:02X}")
            };
            let info = format!("{hex}  C复制");
            let mut rc = RECT {
                left: lr.x + 8,
                top: lr.bottom() - 26,
                right: lr.right() - 8,
                bottom: lr.bottom() - 4,
            };
            SetTextColor(mem_dc, COLORREF(0x00E5_E7EB));
            let mut w: Vec<u16> = info.encode_utf16().collect();
            DrawTextW(mem_dc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        let _ = SelectObject(mem_dc, old);
    }
}

fn fill_box(dst: &mut [u8], w: i32, h: i32, r: ScreenRect, c: AnnotColor) {
    let cl = r.clamp_to(w, h);
    for y in cl.y..cl.bottom() {
        for x in cl.x..cl.right() {
            let off = ((y as usize) * (w as usize) + (x as usize)) * 4;
            dst[off] = c.2;
            dst[off + 1] = c.1;
            dst[off + 2] = c.0;
            dst[off + 3] = 255;
        }
    }
}

fn fill_box_alpha(dst: &mut [u8], w: i32, h: i32, r: ScreenRect, c: AnnotColor, alpha: u8) {
    let cl = r.clamp_to(w, h);
    let a = alpha as u16;
    let inv = 255 - a;
    for y in cl.y..cl.bottom() {
        for x in cl.x..cl.right() {
            let off = ((y as usize) * (w as usize) + (x as usize)) * 4;
            dst[off] = ((c.2 as u16 * a + dst[off] as u16 * inv) >> 8) as u8;
            dst[off + 1] = ((c.1 as u16 * a + dst[off + 1] as u16 * inv) >> 8) as u8;
            dst[off + 2] = ((c.0 as u16 * a + dst[off + 2] as u16 * inv) >> 8) as u8;
            dst[off + 3] = 255;
        }
    }
}

fn stroke_box(dst: &mut [u8], w: i32, h: i32, r: ScreenRect, t: i32, c: AnnotColor) {
    if r.w < 2 || r.h < 2 {
        return;
    }
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.y, w: r.w, h: t }, c);
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.bottom() - t, w: r.w, h: t }, c);
    fill_box(dst, w, h, ScreenRect { x: r.x, y: r.y, w: t, h: r.h }, c);
    fill_box(dst, w, h, ScreenRect { x: r.right() - t, y: r.y, w: t, h: r.h }, c);
}

pub fn hit_test_handle(sel: ScreenRect, x: i32, y: i32) -> Option<usize> {
    handle_points(sel)
        .into_iter()
        .position(|(hx, hy)| (x - hx).abs() <= 6 && (y - hy).abs() <= 6)
}

pub fn resize_rect_by_handle(orig: ScreenRect, idx: usize, x: i32, y: i32) -> ScreenRect {
    let (mut l, mut t, mut r, mut b) = (orig.x, orig.y, orig.right(), orig.bottom());
    match idx {
        0 => { l = x; t = y; }
        1 => t = y,
        2 => { r = x; t = y; }
        3 => l = x,
        4 => r = x,
        5 => { l = x; b = y; }
        6 => b = y,
        7 => { r = x; b = y; }
        _ => {}
    }
    ScreenRect::from_points(l, t, r, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_stays_inside_viewport_bounds() {
        let (bar, hits) = build_toolbar_layout(
            ScreenRect { x: 50, y: 1050, w: 200, h: 20 },
            1920,
            1080,
            ToolKind::Rect,
            0,
        );
        assert!(bar.x >= 0 && bar.right() <= 1920);
        assert!(bar.y >= 0 && bar.bottom() <= 1080);
        assert!(hits.len() >= 16);
    }
}
