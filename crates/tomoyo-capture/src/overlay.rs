//! Interactive full-virtual-desktop borderless Win32 capture & editable vector annotation overlay.

#![cfg(windows)]

use crate::annotate::{Annotation, AnnotationStack, ToolKind};
use crate::overlay_draw::{
    build_toolbar_layout, compose_overlay_pixels, precompute_dimmed, precompute_hover_dimmed,
    ToolbarCmd, ToolbarLayout, ToolbarStyleState,
};
use crate::overlay_input::{
    cursor_for_hover, handle_key_down, handle_lbutton_dblclk, handle_lbutton_down,
    handle_mouse_move, handle_mouse_wheel,
};
use crate::overlay_text::{paint_overlay_gdi_labels, render_text_annotations_bgra, TextMaskCache};
use crate::overlay_text_editor::{
    InlineTextEditor, TIMER_TEXT_CARET, WM_APP_TEXT_CANCEL, WM_APP_TEXT_COMMIT, WM_APP_TEXT_SYNC,
};
use crate::screen::{capture_virtual_desktop, enumerate_snap_windows, DesktopSnapshot, ScreenRect};
use crate::snap_detect::SnapContext;
use crate::snap_motion::{SnapMotion, SnapTickRequest};
use crate::win_record::RecordRequest;
use std::ffi::c_void;
use tomoyo_core::{CaptureToolHotkeys, PrivateFonts, TomoyoConfig};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject,
    EndPaint, InvalidateRect, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, FF_DONTCARE, FW_NORMAL,
    HBITMAP, HDC, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetFocus, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, GetWindowLongPtrW,
    KillTimer, LoadCursorW, PostQuitMessage, RegisterClassW, SetCursor, SetTimer,
    SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_DBLCLKS, GWLP_USERDATA, IDC_CROSS, MSG,
    SW_SHOW, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub(crate) const TIMER_WHEEL_HUD: usize = 601;
pub(crate) const TIMER_SNAP_MOTION: usize = 602;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Region,
    QuickOcr,
    QuickTranslate,
    Record,
}

pub enum CaptureOutcome {
    Done,
    Pin { width: u32, height: u32, bgra: Vec<u8>, screen_x: i32, screen_y: i32 },
    ScrollLong { screen_x: i32, screen_y: i32, width: i32, height: i32, save_dir: String },
    Record(RecordRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragMode {
    None,
    Creating(i32, i32),
    Moving(i32, i32, ScreenRect),
    Resizing(usize, ScreenRect),
    Drawing(i32, i32),
    MoveAnnot(usize, i32, i32),
    EditControl(usize, usize),
    PendingTextDrag(i32, i32, i32, i32),
    MoveTextEditor(i32, i32, i32, i32),
}

pub(crate) struct OverlayState {
    pub _private_fonts: PrivateFonts,
    pub font_face: &'static str,
    pub snap: DesktopSnapshot,
    pub dimmed: Vec<u8>,
    pub hover_dimmed: Vec<u8>,
    pub snap_ctx: SnapContext,
    pub snap_motion: SnapMotion,
    pub start_time: std::time::Instant,
    pub mode: CaptureMode,
    pub save_dir: String,
    pub hex_lower: bool,
    pub rgb_mode: bool,
    pub tool_hotkeys: CaptureToolHotkeys,
    pub sel: Option<ScreenRect>,
    pub tool: ToolKind,
    pub color_idx: usize,
    pub thickness: i32,
    pub step_radius: i32,
    pub text_size: i32,
    pub text_bold: bool,
    pub shape_filled: bool,
    pub mosaic_blur: bool,
    pub record_open: bool,
    pub rec_is_gif: bool,
    pub rec_fps: u8,
    pub rec_audio: u8,
    pub rec_cursor: bool,
    pub stack: AnnotationStack,
    pub live_ann: Option<Annotation>,
    pub pen_pts: Vec<(i32, i32)>,
    pub drag: DragMode,
    pub cursor: (i32, i32),
    pub hovered_cmd: Option<ToolbarCmd>,
    pub hovered_annot: Option<usize>,
    pub hovered_ctrl: Option<(usize, usize)>,
    pub wheel_hud: Option<String>,
    pub text_editor: Option<InlineTextEditor>,
    pub text_cache: TextMaskCache,
    pub layout: ToolbarLayout,
    pub mem_dc: HDC,
    pub dib_bmp: HBITMAP,
    pub dib_bits: *mut u8,
    pub font: HFONT,
    pub outcome: Option<CaptureOutcome>,
}

impl OverlayState {
    pub(crate) fn now_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    pub(crate) fn apply_snap_tick(&self, hwnd: HWND, tick: SnapTickRequest) {
        unsafe {
            if let Some(ms) = tick.next_timer_ms {
                let _ = SetTimer(Some(hwnd), TIMER_SNAP_MOTION, ms.max(10), None);
            } else {
                let _ = KillTimer(Some(hwnd), TIMER_SNAP_MOTION);
            }
            if tick.needs_redraw {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
    }
}

impl Drop for OverlayState {
    fn drop(&mut self) {
        if let Some(ed) = self.text_editor.take() {
            ed.cancel_and_close();
        }
        unsafe {
            if !self.font.is_invalid() { let _ = DeleteObject(self.font.into()); }
            if !self.dib_bmp.is_invalid() { let _ = DeleteObject(self.dib_bmp.into()); }
            if !self.mem_dc.is_invalid() { let _ = DeleteDC(self.mem_dc); }
        }
    }
}

pub fn run_capture_overlay(mode: CaptureMode) -> Result<CaptureOutcome, String> {
    let cfg = TomoyoConfig::load_or_create();
    let snap = capture_virtual_desktop()?;
    let sw = enumerate_snap_windows(snap.origin_x, snap.origin_y, snap.width, snap.height);
    let mut pt = POINT::default();
    unsafe { let _ = GetCursorPos(&mut pt); }
    let init_cursor = (
        (pt.x - snap.origin_x).clamp(0, (snap.width - 1).max(0)),
        (pt.y - snap.origin_y).clamp(0, (snap.height - 1).max(0)),
    );
    let start_time = std::time::Instant::now();
    let mut snap_ctx = SnapContext::with_fullscreen(sw.windows, sw.fullscreen, sw.monitors, snap.width, snap.height);
    let initial_snap = snap_ctx.hover_at(&snap, init_cursor.0, init_cursor.1);
    let snap_motion = SnapMotion::new(initial_snap, 0);
    let dimmed = precompute_dimmed(&snap.bgra);
    let hover_dimmed = precompute_hover_dimmed(&snap.bgra);

    unsafe {
        let mem_dc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: snap.width, biHeight: -snap.height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib_bmp = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|e| format!("overlay DIB failed: {e}"))?;
        let _ = SelectObject(mem_dc, dib_bmp.into());
        let private_fonts = PrivateFonts::load_default();
        let font_face = private_fonts.medium_family();
        let face: Vec<u16> = font_face.encode_utf16().chain(std::iter::once(0)).collect();
        let font = CreateFontW(
            -13, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(face.as_ptr()),
        );
        let hinst = GetModuleHandleW(None).map_err(|e| format!("{e}"))?;
        let cls = w!("TomoyoCaptureOverlayClass");
        let _ = RegisterClassW(&WNDCLASSW {
            style: CS_DBLCLKS, lpfnWndProc: Some(overlay_wndproc), hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(), lpszClassName: cls,
            ..Default::default()
        });
        let (ox, oy, w, h) = (snap.origin_x, snap.origin_y, snap.width, snap.height);
        let cap = cfg.capture.with_plugin_overrides(cfg.plugin_settings.get(tomoyo_core::PLUGIN_CAPTURE));
        let rec_is_gif = cap.record_format.eq_ignore_ascii_case("gif");
        let rec_fps = cap.record_fps;
        let rec_audio = match cap.record_audio.as_str() { "system" => 1, "mic" => 2, "both" => 3, _ => 0 };
        let rec_cursor = cap.record_cursor;
        let mut st = Box::new(OverlayState {
            _private_fonts: private_fonts, font_face,
            snap, dimmed, hover_dimmed, snap_ctx, snap_motion, start_time, mode, save_dir: cap.save_dir,
            hex_lower: cap.copy_hex_lowercase, rgb_mode: false,
            tool_hotkeys: cap.tool_hotkeys, sel: None,
            tool: ToolKind::Select, color_idx: 0, thickness: 4, step_radius: 13, text_size: 22,
            text_bold: false, shape_filled: false, mosaic_blur: false,
            record_open: mode == CaptureMode::Record, rec_is_gif, rec_fps, rec_audio, rec_cursor,
            stack: AnnotationStack::default(), live_ann: None, pen_pts: Vec::new(),
            drag: DragMode::None, cursor: init_cursor, hovered_cmd: None, hovered_annot: None,
            hovered_ctrl: None, wheel_hud: None,
            text_editor: None, text_cache: TextMaskCache::default(),
            layout: ToolbarLayout::default(), mem_dc, dib_bmp,
            dib_bits: bits as *mut u8, font, outcome: None,
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW, cls, w!("Tomoyo Capture"), WS_POPUP,
            ox, oy, w, h, None, None, Some(hinst.into()), None,
        ).map_err(|e| format!("CreateWindowExW overlay failed: {e}"))?;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *st as *mut OverlayState as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = InvalidateRect(Some(hwnd), None, false);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(st.outcome.take().unwrap_or(CaptureOutcome::Done))
    }
}

unsafe extern "system" fn overlay_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut OverlayState;
    if ptr.is_null() { return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }; }
    let st = unsafe { &mut *ptr };
    let xy = || ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER if wparam.0 == TIMER_WHEEL_HUD => {
            st.wheel_hud = None;
            unsafe { let _ = KillTimer(Some(hwnd), TIMER_WHEEL_HUD); let _ = InvalidateRect(Some(hwnd), None, false); }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_SNAP_MOTION => {
            if st.sel.is_none() && st.drag == DragMode::None {
                let now_ms = st.now_ms();
                let tick = st.snap_motion.on_timer(&mut st.snap_ctx, &st.snap, now_ms);
                st.apply_snap_tick(hwnd, tick);
            } else {
                unsafe { let _ = KillTimer(Some(hwnd), TIMER_SNAP_MOTION); }
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_TEXT_CARET => {
            if let Some(ed) = st.text_editor.as_mut() {
                ed.caret_visible = !ed.caret_visible;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            } else {
                unsafe { let _ = KillTimer(Some(hwnd), TIMER_TEXT_CARET); }
            }
            LRESULT(0)
        }
        WM_APP_TEXT_SYNC => {
            if let Some(ed) = st.text_editor.as_mut() {
                ed.sync_from_hwnd(st.font_face);
                st.restart_text_caret_timer(hwnd);
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            LRESULT(0)
        }
        WM_APP_TEXT_COMMIT => {
            st.commit_text_if_open(hwnd);
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            LRESULT(0)
        }
        WM_APP_TEXT_CANCEL => {
            st.cancel_text_if_open(hwnd);
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if let Ok(cur) = unsafe { LoadCursorW(None, cursor_for_hover(st)) } {
                unsafe { SetCursor(Some(cur)); }
            }
            LRESULT(1)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            let style = ToolbarStyleState {
                active_tool: st.tool, active_color: st.color_idx, active_thickness: st.thickness,
                text_size: st.text_editor.as_ref().map_or(st.text_size, |e| e.size),
                text_bold: st.text_editor.as_ref().map_or(st.text_bold, |e| e.bold),
                shape_filled: st.shape_filled, mosaic_blur: st.mosaic_blur,
                allow_record: true, record_open: st.record_open, rec_is_gif: st.rec_is_gif,
                rec_fps: st.rec_fps, rec_audio: st.rec_audio, rec_cursor: st.rec_cursor,
            };
            st.layout = st.sel.filter(|r| r.is_valid()).map_or_else(ToolbarLayout::default, |s| {
                build_toolbar_layout(s, st.snap.width, st.snap.height, &style, &st.tool_hotkeys)
            });
            let show_loupe = st.sel.is_none() || matches!(st.drag, DragMode::Creating(_, _) | DragMode::Resizing(_, _));
            let byte_len = (st.snap.width as usize) * (st.snap.height as usize) * 4;
            let dst = unsafe { std::slice::from_raw_parts_mut(st.dib_bits, byte_len) };
            compose_overlay_pixels(
                dst, &st.snap, &st.dimmed, &st.hover_dimmed, st.sel,
                st.snap_motion.visual_rect(), st.stack.items(),
                st.stack.selected, st.hovered_annot, st.live_ann.as_ref(), st.cursor, show_loupe,
                st.snap_motion.fast_moving, &st.layout, st.hovered_cmd, st.wheel_hud.as_deref(),
            );
            if st.sel.is_some() {
                render_text_annotations_bgra(
                    dst, st.snap.width, st.snap.height, 0, 0, st.stack.items(), st.font_face,
                    Some(&mut st.text_cache),
                );
                if let Some(ed) = &st.text_editor {
                    ed.paint_into_bgra(dst, st.snap.width, st.snap.height, st.font_face, &mut st.text_cache);
                }
            }
            paint_overlay_gdi_labels(
                st.mem_dc, st.font, &st.snap, st.sel, st.snap_motion.target(),
                st.snap_motion.visual_rect(), st.snap_ctx.cycle_count,
                st.cursor, show_loupe, &st.layout, st.hovered_cmd, st.hex_lower, st.rgb_mode, st.wheel_hud.as_deref(),
            );
            unsafe {
                let _ = BitBlt(hdc, 0, 0, st.snap.width, st.snap.height, Some(st.mem_dc), 0, 0, SRCCOPY);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => { handle_mouse_wheel(st, hwnd, ((wparam.0 >> 16) as i16) as i32); LRESULT(0) }
        WM_LBUTTONDOWN => { let (x, y) = xy(); handle_lbutton_down(st, hwnd, x, y); LRESULT(0) }
        WM_MOUSEMOVE => { let (x, y) = xy(); handle_mouse_move(st, hwnd, x, y); LRESULT(0) }
        WM_LBUTTONUP => {
            let prev = st.drag;
            st.drag = DragMode::None;
            unsafe { let _ = ReleaseCapture(); }
            match prev {
                DragMode::Creating(_, _) => {
                    if !st.sel.is_some_and(ScreenRect::is_valid) { st.sel = st.snap_motion.target().map(|c| c.rect); }
                    if st.sel.is_some_and(ScreenRect::is_valid) {
                        match st.mode {
                            CaptureMode::QuickOcr => { st.execute_action(hwnd, ToolbarCmd::Ocr); return LRESULT(0); }
                            CaptureMode::QuickTranslate => { st.execute_action(hwnd, ToolbarCmd::Translate); return LRESULT(0); }
                            CaptureMode::Record => { st.record_open = true; }
                            CaptureMode::Region => {}
                        }
                    }
                }
                DragMode::Drawing(_, _) => {
                    if let Some(a) = st.live_ann.take() {
                        let keep = match &a {
                            Annotation::Rect { rect, .. } | Annotation::Ellipse { rect, .. } | Annotation::Mosaic { rect, .. } => rect.w >= 4 || rect.h >= 4,
                            Annotation::Arrow { x0, y0, x1, y1, .. } => (x1 - x0).abs() + (y1 - y0).abs() >= 6,
                            _ => true,
                        };
                        if keep { st.stack.push(a); }
                    }
                    st.pen_pts.clear();
                }
                DragMode::PendingTextDrag(dx, dy, ..) => {
                    if let Some(ed) = st.text_editor.as_mut() {
                        ed.place_caret_at(dx, dy, st.font_face);
                        st.restart_text_caret_timer(hwnd);
                    }
                }
                DragMode::MoveTextEditor(..) => {
                    if let Some(ed) = st.text_editor.as_mut() {
                        ed.update_layout_and_ime_anchor(st.font_face);
                        unsafe { let _ = SetFocus(Some(ed.hwnd)); }
                    }
                }
                _ => {}
            }
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = xy();
            handle_lbutton_dblclk(st, hwnd, x, y);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            if st.commit_text_if_open(hwnd) || st.stack.selected.take().is_some() {
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            } else if st.sel.is_some() {
                st.sel = None;
                st.tool = ToolKind::Select;
                st.record_open = st.mode == CaptureMode::Record;
                let now_ms = st.now_ms();
                st.snap_motion.reset_at(&mut st.snap_ctx, &st.snap, st.cursor.0, st.cursor.1, now_ms);
                unsafe { let _ = KillTimer(Some(hwnd), TIMER_SNAP_MOTION); let _ = InvalidateRect(Some(hwnd), None, false); }
            } else {
                st.execute_action(hwnd, ToolbarCmd::Cancel);
            }
            LRESULT(0)
        }
        WM_SYSKEYDOWN if wparam.0 as u16 == VK_MENU.0 => {
            if st.sel.is_none() && st.drag == DragMode::None {
                let now_ms = st.now_ms();
                let tick = st.snap_motion.on_alt_changed(&mut st.snap_ctx, &st.snap, st.cursor.0, st.cursor.1, true, now_ms);
                st.apply_snap_tick(hwnd, tick);
            }
            LRESULT(0)
        }
        WM_SYSKEYUP | WM_KEYUP if wparam.0 as u16 == VK_MENU.0 => {
            if st.sel.is_none() && st.drag == DragMode::None {
                let now_ms = st.now_ms();
                let tick = st.snap_motion.on_alt_changed(&mut st.snap_ctx, &st.snap, st.cursor.0, st.cursor.1, false, now_ms);
                st.apply_snap_tick(hwnd, tick);
            }
            if msg == WM_SYSKEYUP { LRESULT(0) } else { unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) } }
        }
        WM_KEYDOWN => { handle_key_down(st, hwnd, wparam.0 as u16); LRESULT(0) }
        WM_DESTROY => { unsafe { PostQuitMessage(0); } LRESULT(0) }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
