//! Theme-aware ClearType GDI sticky-card & CSS color-swatch renderer for pinned clipboard text.

#![cfg(windows)]

use crate::pin_clipboard::PinPayload;
use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GdiFlush,
    SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DT_CALCRECT, DT_EXPANDTABS, DT_LEFT, DT_NOPREFIX, DT_WORDBREAK, FF_DONTCARE, FW_NORMAL,
    FW_SEMIBOLD, HBITMAP, HDC, HFONT, OUT_DEFAULT_PRECIS, TRANSPARENT,
};

#[derive(Clone, Copy)]
struct CardPalette {
    bg: (u8, u8, u8),
    fg: (u8, u8, u8),
    muted: (u8, u8, u8),
    accent: (u8, u8, u8),
    border: (u8, u8, u8),
}

impl CardPalette {
    fn from_theme(theme: &str) -> Self {
        match theme.to_ascii_lowercase().as_str() {
            "dark" => Self {
                bg: (0x1B, 0x1D, 0x24),
                fg: (0xF4, 0xF4, 0xF5),
                muted: (0x9C, 0xA3, 0xAF),
                accent: (0x60, 0xA5, 0xFA),
                border: (0x33, 0x38, 0x46),
            },
            "midnight" => Self {
                bg: (0x11, 0x16, 0x26),
                fg: (0xF1, 0xF5, 0xF9),
                muted: (0x94, 0xA3, 0xB8),
                accent: (0x81, 0x8C, 0xF8),
                border: (0x29, 0x35, 0x4F),
            },
            _ => Self {
                bg: (0xFF, 0xFF, 0xFF),
                fg: (0x18, 0x18, 0x1B),
                muted: (0x71, 0x71, 0x7A),
                accent: (0x25, 0x63, 0xEB),
                border: (0xE4, 0xE4, 0xE7),
            },
        }
    }
}

pub fn parse_color_literal(raw: &str) -> Option<(u8, u8, u8)> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some((r, g, b));
        }
        if hex.len() == 3 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let d = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17);
            return Some((d(0)?, d(1)?, d(2)?));
        }
    }
    let low = s.to_ascii_lowercase();
    if let Some(inner) = low.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
        let nums: Vec<u8> = inner
            .split(',')
            .filter_map(|p| p.trim().parse::<u8>().ok())
            .collect();
        if nums.len() == 3 {
            return Some((nums[0], nums[1], nums[2]));
        }
    }
    None
}

pub fn render_text_or_color_pin_card(text: &str, theme: &str, dpi_scale: f32) -> Option<PinPayload> {
    let scale = dpi_scale.clamp(1.0, 3.0);
    let clean = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return None;
    }
    let pal = CardPalette::from_theme(theme);

    if let Some(rgb) = parse_color_literal(trimmed) {
        return render_color_card(trimmed, rgb, pal, scale);
    }

    let truncated: String = trimmed.chars().take(2400).collect();
    let display = if trimmed.chars().count() > 2400 {
        format!("{truncated}\n…（已截断显示，Ctrl+C 仍复制完整原文）")
    } else {
        truncated
    };
    let looks_code = display.contains('{')
        || display.contains("fn ")
        || display.contains("=>")
        || display.contains("::")
        || display.contains("http://")
        || display.contains("https://");
    let face = if looks_code { "Cascadia Mono" } else { "Microsoft YaHei UI" };
    let pad_x = (16.0 * scale).round() as i32;
    let pad_top = (15.0 * scale).round() as i32;
    let foot_h = (26.0 * scale).round() as i32;
    let max_text_w = (560.0 * scale).round() as i32;
    let min_card_w = (240.0 * scale).round() as i32;
    let max_text_h = (520.0 * scale).round() as i32;
    let font_h = -((14.0 * scale).round() as i32);
    let foot_font_h = -((11.0 * scale).round() as i32);
    let wide_body: Vec<u16> = display.encode_utf16().collect();
    let line_cnt = trimmed.lines().count().max(1);
    let char_cnt = trimmed.chars().count();
    let foot_txt = format!("{line_cnt} 行 · {char_cnt} 字 · Ctrl+C 复制原文 · 双击关闭");
    let wide_foot: Vec<u16> = foot_txt.encode_utf16().collect();

    unsafe {
        let mem_dc = CreateCompatibleDC(None);
        let body_font = make_font(face, font_h, FW_NORMAL.0 as i32);
        let foot_font = make_font("Microsoft YaHei UI", foot_font_h, FW_NORMAL.0 as i32);
        let old_f = SelectObject(mem_dc, body_font.into());

        let mut calc_rc = RECT { left: 0, top: 0, right: max_text_w, bottom: 0 };
        let _ = DrawTextW(
            mem_dc,
            &mut wide_body.clone(),
            &mut calc_rc,
            DT_CALCRECT | DT_LEFT | DT_WORDBREAK | DT_NOPREFIX | DT_EXPANDTABS,
        );
        let text_w = calc_rc.right.clamp(min_card_w - pad_x * 2, max_text_w);
        let text_h = calc_rc.bottom.clamp((22.0 * scale).round() as i32, max_text_h);
        let w = (text_w + pad_x * 2).max(min_card_w);
        let h = pad_top + text_h + (12.0 * scale).round() as i32 + foot_h;

        let Some((dib, bits)) = create_dib_32(mem_dc, w, h) else {
            SelectObject(mem_dc, old_f);
            let _ = DeleteObject(body_font.into());
            let _ = DeleteObject(foot_font.into());
            let _ = DeleteDC(mem_dc);
            return None;
        };
        let old_b = SelectObject(mem_dc, dib.into());
        let pixels = std::slice::from_raw_parts_mut(bits, (w as usize) * (h as usize) * 4);
        fill_card_chrome(pixels, w, h, pal, (3.0 * scale).round() as i32, foot_h);

        let _ = SetBkMode(mem_dc, TRANSPARENT);
        let _ = SetTextColor(mem_dc, to_colorref(pal.fg));
        let mut body_rc =
            RECT { left: pad_x, top: pad_top, right: w - pad_x, bottom: pad_top + text_h };
        let _ = DrawTextW(
            mem_dc,
            &mut wide_body.clone(),
            &mut body_rc,
            DT_LEFT | DT_WORDBREAK | DT_NOPREFIX | DT_EXPANDTABS,
        );

        SelectObject(mem_dc, foot_font.into());
        let _ = SetTextColor(mem_dc, to_colorref(pal.muted));
        let mut foot_rc = RECT {
            left: pad_x,
            top: h - foot_h + (5.0 * scale).round() as i32,
            right: w - pad_x,
            bottom: h,
        };
        let _ = DrawTextW(mem_dc, &mut wide_foot.clone(), &mut foot_rc, DT_LEFT | DT_NOPREFIX);
        let _ = GdiFlush();

        for px in pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        let bgra = pixels.to_vec();

        SelectObject(mem_dc, old_b);
        SelectObject(mem_dc, old_f);
        let _ = DeleteObject(dib.into());
        let _ = DeleteObject(body_font.into());
        let _ = DeleteObject(foot_font.into());
        let _ = DeleteDC(mem_dc);

        Some(PinPayload {
            width: w as u32,
            height: h as u32,
            bgra,
            source_text: Some(text.to_string()),
        })
    }
}

fn render_color_card(
    original: &str,
    rgb: (u8, u8, u8),
    pal: CardPalette,
    scale: f32,
) -> Option<PinPayload> {
    let w = (290.0 * scale).round() as i32;
    let h = (124.0 * scale).round() as i32;
    let swatch = (72.0 * scale).round() as i32;
    let pad = (16.0 * scale).round() as i32;
    let hex_str = format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2);
    let rgb_str = format!("RGB({}, {}, {})", rgb.0, rgb.1, rgb.2);
    let hint = "Ctrl+C 复制色值 · 双击关闭";
    let card_pal = CardPalette { accent: rgb, ..pal };

    unsafe {
        let mem_dc = CreateCompatibleDC(None);
        let Some((dib, bits)) = create_dib_32(mem_dc, w, h) else {
            let _ = DeleteDC(mem_dc);
            return None;
        };
        let old_b = SelectObject(mem_dc, dib.into());
        let pixels = std::slice::from_raw_parts_mut(bits, (w as usize) * (h as usize) * 4);
        fill_card_chrome(pixels, w, h, card_pal, (4.0 * scale).round() as i32, 0);

        for y in (pad + 4)..(pad + 4 + swatch).min(h - 2) {
            for x in pad..(pad + swatch).min(w - 2) {
                let o = ((y as usize) * (w as usize) + (x as usize)) * 4;
                pixels[o] = rgb.2;
                pixels[o + 1] = rgb.1;
                pixels[o + 2] = rgb.0;
                pixels[o + 3] = 255;
            }
        }

        let f_big = make_font("Cascadia Mono", -((19.0 * scale).round() as i32), FW_SEMIBOLD.0 as i32);
        let f_sub = make_font("Cascadia Mono", -((12.5 * scale).round() as i32), FW_NORMAL.0 as i32);
        let f_hint =
            make_font("Microsoft YaHei UI", -((11.0 * scale).round() as i32), FW_NORMAL.0 as i32);
        let old_f = SelectObject(mem_dc, f_big.into());
        let _ = SetBkMode(mem_dc, TRANSPARENT);
        let tx = pad + swatch + (14.0 * scale).round() as i32;

        let _ = SetTextColor(mem_dc, to_colorref(pal.fg));
        let mut r1 = RECT { left: tx, top: pad + (8.0 * scale).round() as i32, right: w - pad, bottom: h };
        let _ = DrawTextW(
            mem_dc,
            &mut hex_str.encode_utf16().collect::<Vec<_>>(),
            &mut r1,
            DT_LEFT | DT_NOPREFIX,
        );

        SelectObject(mem_dc, f_sub.into());
        let _ = SetTextColor(mem_dc, to_colorref(pal.muted));
        let mut r2 = RECT { left: tx, top: pad + (36.0 * scale).round() as i32, right: w - pad, bottom: h };
        let _ = DrawTextW(
            mem_dc,
            &mut rgb_str.encode_utf16().collect::<Vec<_>>(),
            &mut r2,
            DT_LEFT | DT_NOPREFIX,
        );

        SelectObject(mem_dc, f_hint.into());
        let mut r3 = RECT { left: pad, top: h - (24.0 * scale).round() as i32, right: w - pad, bottom: h };
        let _ = DrawTextW(
            mem_dc,
            &mut hint.encode_utf16().collect::<Vec<_>>(),
            &mut r3,
            DT_LEFT | DT_NOPREFIX,
        );
        let _ = GdiFlush();

        for px in pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        let bgra = pixels.to_vec();

        SelectObject(mem_dc, old_b);
        SelectObject(mem_dc, old_f);
        let _ = DeleteObject(dib.into());
        let _ = DeleteObject(f_big.into());
        let _ = DeleteObject(f_sub.into());
        let _ = DeleteObject(f_hint.into());
        let _ = DeleteDC(mem_dc);

        Some(PinPayload {
            width: w as u32,
            height: h as u32,
            bgra,
            source_text: Some(original.to_string()),
        })
    }
}

fn fill_card_chrome(
    pixels: &mut [u8],
    w: i32,
    h: i32,
    pal: CardPalette,
    accent_bar_h: i32,
    foot_h: i32,
) {
    let sep_y = if foot_h > 0 { h - foot_h } else { -1 };
    for y in 0..h {
        for x in 0..w {
            let col = if y < accent_bar_h {
                pal.accent
            } else if x == 0 || x == w - 1 || y == h - 1 || y == sep_y {
                pal.border
            } else {
                pal.bg
            };
            let o = ((y as usize) * (w as usize) + (x as usize)) * 4;
            pixels[o] = col.2;
            pixels[o + 1] = col.1;
            pixels[o + 2] = col.0;
            pixels[o + 3] = 255;
        }
    }
}

fn to_colorref(rgb: (u8, u8, u8)) -> COLORREF {
    COLORREF((rgb.0 as u32) | ((rgb.1 as u32) << 8) | ((rgb.2 as u32) << 16))
}

unsafe fn make_font(face: &str, height: i32, weight: i32) -> HFONT {
    let w_face: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(w_face.as_ptr()),
        )
    }
}

unsafe fn create_dib_32(mem_dc: HDC, w: i32, h: i32) -> Option<(HBITMAP, *mut u8)> {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let dib =
        unsafe { CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()? };
    if bits.is_null() {
        let _ = unsafe { DeleteObject(dib.into()) };
        return None;
    }
    Some((dib, bits as *mut u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_rgb_color_literals() {
        assert_eq!(parse_color_literal("#2563eb"), Some((0x25, 0x63, 0xEB)));
        assert_eq!(parse_color_literal("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_color_literal("rgb(16, 185, 129)"), Some((16, 185, 129)));
        assert_eq!(parse_color_literal("hello world"), None);
    }

    #[test]
    fn renders_text_and_color_pin_cards_cleanly() {
        let _gui_guard = crate::PROCESS_GUI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let txt = render_text_or_color_pin_card("Tomoyo F3 贴图\n支持中英文与代码", "light", 1.0)
            .unwrap();
        assert!(txt.width >= 240 && txt.height >= 60);
        assert_eq!(txt.bgra.len(), (txt.width as usize) * (txt.height as usize) * 4);
        assert_eq!(txt.source_text.as_deref(), Some("Tomoyo F3 贴图\n支持中英文与代码"));

        let col = render_text_or_color_pin_card("#2563EB", "dark", 1.0).unwrap();
        assert_eq!(col.width, 290);
        assert_eq!(col.height, 124);

        // Verify strictly 0 GDI/USER handle drift over 10 repeated renders after one-time OS GDI init.
        let before = tomoyo_core::current_process_stats();
        for _ in 0..10 {
            let _ = render_text_or_color_pin_card("反复渲染零泄露验证\nfn main() {}", "midnight", 1.25).unwrap();
            let _ = render_text_or_color_pin_card("rgb(37, 99, 235)", "light", 1.25).unwrap();
        }
        let after = tomoyo_core::current_process_stats();
        assert_eq!(after.gdi_objects, before.gdi_objects, "GDI leak across repeated pin cards");
        assert_eq!(after.user_objects, before.user_objects, "USER leak across repeated pin cards");
    }
}
