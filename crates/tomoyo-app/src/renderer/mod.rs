//! Ultra-low-memory Fluent renderer orchestrator (SDF 32-bit DIB + ClearType GDI + Shell Icons).

mod canvas;
mod gdi;
mod theme;

pub use theme::{Rgb, ThemePalette};

use crate::brand::paint_modern_white_emblem;
use crate::icons::IconCache;
use canvas::Canvas;
use gdi::{dip_rect, draw_multiline_gdi, draw_text_gdi, DibSurface, FontPack};
use theme::category_rgb;
use tomoyo_core::{current_process_stats, Lang, ResolvedPreview};
use tomoyo_protocol::{IconSpec, ResultItem};
use windows::Win32::Graphics::Gdi::{BitBlt, GdiFlush, HDC, SRCCOPY, DT_CENTER, DT_LEFT, DT_RIGHT};
use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_NORMAL};

pub const DEFAULT_WINDOW_WIDTH_DIP: i32 = 720;
pub const DEFAULT_PREVIEW_WIDTH_DIP: i32 = 310;
pub const HEADER_HEIGHT_DIP: i32 = 62;
pub const ROW_HEIGHT_DIP: i32 = 54;
pub const FOOTER_HEIGHT_DIP: i32 = 30;
pub const MIN_WINDOW_WIDTH_DIP: i32 = 520;
pub const MAX_WINDOW_WIDTH_DIP: i32 = 1600;

pub struct RenderState<'a> {
    pub lang: Lang,
    pub theme: ThemePalette,
    pub mode_badge: Option<&'a str>,
    pub attached_capsule: Option<&'a str>,
    pub update_badge: Option<&'a str>,
    pub items: &'a [ResultItem],
    pub selected: usize,
    pub hovered: Option<usize>,
    pub scroll_offset: usize,
    pub max_visible_rows: usize,
    pub preview_width_dip: i32,
    pub action_menu_open: bool,
    pub selected_action: usize,
    pub preview: Option<&'a ResolvedPreview>,
    pub everything_status: Option<&'static str>,
    pub toast: Option<&'a str>,
    pub dpi_scale: f32,
}

struct FrameLayout {
    w_dip: f32,
    h_dip: f32,
    header_h: f32,
    row_h: f32,
    list_top: f32,
    fy0: f32,
    visible_cap: usize,
    visible_count: usize,
    has_preview: bool,
    list_w_dip: f32,
}

pub struct FluentRenderer {
    surface: Option<DibSurface>,
    fonts: Option<FontPack>,
    icons: IconCache,
    font_regular: &'static str,
    font_medium: &'static str,
}

impl FluentRenderer {
    pub fn new(font_regular: &'static str, font_medium: &'static str) -> windows::core::Result<Self> {
        Ok(Self {
            surface: None,
            fonts: None,
            icons: IconCache::new(),
            font_regular,
            font_medium,
        })
    }

    pub fn hibernate_release(&mut self) {
        self.icons.clear();
        self.surface = None;
        self.fonts = None;
    }

    pub fn is_hibernated(&self) -> bool {
        self.surface.is_none() && self.fonts.is_none()
    }

    pub fn paint(&mut self, screen_hdc: HDC, width: i32, height: i32, state: &RenderState<'_>) {
        if width <= 0 || height <= 0 {
            return;
        }
        let scale = state.dpi_scale.max(1.0);
        let dpi_key = (scale * 100.0).round() as i32;
        if self.fonts.as_ref().map(|f| f.dpi_key) != Some(dpi_key) {
            self.fonts = Some(FontPack::create(scale, self.font_regular, self.font_medium));
        }
        if self.surface.as_ref().map(|s| (s.width, s.height)) != Some((width, height)) {
            self.surface = None;
            self.surface = DibSurface::create(screen_hdc, width, height);
        }

        let Some(surface) = self.surface.as_mut() else { return };
        let Some(fonts) = self.fonts.as_ref() else { return };

        let lay = compute_layout(width, height, scale, state);
        paint_sdf_pass(surface, width, height, scale, &lay, state);
        paint_gdi_pass(surface, fonts, &mut self.icons, width, height, scale, &lay, state, screen_hdc);
    }
}

fn compute_layout(width: i32, height: i32, scale: f32, state: &RenderState<'_>) -> FrameLayout {
    let w_dip = width as f32 / scale;
    let h_dip = height as f32 / scale;
    let header_h = HEADER_HEIGHT_DIP as f32;
    let footer_h = FOOTER_HEIGHT_DIP as f32;
    let row_h = ROW_HEIGHT_DIP as f32;
    let list_top = header_h + 4.0;
    let max_rows_by_h = ((h_dip - header_h - footer_h - 4.0).max(0.0) / row_h).floor() as usize;
    let visible_cap = state.max_visible_rows.max(1).min(max_rows_by_h.max(1));
    let visible_count = state.items.len().min(visible_cap);
    let has_preview = state.preview.is_some() && !state.items.is_empty() && w_dip >= 680.0;
    let preview_w = (state.preview_width_dip as f32).clamp(220.0, (w_dip * 0.48).max(220.0));
    let list_w_dip = if has_preview { (w_dip - preview_w).max(380.0) } else { w_dip };
    FrameLayout {
        w_dip,
        h_dip,
        header_h,
        row_h,
        list_top,
        fy0: h_dip - footer_h,
        visible_cap,
        visible_count,
        has_preview,
        list_w_dip,
    }
}

fn paint_sdf_pass(
    surface: &mut DibSurface,
    width: i32,
    height: i32,
    scale: f32,
    lay: &FrameLayout,
    state: &RenderState<'_>,
) {
    let pal = state.theme;
    let mut canvas = Canvas {
        buf: surface.pixels_mut(),
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
        canvas.fill_rounded_rect(px0, 19.0, header_right_x, 43.0, 6.0, pal.pill_bg);
        canvas.stroke_rounded_rect(px0, 19.0, header_right_x, 43.0, 6.0, pal.accent);
        header_right_x = px0 - 8.0;
    }
    if let Some(cap) = state.attached_capsule {
        let cap_w = (cap.chars().count() as f32 * 8.5 + 24.0).clamp(96.0, 210.0);
        let cx0 = header_right_x - cap_w;
        canvas.fill_rounded_rect(cx0, 18.0, header_right_x, 44.0, 8.0, pal.card_sel);
        canvas.stroke_rounded_rect(cx0, 18.0, header_right_x, 44.0, 8.0, pal.accent);
    }

    if lay.has_preview {
        if let Some(prev) = state.preview {
            if let Some((iw, ih, rgba)) = &prev.image_rgba {
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
        }
    }

    if !state.items.is_empty() {
        canvas.fill_rect_dip(12.0, lay.header_h, lay.w_dip - 12.0, lay.header_h + 1.0, pal.divider);
    }

    for slot in 0..lay.visible_count {
        let idx = state.scroll_offset + slot;
        let Some(item) = state.items.get(idx) else { break };
        let y0 = lay.list_top + (slot as f32) * lay.row_h;
        let y1 = y0 + lay.row_h - 2.0;
        let is_sel = idx == state.selected;
        if is_sel {
            canvas.fill_rounded_rect(10.0, y0 + 2.0, lay.list_w_dip - 12.0, y1 - 2.0, 8.0, pal.card_sel);
            canvas.fill_rounded_rect(10.0, y0 + 10.0, 13.5, y1 - 10.0, 2.0, pal.accent);
        } else if state.hovered == Some(idx) {
            canvas.fill_rounded_rect(10.0, y0 + 2.0, lay.list_w_dip - 12.0, y1 - 2.0, 8.0, pal.card_hover);
        }
        if !matches!(item.icon, IconSpec::ShellPath(_)) {
            canvas.fill_rounded_rect(22.0, y0 + 11.0, 52.0, y0 + 41.0, 7.0, category_rgb(item.category));
        }
        let mut text_right = lay.list_w_dip - 20.0;
        if is_sel || slot < 8 {
            text_right -= 48.0;
        }
        if let Some(badge) = &item.badge {
            let bw = (badge.chars().count() as f32 * 6.5 + 16.0).clamp(42.0, 118.0);
            canvas.fill_rounded_rect(text_right - bw, y0 + 16.0, text_right, y0 + 36.0, 5.0, pal.pill_bg);
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

    let dot_col = if state.everything_status.is_some() { Rgb(0x10, 0xB9, 0x81) } else { Rgb(0x6B, 0x72, 0x80) };
    canvas.fill_rounded_rect(14.0, lay.fy0 + 11.0, 22.0, lay.fy0 + 19.0, 4.0, dot_col);

    if let Some(ver) = state.update_badge {
        let uw = (ver.chars().count() as f32 * 7.0 + 18.0).clamp(64.0, 120.0);
        let ux0 = lay.w_dip - 184.0 - uw;
        if ux0 > 220.0 {
            canvas.fill_rounded_rect(ux0, lay.fy0 + 5.0, lay.w_dip - 184.0, lay.fy0 + 25.0, 5.0, pal.accent);
        }
    }

    if state.action_menu_open {
        if let Some(sel) = state.items.get(state.selected) {
            if !sel.actions.is_empty() {
                let n = sel.actions.len().min(6);
                let (mx1, my1) = (lay.list_w_dip - 14.0, lay.fy0 - 10.0);
                let (mx0, my0) = (mx1 - 240.0, (my1 - (n as f32 * 30.0 + 16.0)).max(lay.header_h + 8.0));
                canvas.fill_rounded_rect(mx0, my0, mx1, my1, 8.0, pal.pop_bg);
                canvas.stroke_rounded_rect(mx0, my0, mx1, my1, 8.0, pal.accent);
                if state.selected_action < n {
                    let ay0 = my0 + 8.0 + (state.selected_action as f32) * 30.0;
                    canvas.fill_rounded_rect(mx0 + 6.0, ay0, mx1 - 6.0, ay0 + 28.0, 5.0, pal.accent);
                }
            }
        }
    }

    canvas.fill_rect_dip(0.0, 0.0, lay.w_dip, 1.0, pal.border);
    canvas.fill_rect_dip(0.0, lay.h_dip - 1.0, lay.w_dip, lay.h_dip, pal.border);
    canvas.fill_rect_dip(0.0, 0.0, 1.0, lay.h_dip, pal.border);
    canvas.fill_rect_dip(lay.w_dip - 1.0, 0.0, lay.w_dip, lay.h_dip, pal.border);
    for step in 0..3 {
        let off = 4.0 + (step as f32) * 3.5;
        canvas.fill_rect_dip(lay.w_dip - off - 1.5, lay.h_dip - 4.5, lay.w_dip - off, lay.h_dip - 3.0, pal.text_muted);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_gdi_pass(
    surface: &mut DibSurface,
    fonts: &FontPack,
    icons: &mut IconCache,
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
            draw_text_gdi(dc, fonts.badge, pal.text_primary, mode, dip_rect(px0, 19.0, header_right_x, 43.0, scale), DT_CENTER);
            header_right_x = px0 - 8.0;
        }
        if let Some(cap) = state.attached_capsule {
            let cap_w = (cap.chars().count() as f32 * 8.5 + 24.0).clamp(96.0, 210.0);
            let cx0 = header_right_x - cap_w;
            draw_text_gdi(dc, fonts.badge, pal.text_primary, cap, dip_rect(cx0 + 6.0, 18.0, header_right_x - 6.0, 44.0, scale), DT_CENTER);
        }

        for slot in 0..lay.visible_count {
            let idx = state.scroll_offset + slot;
            let Some(item) = state.items.get(idx) else { break };
            let y0 = lay.list_top + (slot as f32) * lay.row_h;
            let y1 = y0 + lay.row_h - 2.0;
            let is_sel = idx == state.selected;

            let mut drew_shell_icon = false;
            if let IconSpec::ShellPath(path) = &item.icon {
                if let Some(hicon) = icons.get_or_load(path) {
                    let isize = (30.0 * scale).round() as i32;
                    let _ = DrawIconEx(dc, (22.0 * scale).round() as i32, ((y0 + 11.0) * scale).round() as i32, hicon, isize, isize, 0, None, DI_NORMAL);
                    drew_shell_icon = true;
                }
            }
            if !drew_shell_icon {
                if matches!(item.icon, IconSpec::ShellPath(_)) {
                    let mut c = Canvas { buf: surface.pixels_mut(), w: width, h: height, scale };
                    c.fill_rounded_rect(22.0, y0 + 11.0, 52.0, y0 + 41.0, 7.0, category_rgb(item.category));
                    let _ = GdiFlush();
                }
                let label = match &item.icon {
                    IconSpec::Monogram(m) => m.as_str(),
                    IconSpec::Symbol(s) => s.as_str(),
                    _ => item.category.short_label(),
                };
                draw_text_gdi(dc, fonts.badge, Rgb(0xFF, 0xFF, 0xFF), label, dip_rect(22.0, y0 + 11.0, 52.0, y0 + 41.0, scale), DT_CENTER);
            }

            let mut text_right = lay.list_w_dip - 20.0;
            let shortcut = if is_sel { "Enter".to_string() } else if slot < 8 { format!("Alt+{}", slot + 1) } else { String::new() };
            if !shortcut.is_empty() {
                draw_text_gdi(dc, fonts.footer, pal.text_muted, &shortcut, dip_rect(text_right - 40.0, y0 + 8.0, text_right, y1 - 8.0, scale), DT_RIGHT);
                text_right -= 48.0;
            }
            if let Some(badge) = &item.badge {
                let bw = (badge.chars().count() as f32 * 6.5 + 16.0).clamp(42.0, 118.0);
                let bx0 = text_right - bw;
                draw_text_gdi(dc, fonts.badge, pal.text_muted, badge, dip_rect(bx0, y0 + 16.0, text_right, y0 + 36.0, scale), DT_CENTER);
                text_right = bx0 - 10.0;
            }
            draw_text_gdi(dc, fonts.title, pal.text_primary, &item.title, dip_rect(62.0, y0 + 5.0, text_right, y0 + 28.0, scale), DT_LEFT);
            draw_text_gdi(dc, fonts.subtitle, pal.text_muted, &item.subtitle, dip_rect(62.0, y0 + 28.0, text_right, y0 + 47.0, scale), DT_LEFT);
        }

        if lay.has_preview {
            if let Some(prev) = state.preview {
                let (px0, px1) = (lay.list_w_dip + 14.0, lay.w_dip - 14.0);
                draw_text_gdi(dc, fonts.title, pal.text_primary, &prev.title, dip_rect(px0, lay.header_h + 10.0, px1, lay.header_h + 32.0, scale), DT_LEFT);
                draw_text_gdi(dc, fonts.footer, pal.text_muted, &prev.meta_line, dip_rect(px0, lay.header_h + 32.0, px1, lay.header_h + 50.0, scale), DT_LEFT);
                let body_font = if prev.is_monospace { fonts.preview_mono } else { fonts.preview_body };
                let body_top = if let Some((iw, ih, _)) = &prev.image_rgba {
                    let avail_w = (px1 - px0).max(40.0);
                    let avail_h = (lay.fy0 - (lay.header_h + 60.0) - 52.0).clamp(60.0, 210.0);
                    let dh = (*ih as f32 * (avail_w / (*iw as f32)).min(avail_h / (*ih as f32)).min(1.0)).max(16.0);
                    lay.header_h + 66.0 + dh
                } else {
                    lay.header_h + 58.0
                };
                draw_multiline_gdi(dc, body_font, pal.text_primary, &prev.body, dip_rect(px0, body_top, px1, lay.fy0 - 10.0, scale));
            }
        }

        if state.action_menu_open {
            if let Some(sel) = state.items.get(state.selected) {
                let n = sel.actions.len().min(6);
                let (mx1, my1) = (lay.list_w_dip - 14.0, lay.fy0 - 10.0);
                let (mx0, my0) = (mx1 - 240.0, (my1 - (n as f32 * 30.0 + 16.0)).max(lay.header_h + 8.0));
                for (ai, act) in sel.actions.iter().take(n).enumerate() {
                    let ay0 = my0 + 8.0 + (ai as f32) * 30.0;
                    let col = if ai == state.selected_action { pal.on_accent } else { pal.text_primary };
                    draw_text_gdi(dc, fonts.footer, col, &act.label, dip_rect(mx0 + 14.0, ay0, mx1 - 14.0, ay0 + 28.0, scale), DT_LEFT);
                }
            }
        }

        let left_text = if let Some(toast) = state.toast {
            toast.to_string()
        } else if state.action_menu_open {
            state.lang.pick("动作菜单：↑↓ 选择 · 回车执行 · Esc 返回", "Actions: ↑↓ Select · Enter Execute · Esc Close").to_string()
        } else {
            let ev = state.everything_status.unwrap_or_else(|| state.lang.pick("Everything 未运行", "Everything offline"));
            match state.lang {
                Lang::Zh => format!("{ev} · f 文件 · cb 剪贴板 · kill 进程 · Ctrl+, 设置 · Ctrl+P 预览"),
                Lang::En => format!("{ev} · f Files · cb Clip · kill Proc · Ctrl+, Settings · Ctrl+P Preview"),
            }
        };

        let left_right = if let Some(ver) = state.update_badge {
            let uw = (ver.chars().count() as f32 * 7.0 + 18.0).clamp(64.0, 120.0);
            let ux0 = lay.w_dip - 184.0 - uw;
            if ux0 > 220.0 {
                draw_text_gdi(dc, fonts.badge, pal.on_accent, ver, dip_rect(ux0, lay.fy0 + 5.0, lay.w_dip - 184.0, lay.fy0 + 25.0, scale), DT_CENTER);
                ux0 - 10.0
            } else {
                lay.w_dip - 175.0
            }
        } else {
            lay.w_dip - 175.0
        };
        draw_text_gdi(dc, fonts.footer, pal.text_muted, &left_text, dip_rect(28.0, lay.fy0, left_right, lay.h_dip, scale), DT_LEFT);

        let stats = current_process_stats();
        let mem_text = match state.lang {
            Lang::Zh => format!("{} 项 · 内存 {:.1} MB", state.items.len(), stats.working_set_mb()),
            Lang::En => format!("{} hits · RAM {:.1} MB", state.items.len(), stats.working_set_mb()),
        };
        draw_text_gdi(dc, fonts.footer, pal.text_muted, &mem_text, dip_rect(lay.w_dip - 180.0, lay.fy0, lay.w_dip - 14.0, lay.h_dip, scale), DT_RIGHT);

        let _ = BitBlt(screen_hdc, 0, 0, width, height, Some(dc), 0, 0, SRCCOPY);
    }
}
