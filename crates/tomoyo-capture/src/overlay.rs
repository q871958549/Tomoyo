//! Interactive full-virtual-desktop borderless Win32 capture & editable vector annotation overlay.

#![cfg(windows)]

use crate::annotate::{Annotation, AnnotationStack, ToolKind, COLOR_PRESETS};
use crate::annotate_render::render_annotations;
use crate::export::{
    copy_bgra_to_clipboard, default_save_path, handoff_to_ocr_or_translate, save_bgra_as_png,
};
use crate::overlay_draw::{
    build_toolbar_layout, compose_overlay_pixels, precompute_dimmed, ToolbarCmd, ToolbarLayout,
    ToolbarStyleState,
};
use crate::overlay_input::{
    cursor_for_hover, handle_key_down, handle_lbutton_dblclk, handle_lbutton_down,
    handle_mouse_move, handle_mouse_wheel,
};
use crate::overlay_text::{
    bake_text_annotations_into_bgra, paint_overlay_gdi_labels, InlineTextEditor,
};
use crate::screen::{capture_virtual_desktop, enumerate_snap_windows, DesktopSnapshot, ScreenRect};
use std::ffi::c_void;
use tomoyo_core::{CaptureToolHotkeys, TomoyoConfig};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject,
    EndPaint, InvalidateRect, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, FF_DONTCARE, FW_NORMAL,
    HBITMAP, HDC, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetFocus, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassW, SetCursor,
    SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_DBLCLKS, GWLP_USERDATA,
    IDC_CROSS, MSG, SW_SHOW, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONUP,
    WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const TIMER_WHEEL_HUD: usize = 601;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Region,
    QuickOcr,
    QuickTranslate,
}

pub enum CaptureOutcome {
    Done,
    Pin { width: u32, height: u32, bgra: Vec<u8>, screen_x: i32, screen_y: i32 },
    ScrollLong { screen_x: i32, screen_y: i32, width: i32, height: i32, save_dir: String },
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
}

pub(crate) struct OverlayState {
    pub snap: DesktopSnapshot,
    pub dimmed: Vec<u8>,
    pub snap_windows: Vec<ScreenRect>,
    pub mode: CaptureMode,
    pub save_dir: String,
    pub hex_lower: bool,
    pub rgb_mode: bool,
    pub tool_hotkeys: CaptureToolHotkeys,
    pub sel: Option<ScreenRect>,
    pub hover_snap: Option<ScreenRect>,
    pub tool: ToolKind,
    pub color_idx: usize,
    pub thickness: i32,
    pub step_radius: i32,
    pub text_size: i32,
    pub shape_filled: bool,
    pub mosaic_blur: bool,
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
    pub layout: ToolbarLayout,
    pub mem_dc: HDC,
    pub dib_bmp: HBITMAP,
    pub dib_bits: *mut u8,
    pub font: HFONT,
    pub outcome: Option<CaptureOutcome>,
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

impl OverlayState {
    pub(crate) fn commit_text_if_open(&mut self, hwnd: HWND) {
        if let Some(ed) = self.text_editor.take() {
            if let Some(ann) = ed.commit_and_close() {
                self.stack.push(ann);
            }
            unsafe { let _ = SetFocus(Some(hwnd)); }
        }
    }

    fn finalize_cropped(&mut self, hwnd: HWND) -> Option<(u32, u32, Vec<u8>, ScreenRect)> {
        self.commit_text_if_open(hwnd);
        let sel = self.sel?.clamp_to(self.snap.width, self.snap.height);
        if !sel.is_valid() { return None; }
        let (cw, ch, mut buf) = self.snap.crop_bgra(sel)?;
        render_annotations(&mut buf, cw as i32, ch as i32, sel.x, sel.y, self.stack.items());
        bake_text_annotations_into_bgra(&mut buf, cw as i32, ch as i32, sel.x, sel.y, self.stack.items());
        Some((cw, ch, buf, sel))
    }

    pub(crate) fn show_wheel_hud(&mut self, hwnd: HWND, msg: String) {
        self.wheel_hud = Some(msg);
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER_WHEEL_HUD);
            let _ = SetTimer(Some(hwnd), TIMER_WHEEL_HUD, 900, None);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }

    pub(crate) fn execute_action(&mut self, hwnd: HWND, cmd: ToolbarCmd) {
        self.commit_text_if_open(hwnd);
        match cmd {
            ToolbarCmd::Tool(t) => {
                self.tool = if self.tool == t { ToolKind::Select } else { t };
                if self.tool != ToolKind::Select { self.stack.selected = None; }
            }
            ToolbarCmd::Color(i) => {
                self.color_idx = i.min(COLOR_PRESETS.len() - 1);
                if let Some(sel_i) = self.stack.selected
                    && sel_i < self.stack.items().len() {
                        self.stack.save_undo_snapshot();
                        self.stack.items_mut()[sel_i].set_color(COLOR_PRESETS[self.color_idx]);
                    }
            }
            ToolbarCmd::Thickness(t) => {
                self.thickness = t.clamp(1, 20);
                if let Some(sel_i) = self.stack.selected.filter(|&i| i < self.stack.items().len()) {
                    self.stack.save_undo_snapshot();
                    match &mut self.stack.items_mut()[sel_i] {
                        Annotation::Rect { thickness, .. }
                        | Annotation::Ellipse { thickness, .. }
                        | Annotation::Arrow { thickness, .. } => *thickness = self.thickness,
                        Annotation::Pen { thickness, highlighter, .. } => {
                            *thickness = if *highlighter { (self.thickness * 3).clamp(12, 36) } else { self.thickness };
                        }
                        Annotation::Mosaic { block, .. } => *block = (self.thickness * 3).clamp(6, 36),
                        _ => {}
                    }
                }
                self.show_wheel_hud(hwnd, format!("粗细 {} px", self.thickness));
            }
            ToolbarCmd::ShapeFilled(f) => {
                self.shape_filled = f;
                if let Some(sel_i) = self.stack.selected.filter(|&i| i < self.stack.items().len()) {
                    match &mut self.stack.items_mut()[sel_i] {
                        Annotation::Rect { filled, .. } | Annotation::Ellipse { filled, .. } => {
                            *filled = f;
                        }
                        _ => {}
                    }
                }
            }
            ToolbarCmd::MosaicBlur(b) => {
                self.mosaic_blur = b;
                if let Some(sel_i) = self.stack.selected.filter(|&i| i < self.stack.items().len())
                    && let Annotation::Mosaic { blur, .. } = &mut self.stack.items_mut()[sel_i] {
                        *blur = b;
                    }
            }
            ToolbarCmd::Undo => { self.stack.undo(); }
            ToolbarCmd::ScrollLong => {
                if let Some(s) = self.sel.filter(|r| r.is_valid()) {
                    let r = s.clamp_to(self.snap.width, self.snap.height);
                    self.outcome = Some(CaptureOutcome::ScrollLong {
                        screen_x: self.snap.origin_x + r.x,
                        screen_y: self.snap.origin_y + r.y,
                        width: r.w,
                        height: r.h,
                        save_dir: self.save_dir.clone(),
                    });
                    unsafe { let _ = DestroyWindow(hwnd); }
                    return;
                }
            }
            ToolbarCmd::Cancel => {
                self.outcome = Some(CaptureOutcome::Done);
                unsafe { let _ = DestroyWindow(hwnd); }
                return;
            }
            ToolbarCmd::Copy => {
                if let Some((w, h, buf, _)) = self.finalize_cropped(hwnd) {
                    let _ = copy_bgra_to_clipboard(w, h, &buf);
                }
                self.outcome = Some(CaptureOutcome::Done);
                unsafe { let _ = DestroyWindow(hwnd); }
                return;
            }
            ToolbarCmd::Save => {
                if let Some((w, h, buf, _)) = self.finalize_cropped(hwnd) {
                    let path = default_save_path(&self.save_dir);
                    let _ = save_bgra_as_png(&path, w, h, &buf);
                    let _ = copy_bgra_to_clipboard(w, h, &buf);
                }
                self.outcome = Some(CaptureOutcome::Done);
                unsafe { let _ = DestroyWindow(hwnd); }
                return;
            }
            ToolbarCmd::Pin => {
                self.outcome = Some(if let Some((w, h, buf, r)) = self.finalize_cropped(hwnd) {
                    CaptureOutcome::Pin {
                        width: w, height: h, bgra: buf,
                        screen_x: self.snap.origin_x + r.x, screen_y: self.snap.origin_y + r.y,
                    }
                } else { CaptureOutcome::Done });
                unsafe { let _ = DestroyWindow(hwnd); }
                return;
            }
            ToolbarCmd::Ocr | ToolbarCmd::Translate => {
                let is_tr = cmd == ToolbarCmd::Translate;
                if let Some((w, h, buf, _)) = self.finalize_cropped(hwnd) {
                    unsafe { let _ = DestroyWindow(hwnd); }
                    let _ = handoff_to_ocr_or_translate(w, h, &buf, is_tr);
                    self.outcome = Some(CaptureOutcome::Done);
                    return;
                }
                self.outcome = Some(CaptureOutcome::Done);
                unsafe { let _ = DestroyWindow(hwnd); }
                return;
            }
        }
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    }
}

pub fn run_capture_overlay(mode: CaptureMode) -> Result<CaptureOutcome, String> {
    let cfg = TomoyoConfig::load_or_create();
    let snap = capture_virtual_desktop()?;
    let snap_windows = enumerate_snap_windows(snap.origin_x, snap.origin_y, snap.width, snap.height);
    let dimmed = precompute_dimmed(&snap.bgra);

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
        let face: Vec<u16> = "Microsoft YaHei UI".encode_utf16().chain(std::iter::once(0)).collect();
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
        let mut st = Box::new(OverlayState {
            snap, dimmed, snap_windows, mode, save_dir: cfg.capture.save_dir,
            hex_lower: cfg.capture.copy_hex_lowercase, rgb_mode: false,
            tool_hotkeys: cfg.capture.tool_hotkeys, sel: None, hover_snap: None,
            tool: ToolKind::Select, color_idx: 0, thickness: 4, step_radius: 13, text_size: 20,
            shape_filled: false, mosaic_blur: false,
            stack: AnnotationStack::default(), live_ann: None, pen_pts: Vec::new(),
            drag: DragMode::None, cursor: (w / 2, h / 2), hovered_cmd: None, hovered_annot: None,
            hovered_ctrl: None, wheel_hud: None,
            text_editor: None, layout: ToolbarLayout::default(), mem_dc, dib_bmp,
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
            if msg.message == WM_KEYDOWN && st.text_editor.is_some() {
                let vk = msg.wParam.0 as u16;
                if vk == VK_RETURN.0 {
                    st.commit_text_if_open(hwnd);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    continue;
                } else if vk == VK_ESCAPE.0 {
                    if let Some(ed) = st.text_editor.take() { ed.cancel_and_close(); }
                    let _ = SetFocus(Some(hwnd));
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    continue;
                }
            }
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
                shape_filled: st.shape_filled, mosaic_blur: st.mosaic_blur,
            };
            st.layout = st.sel.filter(|r| r.is_valid()).map_or_else(ToolbarLayout::default, |s| {
                build_toolbar_layout(s, st.snap.width, st.snap.height, &style, &st.tool_hotkeys)
            });
            let show_loupe = st.sel.is_none() || matches!(st.drag, DragMode::Creating(_, _) | DragMode::Resizing(_, _));
            let byte_len = (st.snap.width as usize) * (st.snap.height as usize) * 4;
            let dst = unsafe { std::slice::from_raw_parts_mut(st.dib_bits, byte_len) };
            compose_overlay_pixels(
                dst, &st.snap, &st.dimmed, st.sel, st.hover_snap, st.stack.items(),
                st.stack.selected, st.hovered_annot, st.live_ann.as_ref(), st.cursor, show_loupe,
                &st.layout, st.hovered_cmd, st.wheel_hud.as_deref(),
            );
            paint_overlay_gdi_labels(
                st.mem_dc, st.font, &st.snap, st.sel, st.stack.items(), st.cursor, show_loupe,
                &st.layout, st.hovered_cmd, st.hex_lower, st.rgb_mode, st.wheel_hud.as_deref(),
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
                    if !st.sel.is_some_and(ScreenRect::is_valid) { st.sel = st.hover_snap; }
                    if st.sel.is_some_and(ScreenRect::is_valid) {
                        match st.mode {
                            CaptureMode::QuickOcr => { st.execute_action(hwnd, ToolbarCmd::Ocr); return LRESULT(0); }
                            CaptureMode::QuickTranslate => { st.execute_action(hwnd, ToolbarCmd::Translate); return LRESULT(0); }
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
            if st.stack.selected.take().is_some() {
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            } else if st.sel.is_some() {
                st.sel = None;
                st.tool = ToolKind::Select;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            } else {
                st.execute_action(hwnd, ToolbarCmd::Cancel);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => { handle_key_down(st, hwnd, wparam.0 as u16); LRESULT(0) }
        WM_DESTROY => { unsafe { PostQuitMessage(0); } LRESULT(0) }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
