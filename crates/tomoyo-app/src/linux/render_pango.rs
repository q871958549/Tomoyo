//! Hybrid SDF + PangoCairo Linux renderer reusing `paint_sdf_pass` and GTK icon theme cache.

use crate::renderer::{
    compute_filter_bar_layout, compute_frame_layout, paint_sdf_pass, Canvas, FrameLayout,
    RenderState, Rgb,
};
use lru::LruCache;
use std::ffi::{c_char, c_int, c_void, CString};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use tomoyo_core::{current_process_stats, highlight_title_ranges, Lang};
use tomoyo_protocol::{extension_short_tag, IconSpec, ResultCategory, ResultItem};

unsafe extern "C" {
    fn tomoyo_cairo_begin_rounded(cr: *mut c_void, w: f64, h: f64, r: f64);
    fn tomoyo_cairo_end_rounded(cr: *mut c_void, w: f64, h: f64, r: f64, border_rgb: u32);
    fn tomoyo_cairo_blit_bgra(
        cr: *mut c_void,
        bgra: *const u8,
        w: c_int,
        h: c_int,
        x: f64,
        y: f64,
        dw: f64,
        dh: f64,
    );
    fn tomoyo_pango_draw_text(
        cr: *mut c_void,
        font_desc: *const c_char,
        rgb: u32,
        align: c_int,
        single_line: c_int,
        text: *const c_char,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        hl_pairs: *const u32,
        hl_count: usize,
        hl_rgb: u32,
    );
    fn tomoyo_linux_resolve_icon_path(
        icon_name: *const c_char,
        sz: c_int,
        out: *mut c_char,
        out_len: usize,
    ) -> c_int;
    fn tomoyo_linux_load_icon_bgra(
        icon_name_or_path: *const c_char,
        sz: c_int,
        out: *mut u8,
        out_len: usize,
    ) -> c_int;
    fn tomoyo_offscreen_cairo_create(w: c_int, h: c_int, bgra: *mut u8) -> *mut c_void;
    fn tomoyo_offscreen_cairo_destroy(cr: *mut c_void);
}

#[inline]
fn rgb_u32(c: Rgb) -> u32 {
    ((c.0 as u32) << 16) | ((c.1 as u32) << 8) | (c.2 as u32)
}

pub struct LinuxRenderer {
    pixels: Vec<u8>,
    icon_cache: LruCache<String, Option<Vec<u8>>>,
    font_family: &'static str,
}

impl LinuxRenderer {
    pub fn new(font_family: &'static str) -> Self {
        Self {
            pixels: Vec::new(),
            icon_cache: LruCache::new(NonZeroUsize::new(256).unwrap()),
            font_family,
        }
    }

    pub fn hibernate_release(&mut self) {
        self.pixels.clear();
        self.pixels.shrink_to_fit();
        self.icon_cache.clear();
    }

    pub fn is_hibernated(&self) -> bool {
        self.pixels.is_empty() && self.icon_cache.is_empty()
    }

    pub fn paint_offscreen_for_test(&mut self, w_px: i32, h_px: i32, state: &RenderState<'_>) {
        if w_px <= 0 || h_px <= 0 {
            return;
        }
        let mut surface_out = vec![0u8; (w_px * h_px * 4) as usize];
        let cr = unsafe { tomoyo_offscreen_cairo_create(w_px, h_px, surface_out.as_mut_ptr()) };
        if !cr.is_null() {
            self.paint_cairo(cr, w_px, h_px, state);
            unsafe { tomoyo_offscreen_cairo_destroy(cr); }
        }
    }

    pub fn paint_cairo(&mut self, cr: *mut c_void, width: i32, height: i32, state: &RenderState<'_>) {
        if cr.is_null() || width <= 0 || height <= 0 {
            return;
        }
        let needed = (width as usize) * (height as usize) * 4;
        if self.pixels.len() != needed {
            self.pixels.resize(needed, 0);
        }
        let scale = state.dpi_scale.max(1.0);
        let lay = compute_frame_layout(width, height, scale, state);
        paint_sdf_pass(&mut self.pixels, width, height, scale, &lay, state);

        // Draw category fallback rect for rows without loaded theme icons before blitting SDF
        self.draw_fallback_icon_tiles(&lay, width, height, scale, state);
        let radius = 12.0 * (scale as f64);
        unsafe {
            tomoyo_cairo_begin_rounded(cr, width as f64, height as f64, radius);
            tomoyo_cairo_blit_bgra(
                cr,
                self.pixels.as_ptr(),
                width,
                height,
                0.0,
                0.0,
                width as f64,
                height as f64,
            );
        }
        self.paint_pango_pass(cr, scale, &lay, state);
        unsafe {
            tomoyo_cairo_end_rounded(cr, width as f64, height as f64, radius, rgb_u32(state.theme.border));
        }
    }

    fn draw_fallback_icon_tiles(
        &mut self,
        lay: &FrameLayout,
        width: i32,
        height: i32,
        scale: f32,
        state: &RenderState<'_>,
    ) {
        let icon_sz = (30.0 * scale).round() as i32;
        for slot in 0..lay.visible_count {
            let idx = state.scroll_offset + slot;
            let Some(item) = state.items.get(idx) else { break };
            let y0 = lay.list_top + (slot as f32) * lay.row_h;
            let has_theme_icon = extract_icon_hint(item)
                .and_then(|hint| self.load_cached_icon(hint, icon_sz))
                .is_some();
            if !has_theme_icon && matches!(item.icon, IconSpec::ShellPath(_) | IconSpec::ImageFile(_)) {
                let mut c = Canvas {
                    buf: &mut self.pixels,
                    w: width,
                    h: height,
                    scale,
                };
                c.fill_rounded_rect(
                    22.0,
                    y0 + 11.0,
                    52.0,
                    y0 + 41.0,
                    7.0,
                    crate::renderer::category_rgb(item.category),
                );
            }
        }
    }

    fn load_cached_icon(&mut self, hint: &str, sz: i32) -> Option<&[u8]> {
        let key = format!("{sz}:{hint}");
        if !self.icon_cache.contains(&key) {
            let mut resolved = hint.starts_with('/').then(|| PathBuf::from(hint));
            if resolved.is_none()
                && let Ok(cs) = CString::new(hint)
            {
                let mut pbuf = [0u8; 512];
                let ok = unsafe {
                    tomoyo_linux_resolve_icon_path(cs.as_ptr(), sz, pbuf.as_mut_ptr().cast(), pbuf.len()) != 0
                };
                if ok {
                    let end = pbuf.iter().position(|&b| b == 0).unwrap_or(pbuf.len());
                    resolved = Some(PathBuf::from(String::from_utf8_lossy(&pbuf[..end]).into_owned()));
                }
            }
            let bgra = resolved
                .as_deref()
                .and_then(|p| tomoyo_core::wic_thumb::decode_png_file_bgra_square(p, sz as u32))
                .or_else(|| {
                    let mut buf = vec![0u8; (sz * sz * 4) as usize];
                    let ok = CString::new(hint)
                        .ok()
                        .is_some_and(|cs| unsafe {
                            tomoyo_linux_load_icon_bgra(cs.as_ptr(), sz, buf.as_mut_ptr(), buf.len()) != 0
                        });
                    ok.then_some(buf)
                });
            self.icon_cache.put(key.clone(), bgra);
        }
        self.icon_cache.get(&key).and_then(|v| v.as_deref())
    }

    fn paint_pango_pass(&mut self, cr: *mut c_void, scale: f32, lay: &FrameLayout, state: &RenderState<'_>) {
        let pal = state.theme;
        let pango_w = |w: u16| match w {
            0..=349 => "Light",
            350..=449 => "Normal",
            450..=549 => "Medium",
            550..=649 => "SemiBold",
            _ => "Bold",
        };
        let eff_family = if state.typography.font_family.is_empty() {
            self.font_family
        } else {
            state.typography.font_family.as_str()
        };
        let sub_dip = state.typography.subtitle_size_dip();
        let chip_dip = sub_dip.clamp(11.5, 13.5);
        let f_reg = format!("{eff_family} {}px", (13.0 * scale).round() as i32);
        let f_title = format!("{eff_family} {} {}px", pango_w(state.typography.title_weight), (state.typography.title_size as f32 * scale).round() as i32);
        let f_sub = format!("{eff_family} {} {}px", pango_w(state.typography.subtitle_weight), (sub_dip * scale).round() as i32);
        let f_chip = format!("{eff_family} {} {}px", pango_w(state.typography.subtitle_weight.min(500)), (chip_dip * scale).round() as i32);
        let f_badge = format!("{eff_family} Medium {}px", (10.5 * scale).round() as i32);
        let f_mono = format!("monospace {}px", (11.5 * scale).round() as i32);

        let mut header_right_x = lay.w_dip - 16.0;
        if let Some(mode) = state.mode_badge {
            let pill_w = (mode.chars().count() as f32 * 9.5 + 22.0).clamp(58.0, 130.0);
            let px0 = header_right_x - pill_w;
            draw_pango(cr, &f_badge, pal.chip_active_text, 1, true, mode, px0, 19.0, pill_w, 24.0, scale, &[], pal.accent);
            header_right_x = px0 - 8.0;
        }
        if let Some(cap) = state.attached_capsule {
            let cap_w = (cap.chars().count() as f32 * 8.5 + 24.0).clamp(96.0, 210.0);
            let cx0 = header_right_x - cap_w;
            draw_pango(cr, &f_chip, pal.text_primary, 1, true, cap, cx0 + 6.0, 18.0, cap_w - 12.0, 26.0, scale, &[], pal.accent);
        }

        if state.filter_bar_visible {
            let fl = compute_filter_bar_layout(
                lay.list_w_dip,
                lay.header_h,
                state.filter_tabs,
                state.active_filter,
                state.filter_counts,
                state.filter_bar_wrap,
            );
            for hb in &fl.visible {
                let col = if hb.filter == state.active_filter {
                    pal.chip_active_text
                } else if state.hovered_filter == Some(hb.filter) {
                    pal.chip_hover_text
                } else {
                    pal.chip_text
                };
                draw_pango(cr, &f_chip, col, 1, true, &hb.label, hb.x0, hb.y0, hb.x1 - hb.x0, hb.y1 - hb.y0, scale, &[], pal.accent);
            }
            if let Some(ov) = &fl.overflow_pill {
                let col = if fl.active_in_overflow {
                    pal.chip_active_text
                } else if state.hovered_overflow {
                    pal.chip_hover_text
                } else {
                    pal.text_secondary
                };
                draw_pango(cr, &f_chip, col, 1, true, &ov.label, ov.x0, ov.y0, ov.x1 - ov.x0, ov.y1 - ov.y0, scale, &[], pal.accent);
            }
        }

        let icon_sz = (30.0 * scale).round() as i32;
        for slot in 0..lay.visible_count {
            let idx = state.scroll_offset + slot;
            let Some(item) = state.items.get(idx) else { break };
            let y0 = lay.list_top + (slot as f32) * lay.row_h;
            let is_sel = idx == state.selected;

            let mut drew_icon = false;
            if let Some(hint) = extract_icon_hint(item)
                && let Some(bgra) = self.load_cached_icon(hint, icon_sz)
            {
                unsafe {
                    tomoyo_cairo_blit_bgra(cr, bgra.as_ptr(), icon_sz, icon_sz, (22.0 * scale) as f64, ((y0 + 11.0) * scale) as f64, icon_sz as f64, icon_sz as f64);
                }
                drew_icon = true;
            }
            if !drew_icon {
                let lbl = fallback_badge_label(item);
                draw_pango(cr, &f_badge, Rgb(0xFF, 0xFF, 0xFF), 1, true, lbl, 22.0, y0 + 11.0, 30.0, 30.0, scale, &[], pal.accent);
            }

            let mut text_right = lay.list_w_dip - 20.0;
            if let Some((_, done)) = state.launching_row.filter(|(li, _)| *li == idx) {
                let lbl = if done { state.lang.pick("✓ 已启动", "✓ Opened") } else { state.lang.pick("启动中…", "Opening…") };
                draw_pango(cr, &f_badge, Rgb(0xFF, 0xFF, 0xFF), 1, true, lbl, text_right - 74.0, y0 + 14.5, 74.0, 23.0, scale, &[], pal.accent);
                text_right -= 82.0;
            } else {
                let shortcut = if is_sel { "Enter".into() } else if slot < 8 { format!("Alt+{}", slot + 1) } else { String::new() };
                if !shortcut.is_empty() {
                    let col = if is_sel { pal.chip_active_text } else { pal.text_muted };
                    draw_pango(cr, &f_sub, col, 2, true, &shortcut, text_right - 44.0, y0 + 8.0, 44.0, lay.row_h - 18.0, scale, &[], pal.accent);
                    text_right -= 50.0;
                }
            }
            if let Some(badge) = &item.badge {
                let bw = (badge.chars().count() as f32 * 6.8 + 18.0).clamp(44.0, 120.0);
                draw_pango(cr, &f_badge, pal.text_secondary, 1, true, badge, text_right - bw, y0 + 15.5, bw, 21.0, scale, &[], pal.accent);
                text_right -= bw + 10.0;
            }
            let hl_chars = highlight_title_ranges(state.query, &item.title);
            let hl_bytes = char_spans_to_byte_pairs(&item.title, &hl_chars);
            draw_pango(cr, &f_title, pal.text_primary, 0, true, &item.title, 62.0, y0 + 5.0, (text_right - 62.0).max(20.0), 23.0, scale, &hl_bytes, pal.accent);
            let clean_sub = strip_icon_tag(&item.subtitle);
            draw_pango(cr, &f_sub, pal.text_secondary, 0, true, clean_sub, 62.0, y0 + 28.0, (text_right - 62.0).max(20.0), 19.0, scale, &[], pal.accent);
        }

        if lay.has_preview && let Some(prev) = state.preview {
            let (px0, px1) = (lay.list_w_dip + 14.0, lay.w_dip - 14.0);
            draw_pango(cr, &f_title, pal.text_primary, 0, true, &prev.title, px0, lay.header_h + 10.0, px1 - px0, 22.0, scale, &[], pal.accent);
            draw_pango(cr, &f_sub, pal.text_secondary, 0, true, &prev.meta_line, px0, lay.header_h + 32.0, px1 - px0, 18.0, scale, &[], pal.accent);
            let body_top = if let Some((iw, ih, _)) = &prev.image_rgba {
                let aw = (px1 - px0).max(40.0);
                let ah = (lay.fy0 - (lay.header_h + 60.0) - 52.0).clamp(60.0, 210.0);
                lay.header_h + 66.0 + (*ih as f32 * (aw / *iw as f32).min(ah / *ih as f32).min(1.0)).max(16.0)
            } else {
                lay.header_h + 58.0
            };
            let bfont = if prev.is_monospace { &f_mono } else { &f_reg };
            draw_pango(cr, bfont, pal.text_primary, 0, false, &prev.body, px0, body_top, (px1 - px0).max(20.0), (lay.fy0 - 10.0 - body_top).max(20.0), scale, &[], pal.accent);
        }

        if state.action_menu_open && let Some(sel) = state.items.get(state.selected) {
            let n = sel.actions.len().min(6);
            let (mx1, my1) = (lay.list_w_dip - 14.0, lay.fy0 - 10.0);
            let (mx0, my0) = (mx1 - 240.0, (my1 - (n as f32 * 30.0 + 16.0)).max(lay.list_top + 4.0));
            for (ai, act) in sel.actions.iter().take(n).enumerate() {
                let col = if ai == state.selected_action { pal.chip_active_text } else { pal.text_primary };
                draw_pango(cr, &f_chip, col, 0, true, &act.label, mx0 + 14.0, my0 + 8.0 + (ai as f32) * 30.0, 212.0, 28.0, scale, &[], pal.accent);
            }
        }

        self.paint_footer(cr, scale, lay, state, &f_sub, &f_badge);
    }

    fn paint_footer(&self, cr: *mut c_void, scale: f32, lay: &FrameLayout, state: &RenderState<'_>, f_sub: &str, f_badge: &str) {
        let pal = state.theme;
        let left_text = if let Some(toast) = state.toast {
            toast.to_string()
        } else if state.action_menu_open {
            state.lang.pick("动作菜单：↑↓ 选择 · 回车执行 · Esc 返回", "Actions: ↑↓ Select · Enter Execute · Esc Close").into()
        } else {
            let ev = state.everything_status.unwrap_or_else(|| state.lang.pick("fd/plocate 就绪", "fd/plocate ready"));
            match state.lang {
                Lang::Zh => format!("{ev} · Ctrl+Tab 分组 · 双击打开 · Ctrl+, 设置 · Ctrl+P 预览"),
                Lang::En => format!("{ev} · Ctrl+Tab Filter · DblClick Open · Ctrl+, Settings"),
            }
        };
        let left_right = if let Some(ver) = state.update_badge {
            let uw = (ver.chars().count() as f32 * 7.2 + 18.0).clamp(64.0, 120.0);
            let ux0 = lay.w_dip - 190.0 - uw;
            if ux0 > 220.0 {
                draw_pango(cr, f_badge, pal.chip_active_text, 1, true, ver, ux0, lay.fy0 + 5.0, uw, 20.0, scale, &[], pal.accent);
                ux0 - 10.0
            } else { lay.w_dip - 186.0 }
        } else { lay.w_dip - 186.0 };
        draw_pango(cr, f_sub, pal.text_secondary, 0, true, &left_text, 28.0, lay.fy0, (left_right - 28.0).max(40.0), lay.h_dip - lay.fy0, scale, &[], pal.accent);

        let stats = current_process_stats();
        let mem = match state.lang {
            Lang::Zh => format!("{} 项 · 内存 {:.1} MB", state.items.len(), stats.private_mb()),
            Lang::En => format!("{} hits · RAM {:.1} MB", state.items.len(), stats.private_mb()),
        };
        draw_pango(cr, f_sub, pal.text_muted, 2, true, &mem, lay.w_dip - 186.0, lay.fy0, 168.0, lay.h_dip - lay.fy0, scale, &[], pal.accent);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_pango(
    cr: *mut c_void, font: &str, color: Rgb, align: c_int, single: bool,
    text: &str, x: f32, y: f32, w: f32, h: f32, scale: f32, hl: &[u32], hl_rgb: Rgb,
) {
    if text.is_empty() || w <= 2.0 || h <= 2.0 {
        return;
    }
    let Ok(cf) = CString::new(font) else { return };
    let Ok(ct) = CString::new(text) else { return };
    unsafe {
        tomoyo_pango_draw_text(
            cr, cf.as_ptr(), rgb_u32(color), align, c_int::from(single), ct.as_ptr(),
            (x * scale) as f64, (y * scale) as f64, (w * scale) as f64, (h * scale) as f64,
            if hl.is_empty() { std::ptr::null() } else { hl.as_ptr() }, hl.len() / 2, rgb_u32(hl_rgb),
        );
    }
}

fn extract_icon_hint(item: &ResultItem) -> Option<&str> {
    if let IconSpec::ImageFile(p) = &item.icon {
        return Some(p.as_str());
    }
    if let IconSpec::ShellPath(p) = &item.icon
        && p.ends_with(".desktop")
        && let Some(start) = item.subtitle.rfind('[')
        && item.subtitle.ends_with(']')
    {
        return Some(&item.subtitle[start + 1..item.subtitle.len() - 1]);
    }
    None
}

fn strip_icon_tag(sub: &str) -> &str {
    if let Some(i) = sub.rfind(" [") && sub.ends_with(']') {
        return &sub[..i];
    }
    sub
}

fn char_spans_to_byte_pairs(s: &str, spans: &[(usize, usize)]) -> Vec<u32> {
    if spans.is_empty() { return Vec::new(); }
    let idx: Vec<usize> = s.char_indices().map(|(b, _)| b).chain(std::iter::once(s.len())).collect();
    let mut out = Vec::with_capacity(spans.len() * 2);
    for &(cs, ce) in spans {
        if let (Some(&bs), Some(&be)) = (idx.get(cs), idx.get(ce)) && bs < be {
            out.push(bs as u32);
            out.push(be as u32);
        }
    }
    out
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
