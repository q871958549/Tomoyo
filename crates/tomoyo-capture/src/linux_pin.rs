//! Native Linux Pin-to-Desktop window & PangoCairo clipboard sticky/color card rasterizer.

use crate::export::{
    copy_bgra_to_clipboard, copy_text_to_clipboard, default_save_path, handoff_text_to_translator,
    handoff_to_ocr_or_translate, load_png_as_bgra, read_clipboard_image_bgra, save_bgra_as_png,
};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use tomoyo_core::TomoyoConfig;

unsafe extern "C" {
    fn tomoyo_cap_free(p: *mut c_void);
    fn tomoyo_cap_clip_get_text() -> *mut c_char;
    fn tomoyo_cap_clip_get_uris() -> *mut c_char;
    fn tomoyo_cap_render_text_card(
        text: *const c_char,
        meta: *const c_char,
        swatch_rgb: c_int,
        bg: u32,
        fg: u32,
        muted: u32,
        border: u32,
        scale: f64,
        ow: *mut c_int,
        oh: *mut c_int,
        olen: *mut usize,
    ) -> *mut u8;
    fn tomoyo_cap_win_run(
        w: c_int,
        h: c_int,
        fullscreen: c_int,
        x: c_int,
        y: c_int,
        ctx: *mut c_void,
        on_draw: unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, c_int),
        on_key: unsafe extern "C" fn(*mut c_void, u32, u32) -> c_int,
        on_mouse: unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int, c_int) -> c_int,
    ) -> *mut c_void;
    fn tomoyo_cap_win_resize_top(w: c_int, h: c_int);
    fn tomoyo_cap_win_redraw();
    fn tomoyo_cap_cairo_blit_bgra(cr: *mut c_void, bgra: *const u8, w: c_int, h: c_int, dw: c_int, dh: c_int);
}

pub struct PinPayload {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub source_text: Option<String>,
}

pub fn parse_color_literal(raw: &str) -> Option<(u8, u8, u8)> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some((
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
            ));
        }
        if hex.len() == 3 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let d = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17);
            return Some((d(0)?, d(1)?, d(2)?));
        }
    }
    let low = s.to_ascii_lowercase();
    if let Some(inner) = low.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
        let nums: Vec<u8> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if nums.len() == 3 {
            return Some((nums[0], nums[1], nums[2]));
        }
    }
    None
}

pub fn render_text_or_color_pin_card(text: &str, theme: &str, dpi_scale: f32) -> Option<PinPayload> {
    let clean = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (bg, fg, muted, border) = match theme.to_ascii_lowercase().as_str() {
        "dark" => (0x1B1D24u32, 0xF4F4F5u32, 0x9CA3AFu32, 0x333846u32),
        "midnight" => (0x111626u32, 0xF1F5F9u32, 0x94A3B8u32, 0x29354Fu32),
        _ => (0xFFFFFFu32, 0x18181Bu32, 0x71717Au32, 0xE4E4E7u32),
    };
    let scale = dpi_scale.clamp(1.0, 3.0) as f64;
    let swatch_rgb = parse_color_literal(trimmed)
        .map(|(r, g, b)| ((r as i32) << 16) | ((g as i32) << 8) | (b as i32))
        .unwrap_or(-1);
    let meta = if let Some((r, g, b)) = parse_color_literal(trimmed) {
        format!("rgb({r}, {g}, {b})")
    } else {
        format!("{} 字 · Ctrl+C 复制 · 双击关闭", trimmed.chars().count())
    };
    let c_text = CString::new(trimmed).ok()?;
    let c_meta = CString::new(meta).ok()?;
    let (mut ow, mut oh, mut olen) = (0i32, 0i32, 0usize);
    let ptr = unsafe {
        tomoyo_cap_render_text_card(
            c_text.as_ptr(),
            c_meta.as_ptr(),
            swatch_rgb,
            bg,
            fg,
            muted,
            border,
            scale,
            &mut ow,
            &mut oh,
            &mut olen,
        )
    };
    if ptr.is_null() || ow <= 0 || oh <= 0 || olen == 0 {
        return None;
    }
    let bgra = unsafe { std::slice::from_raw_parts(ptr, olen) }.to_vec();
    unsafe { tomoyo_cap_free(ptr.cast()); }
    Some(PinPayload {
        width: ow as u32,
        height: oh as u32,
        bgra,
        source_text: Some(clean),
    })
}

pub fn read_latest_clipboard_for_pin(theme: &str) -> Option<PinPayload> {
    if let Some((w, h, bgra)) = read_clipboard_image_bgra() {
        return Some(PinPayload { width: w, height: h, bgra, source_text: None });
    }
    let uris_raw = unsafe { tomoyo_cap_clip_get_uris() };
    if !uris_raw.is_null() {
        let s = unsafe { CStr::from_ptr(uris_raw) }.to_string_lossy().into_owned();
        unsafe { tomoyo_cap_free(uris_raw.cast()); }
        let paths: Vec<PathBuf> = s
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| PathBuf::from(l.strip_prefix("file://").unwrap_or(l)))
            .collect();
        for p in &paths {
            if matches!(
                p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
                Some("png" | "jpg" | "jpeg" | "bmp" | "webp")
            ) && Path::new(p).is_file()
                && let Ok((w, h, bgra)) = load_png_as_bgra(p)
            {
                return Some(PinPayload { width: w, height: h, bgra, source_text: None });
            }
        }
        let joined = paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n");
        if !joined.trim().is_empty() {
            return render_text_or_color_pin_card(&joined, theme, 1.0);
        }
    }
    let txt_raw = unsafe { tomoyo_cap_clip_get_text() };
    if !txt_raw.is_null() {
        let s = unsafe { CStr::from_ptr(txt_raw) }.to_string_lossy().into_owned();
        unsafe { tomoyo_cap_free(txt_raw.cast()); }
        return render_text_or_color_pin_card(&s, theme, 1.0);
    }
    None
}

struct PinState {
    w: u32,
    h: u32,
    bgra: Vec<u8>,
    zoom: f32,
    save_dir: String,
    source_text: Option<String>,
}

unsafe extern "C" fn on_pin_draw(ctx: *mut c_void, cr: *mut c_void, dw: c_int, dh: c_int) {
    let st = unsafe { &*(ctx as *const PinState) };
    unsafe { tomoyo_cap_cairo_blit_bgra(cr, st.bgra.as_ptr(), st.w as i32, st.h as i32, dw, dh); }
}

unsafe extern "C" fn on_pin_key(ctx: *mut c_void, kv: u32, state: u32) -> c_int {
    let st = unsafe { &mut *(ctx as *mut PinState) };
    let ctrl = (state & (1 << 2)) != 0;
    if kv == 0xFF1B { return 1; }
    if ctrl && matches!(kv, 0x0063 | 0x0043) {
        if let Some(txt) = &st.source_text { copy_text_to_clipboard(txt); }
        else { let _ = copy_bgra_to_clipboard(st.w, st.h, &st.bgra); }
        return 0;
    }
    if ctrl && matches!(kv, 0x0073 | 0x0053) {
        let _ = save_bgra_as_png(&default_save_path(&st.save_dir), st.w, st.h, &st.bgra);
        return 0;
    }
    if matches!(kv, 0x006F | 0x004F) {
        let _ = handoff_to_ocr_or_translate(st.w, st.h, &st.bgra, false);
    } else if matches!(kv, 0x0074 | 0x0054) {
        if let Some(txt) = &st.source_text { handoff_text_to_translator(txt); }
        else { let _ = handoff_to_ocr_or_translate(st.w, st.h, &st.bgra, true); }
    } else if matches!(kv, 0x0072 | 0x0052) {
        st.zoom = 1.0;
        unsafe {
            tomoyo_cap_win_resize_top(st.w as i32, st.h as i32);
            tomoyo_cap_win_redraw();
        }
    }
    0
}

unsafe extern "C" fn on_pin_mouse(ctx: *mut c_void, kind: c_int, _x: c_int, _y: c_int, d: c_int, btn: c_int) -> c_int {
    let st = unsafe { &mut *(ctx as *mut PinState) };
    match kind {
        2 => 1,
        1 if btn == 2 || btn == 3 => 1,
        6 => {
            st.zoom = (st.zoom + if d > 0 { 0.1 } else { -0.1 }).clamp(0.25, 4.0);
            let nw = ((st.w as f32 * st.zoom).round() as i32).max(40);
            let nh = ((st.h as f32 * st.zoom).round() as i32).max(32);
            unsafe {
                tomoyo_cap_win_resize_top(nw, nh);
                tomoyo_cap_win_redraw();
            }
            0
        }
        _ => 0,
    }
}

pub fn run_pin_window(width: u32, height: u32, bgra: Vec<u8>, sx: i32, sy: i32) {
    run_pin_window_with_text(width, height, bgra, sx, sy, None);
}

pub fn run_pin_window_with_text(width: u32, height: u32, bgra: Vec<u8>, sx: i32, sy: i32, source_text: Option<String>) {
    let cfg = TomoyoConfig::load_or_create();
    let cap = cfg.capture.with_plugin_overrides(cfg.plugin_settings.get(tomoyo_core::PLUGIN_CAPTURE));
    let mut state = Box::new(PinState {
        w: width,
        h: height,
        bgra,
        zoom: 1.0,
        save_dir: cap.save_dir,
        source_text,
    });
    let ptr = &mut *state as *mut PinState as *mut c_void;
    unsafe { tomoyo_cap_win_run(width as i32, height as i32, 0, sx, sy, ptr, on_pin_draw, on_pin_key, on_pin_mouse); }
}
