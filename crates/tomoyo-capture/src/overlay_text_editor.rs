//! Zero-flicker transparent direct-canvas inline text editor for `tomoyo-capture`.
//!
//! Uses a `0x0`-region-clipped subclassed multiline Win32 `EDIT` proxy solely for native
//! Windows keyboard/Chinese IME input (positioning the IME candidate popup at the live caret),
//! while rendering the transparent dashed frame, move grip, selection highlight, 2x supersampled
//! text, and blinking caret directly inside the parent's double-buffered BGRA canvas.

#![cfg(windows)]

use crate::annotate::{text_line_height, AnnotColor, Annotation};
use crate::annotate_render::draw_dashed_frame;
use crate::overlay_icons::{fill_box_alpha, fill_circle_aa, fill_rounded_rect_aa};
use crate::overlay_text::{
    blit_text_mask_bgra, measure_line_prefix_px, measure_multiline_px, TextMaskCache,
};
use crate::screen::ScreenRect;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateRectRgn, DeleteObject, EndPaint, SetWindowRgn, PAINTSTRUCT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_CONTROL, VK_ESCAPE, VK_RETURN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, CreateWindowExW, DestroyWindow, GetParent, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowTextW, MoveWindow, PostMessageW, SendMessageW,
    SetWindowLongPtrW, SetWindowTextW, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE,
    ES_WANTRETURN, GWLP_USERDATA, GWLP_WNDPROC, HTTRANSPARENT, WINDOW_STYLE, WM_APP, WM_CHAR,
    WM_CLEAR, WM_CONTEXTMENU, WM_CUT, WM_ERASEBKGND, WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION,
    WM_IME_STARTCOMPOSITION, WM_KEYDOWN, WM_KEYUP, WM_MOUSEWHEEL, WM_NCHITTEST, WM_NCPAINT,
    WM_PAINT, WM_PASTE, WM_PRINTCLIENT, WM_UNDO, WNDPROC, WS_CHILD, WS_VISIBLE,
};

pub const WM_APP_TEXT_SYNC: u32 = WM_APP + 51;
pub const WM_APP_TEXT_COMMIT: u32 = WM_APP + 52;
pub const WM_APP_TEXT_CANCEL: u32 = WM_APP + 53;
pub const TIMER_TEXT_CARET: usize = 602;
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;

static IME_COMPOSING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextHitZone {
    Outside,
    FrameDrag,
    Body,
}

pub struct InlineTextEditor {
    pub hwnd: HWND,
    pub x: i32,
    pub y: i32,
    pub size: i32,
    pub color: AnnotColor,
    pub bold: bool,
    pub text: String,
    pub raw_utf16: Vec<u16>,
    pub sel_start: usize,
    pub sel_end: usize,
    pub caret_visible: bool,
    pub editing_original: Option<Annotation>,
    cached_box: ScreenRect,
}

impl InlineTextEditor {
    pub fn spawn(
        parent: HWND, x: i32, y: i32, size: i32, color: AnnotColor, bold: bool, font_face: &str,
    ) -> Option<Self> {
        Self::spawn_inner(parent, x, y, size, color, bold, "", None, font_face)
    }

    pub fn spawn_for_reedit(parent: HWND, orig: Annotation, font_face: &str) -> Option<Self> {
        let Annotation::Text { x, y, ref text, size, color, bold } = orig else { return None };
        let init_text = text.clone();
        Self::spawn_inner(parent, x, y, size, color, bold, &init_text, Some(orig), font_face)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_inner(
        parent: HWND, x: i32, y: i32, size: i32, color: AnnotColor, bold: bool,
        initial_text: &str, editing_original: Option<Annotation>, font_face: &str,
    ) -> Option<Self> {
        IME_COMPOSING.store(false, Ordering::SeqCst);
        let lh = text_line_height(size);
        let style = WS_CHILD | WS_VISIBLE
            | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_AUTOHSCROLL | ES_WANTRETURN) as u32);
        let h = unsafe {
            CreateWindowExW(Default::default(), w!("EDIT"), w!(""), style, x, y, 2, lh, Some(parent), None, None, None).ok()?
        };
        unsafe {
            // Clip 100% of native EDIT GDI output to an empty region so zero white pixels or
            // flicker can ever appear, while keeping full native keyboard/IME focus & tracking.
            let rgn = CreateRectRgn(0, 0, 0, 0);
            if !rgn.is_invalid() && SetWindowRgn(h, Some(rgn), false) == 0 {
                let _ = DeleteObject(rgn.into());
            }
            let orig_proc = SetWindowLongPtrW(h, GWLP_WNDPROC, proxy_edit_wndproc as *const () as usize as isize);
            SetWindowLongPtrW(h, GWLP_USERDATA, orig_proc);
            if !initial_text.is_empty() {
                let crlf = initial_text.replace("\r\n", "\n").replace('\n', "\r\n");
                let wide: Vec<u16> = crlf.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = SetWindowTextW(h, PCWSTR(wide.as_ptr()));
                let end = wide.len().saturating_sub(1);
                SendMessageW(h, EM_SETSEL, Some(WPARAM(end)), Some(LPARAM(end as isize)));
            }
            crate::overlay_ime::enable_ime_for_window(h);
            let _ = SetFocus(Some(h));
        }
        let mut ed = Self {
            hwnd: h, x, y, size: size.clamp(12, 64), color, bold,
            text: String::new(), raw_utf16: Vec::new(), sel_start: 0, sel_end: 0,
            caret_visible: true, editing_original,
            cached_box: ScreenRect { x: x - 10, y: y - 7, w: 98, h: lh + 14 },
        };
        ed.sync_from_hwnd(font_face);
        Some(ed)
    }

    pub fn sync_from_hwnd(&mut self, font_face: &str) {
        let (raw, sel_s, sel_e) = unsafe {
            let len = GetWindowTextLengthW(self.hwnd).max(0) as usize;
            let mut buf = vec![0u16; len + 1];
            let n = if len > 0 { GetWindowTextW(self.hwnd, &mut buf) as usize } else { 0 };
            buf.truncate(n);
            let (mut s, mut e) = (0u32, 0u32);
            SendMessageW(self.hwnd, EM_GETSEL, Some(WPARAM(&mut s as *mut _ as usize)), Some(LPARAM(&mut e as *mut _ as isize)));
            (buf, (s as usize).min(n), (e as usize).min(n))
        };
        self.raw_utf16 = raw;
        self.text = String::from_utf16_lossy(&self.raw_utf16).replace("\r\n", "\n");
        self.sel_start = sel_s;
        self.sel_end = sel_e;
        self.caret_visible = true;
        self.update_layout_and_ime_anchor(font_face);
    }

    pub fn update_layout_and_ime_anchor(&mut self, font_face: &str) {
        let lh = text_line_height(self.size);
        let (tw, th) = measure_multiline_px(&self.text, self.size, self.bold, font_face);
        self.cached_box = ScreenRect { x: self.x - 10, y: self.y - 7, w: tw.max(76) + 22, h: th.max(lh) + 14 };
        let (cx, cy) = self.caret_pixel_pos(font_face);
        unsafe { let _ = MoveWindow(self.hwnd, cx, cy, 2, lh, false); }
    }

    fn grip_rect(&self) -> ScreenRect {
        let f = self.cached_box;
        ScreenRect { x: f.x + 8, y: f.y - 6, w: 32, h: 11 }
    }

    pub fn hit_test(&self, px: i32, py: i32) -> TextHitZone {
        let g = self.grip_rect();
        if (ScreenRect { x: g.x - 4, y: g.y - 4, w: g.w + 8, h: g.h + 8 }).contains(px, py) {
            return TextHitZone::FrameDrag;
        }
        let f = self.cached_box;
        if !(ScreenRect { x: f.x - 5, y: f.y - 5, w: f.w + 10, h: f.h + 10 }).contains(px, py) {
            return TextHitZone::Outside;
        }
        let inner = ScreenRect { x: f.x + 6, y: f.y + 5, w: (f.w - 12).max(0), h: (f.h - 10).max(0) };
        if inner.contains(px, py) { TextHitZone::Body } else { TextHitZone::FrameDrag }
    }

    pub fn place_caret_at(&mut self, px: i32, py: i32, font_face: &str) {
        let lh = text_line_height(self.size);
        let lines: Vec<&str> = self.text.split('\n').collect();
        let target_line = ((py - self.y).max(0) / lh).min(lines.len().saturating_sub(1) as i32) as usize;
        let line = lines.get(target_line).copied().unwrap_or("");
        let (rel_x, char_count) = ((px - self.x).max(0), line.chars().count());
        let (mut best_col, mut prev_w) = (char_count, 0i32);
        for col in 1..=char_count {
            let w = measure_line_prefix_px(line, col, self.size, self.bold, font_face);
            if rel_x <= (prev_w + w) / 2 { best_col = col - 1; break; }
            prev_w = w;
        }
        let u16_pos = line_col_to_utf16_offset(&self.raw_utf16, target_line, best_col);
        unsafe {
            let _ = SetFocus(Some(self.hwnd));
            SendMessageW(self.hwnd, EM_SETSEL, Some(WPARAM(u16_pos)), Some(LPARAM(u16_pos as isize)));
        }
        self.sync_from_hwnd(font_face);
    }

    fn caret_pixel_pos(&self, font_face: &str) -> (i32, i32) {
        let lh = text_line_height(self.size);
        let (line_idx, char_col) = utf16_offset_to_line_col(&self.raw_utf16, self.sel_end);
        let line = self.text.split('\n').nth(line_idx).unwrap_or("");
        let dx = measure_line_prefix_px(line, char_col, self.size, self.bold, font_face);
        (self.x + dx, self.y + (line_idx as i32) * lh)
    }

    pub fn paint_into_bgra(
        &self,
        bgra: &mut [u8],
        w: i32,
        h: i32,
        font_face: &str,
        cache: &mut TextMaskCache,
    ) {
        let f = self.cached_box;
        draw_dashed_frame(bgra, w, h, f, AnnotColor(0x3B, 0x82, 0xF6), 0.95);
        // Subtle top-left move grip pill signaling immediate drag-while-editing
        let g = self.grip_rect();
        fill_rounded_rect_aa(bgra, w, h, g, 5.0, AnnotColor(0x25, 0x63, 0xEB), 0.94);
        let cy = g.y as f32 + g.h as f32 * 0.5;
        for dx in [-6.0f32, 0.0, 6.0] {
            fill_circle_aa(bgra, w, h, g.x as f32 + g.w as f32 * 0.5 + dx, cy, 1.35, AnnotColor(0xFF, 0xFF, 0xFF), 0.95);
        }

        let lh = text_line_height(self.size);
        if self.sel_start != self.sel_end {
            let (s_idx, e_idx) = (self.sel_start.min(self.sel_end), self.sel_start.max(self.sel_end));
            let (l0, c0) = utf16_offset_to_line_col(&self.raw_utf16, s_idx);
            let (l1, c1) = utf16_offset_to_line_col(&self.raw_utf16, e_idx);
            for (li, line) in self.text.split('\n').enumerate() {
                if li < l0 || li > l1 { continue; }
                let sc = if li == l0 { c0 } else { 0 };
                let ec = if li == l1 { c1 } else { line.chars().count() };
                let x0 = self.x + measure_line_prefix_px(line, sc, self.size, self.bold, font_face);
                let x1 = self.x + measure_line_prefix_px(line, ec, self.size, self.bold, font_face);
                let rw = (x1 - x0).max(if li < l1 { 8 } else { 0 });
                if rw > 0 {
                    fill_box_alpha(
                        bgra, w, h,
                        ScreenRect { x: x0, y: self.y + (li as i32) * lh, w: rw, h: lh - 2 },
                        AnnotColor(0x3B, 0x82, 0xF6), 92,
                    );
                }
            }
        }

        if !self.text.is_empty()
            && let Some(mask) = cache.get_or_rasterize(&self.text, self.size, self.bold, font_face)
        {
            blit_text_mask_bgra(bgra, w, h, self.x - 1, self.y - 1, mask, self.color);
        }

        if self.caret_visible && self.sel_start == self.sel_end {
            let (cx, cy) = self.caret_pixel_pos(font_face);
            let caret_h = (lh - 3).max(self.size);
            fill_box_alpha(bgra, w, h, ScreenRect { x: cx + 1, y: cy + 2, w: 2, h: caret_h }, AnnotColor(0x0B, 0x0F, 0x19), 115);
            fill_box_alpha(bgra, w, h, ScreenRect { x: cx, y: cy + 1, w: 2, h: caret_h }, self.color, 250);
        }
    }

    pub fn commit_and_close(self) -> Option<Annotation> {
        let final_text = self.text.trim_end().to_string();
        let (x, y, size, color, bold) = (self.x, self.y, self.size, self.color, self.bold);
        self.destroy_hwnd();
        if final_text.trim().is_empty() {
            None
        } else {
            Some(Annotation::Text { x, y, text: final_text, size, color, bold })
        }
    }

    pub fn cancel_and_close(mut self) -> Option<Annotation> {
        let restore = self.editing_original.take();
        self.destroy_hwnd();
        restore
    }

    fn destroy_hwnd(self) {
        IME_COMPOSING.store(false, Ordering::SeqCst);
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn proxy_edit_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_NCPAINT | WM_PRINTCLIENT | WM_CONTEXTMENU => return LRESULT(0),
        WM_NCHITTEST => return LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEWHEEL => {
            if let Ok(parent) = unsafe { GetParent(hwnd) }
                && !parent.is_invalid()
            {
                unsafe { let _ = PostMessageW(Some(parent), WM_MOUSEWHEEL, wparam, lparam); }
            }
            return LRESULT(0);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = unsafe { BeginPaint(hwnd, &mut ps) };
            let _ = unsafe { EndPaint(hwnd, &ps) };
            return LRESULT(0);
        }
        WM_IME_STARTCOMPOSITION => IME_COMPOSING.store(true, Ordering::SeqCst),
        WM_IME_ENDCOMPOSITION => {
            IME_COMPOSING.store(false, Ordering::SeqCst);
            notify_parent(hwnd, WM_APP_TEXT_SYNC);
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            let ctrl = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
            let shift = unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
            if ctrl && vk == b'A' as u16 {
                unsafe {
                    SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                }
                notify_parent(hwnd, WM_APP_TEXT_SYNC);
                return LRESULT(0);
            }
            if !IME_COMPOSING.load(Ordering::SeqCst) {
                if vk == VK_RETURN.0 && !shift {
                    notify_parent(hwnd, WM_APP_TEXT_COMMIT);
                    return LRESULT(0);
                }
                if vk == VK_ESCAPE.0 {
                    notify_parent(hwnd, WM_APP_TEXT_CANCEL);
                    return LRESULT(0);
                }
            }
        }
        _ => {}
    }

    let orig = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    let res = if orig != 0 {
        let f: WNDPROC = unsafe { std::mem::transmute(orig) };
        unsafe { CallWindowProcW(f, hwnd, msg, wparam, lparam) }
    } else {
        LRESULT(0)
    };
    if matches!(
        msg,
        WM_KEYDOWN | WM_KEYUP | WM_CHAR | WM_PASTE | WM_CUT | WM_CLEAR | WM_UNDO | WM_IME_COMPOSITION
    ) {
        notify_parent(hwnd, WM_APP_TEXT_SYNC);
    }
    res
}

fn notify_parent(hwnd: HWND, msg: u32) {
    if let Ok(parent) = unsafe { GetParent(hwnd) } && !parent.is_invalid() {
        unsafe { let _ = PostMessageW(Some(parent), msg, WPARAM(0), LPARAM(0)); }
    }
}

#[inline]
fn next_utf16_step(raw: &[u16], i: usize, limit: usize) -> (bool, usize) {
    let u = raw[i];
    if u == b'\r' as u16 {
        (true, if i + 1 < raw.len() && raw[i + 1] == b'\n' as u16 { 2 } else { 1 })
    } else if u == b'\n' as u16 {
        (true, 1)
    } else {
        (false, if (0xD800..=0xDBFF).contains(&u) && i + 1 < limit { 2 } else { 1 })
    }
}

pub(crate) fn utf16_offset_to_line_col(raw: &[u16], u16_pos: usize) -> (usize, usize) {
    let (limit, mut line, mut col, mut i) = (u16_pos.min(raw.len()), 0, 0, 0);
    while i < limit {
        let (is_nl, step) = next_utf16_step(raw, i, limit);
        if is_nl { line += 1; col = 0; } else { col += 1; }
        i += step;
    }
    (line, col)
}

pub(crate) fn line_col_to_utf16_offset(raw: &[u16], target_line: usize, target_col: usize) -> usize {
    let (mut line, mut col, mut i) = (0, 0, 0);
    while i < raw.len() {
        if line == target_line && col == target_col { return i; }
        let (is_nl, step) = next_utf16_step(raw, i, raw.len());
        if is_nl {
            if line == target_line { return i; }
            line += 1; col = 0;
        } else {
            col += 1;
        }
        i += step;
    }
    raw.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_line_col_roundtrips_cjk_multiline_and_emoji() {
        let sample: Vec<u16> = "你好Hi\r\n🚀测试".encode_utf16().collect();
        // Start of line 0
        assert_eq!(utf16_offset_to_line_col(&sample, 0), (0, 0));
        // After "你好" (2 UTF-16 units)
        assert_eq!(utf16_offset_to_line_col(&sample, 2), (0, 2));
        // Start of line 1 after "\r\n" (4 + 2 = 6)
        assert_eq!(utf16_offset_to_line_col(&sample, 6), (1, 0));
        // After 🚀 (surrogate pair = 2 UTF-16 units -> offset 8 -> col 1)
        assert_eq!(utf16_offset_to_line_col(&sample, 8), (1, 1));
        assert_eq!(line_col_to_utf16_offset(&sample, 1, 1), 8);
        assert_eq!(line_col_to_utf16_offset(&sample, 1, 3), sample.len());
    }
}
