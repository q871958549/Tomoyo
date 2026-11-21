//! Interactive full-virtual-desktop borderless Win32 capture & annotation overlay window.

#![cfg(windows)]

use crate::annotate::{
    render_annotations, Annotation, AnnotationStack, ToolKind, COLOR_PRESETS,
};
use crate::export::{
    copy_bgra_to_clipboard, copy_text_to_clipboard, default_save_path,
    handoff_to_ocr_or_translate, save_bgra_as_png,
};
use crate::overlay_draw::{
    build_toolbar_layout, compose_overlay_pixels, hit_test_handle, paint_overlay_gdi_labels,
    precompute_dimmed, resize_rect_by_handle, ToolbarCmd, ToolbarLayout,
};
use crate::screen::{capture_virtual_desktop, enumerate_snap_windows, DesktopSnapshot, ScreenRect};
use std::ffi::c_void;
use tomoyo_core::TomoyoConfig;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject,
    EndPaint, InvalidateRect, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, FF_DONTCARE, FW_NORMAL,
    HBITMAP, HDC, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_ESCAPE, VK_RETURN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, LoadCursorW, PostQuitMessage, RegisterClassW, SetWindowLongPtrW, ShowWindow,
    TranslateMessage, CS_DBLCLKS, GWLP_USERDATA, IDC_CROSS, MSG, SW_SHOW, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Region,
    QuickOcr,
    QuickTranslate,
}

pub enum CaptureOutcome {
    Done,
    Pin { width: u32, height: u32, bgra: Vec<u8>, screen_x: i32, screen_y: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragMode {
    None,
    Creating(i32, i32),
    Moving(i32, i32, ScreenRect),
    Resizing(usize, ScreenRect),
    Drawing(i32, i32),
}

struct OverlayState {
    snap: DesktopSnapshot,
    dimmed: Vec<u8>,
    snap_windows: Vec<ScreenRect>,
    mode: CaptureMode,
    save_dir: String,
    hex_lower: bool,
    rgb_mode: bool,
    sel: Option<ScreenRect>,
    hover_snap: Option<ScreenRect>,
    tool: ToolKind,
    color_idx: usize,
    thickness: i32,
    stack: AnnotationStack,
    live_ann: Option<Annotation>,
    pen_pts: Vec<(i32, i32)>,
    drag: DragMode,
    cursor: (i32, i32),
    hovered_cmd: Option<ToolbarCmd>,
    layout: ToolbarLayout,
    mem_dc: HDC,
    dib_bmp: HBITMAP,
    dib_bits: *mut u8,
    font: HFONT,
    outcome: Option<CaptureOutcome>,
}

impl Drop for OverlayState {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_invalid() { let _ = DeleteObject(self.font.into()); }
            if !self.dib_bmp.is_invalid() { let _ = DeleteObject(self.dib_bmp.into()); }
            if !self.mem_dc.is_invalid() { let _ = DeleteDC(self.mem_dc); }
        }
    }
}

impl OverlayState {
    fn finalize_cropped(&self) -> Option<(u32, u32, Vec<u8>, ScreenRect)> {
        let sel = self.sel?.clamp_to(self.snap.width, self.snap.height);
        if !sel.is_valid() {
            return None;
        }
        let (cw, ch, mut buf) = self.snap.crop_bgra(sel)?;
        render_annotations(&mut buf, cw as i32, ch as i32, sel.x, sel.y, self.stack.items());
        Some((cw, ch, buf, sel))
    }

    fn execute_action(&mut self, hwnd: HWND, cmd: ToolbarCmd) {
        match cmd {
            ToolbarCmd::Tool(t) => self.tool = if self.tool == t { ToolKind::Select } else { t },
            ToolbarCmd::Color(i) => self.color_idx = i.min(COLOR_PRESETS.len() - 1),
            ToolbarCmd::Thickness(t) => self.thickness = t.clamp(2, 8),
            ToolbarCmd::Undo => { self.stack.undo(); }
            ToolbarCmd::Cancel => {
                self.outcome = Some(CaptureOutcome::Done);
                unsafe { let _ = DestroyWindow(hwnd); }
            }
            ToolbarCmd::Copy => {
                if let Some((w, h, buf, _)) = self.finalize_cropped() {
                    let _ = copy_bgra_to_clipboard(w, h, &buf);
                }
                self.outcome = Some(CaptureOutcome::Done);
                unsafe { let _ = DestroyWindow(hwnd); }
            }
            ToolbarCmd::Save => {
                if let Some((w, h, buf, _)) = self.finalize_cropped() {
                    let path = default_save_path(&self.save_dir);
                    let _ = save_bgra_as_png(&path, w, h, &buf);
                    let _ = copy_bgra_to_clipboard(w, h, &buf);
                }
                self.outcome = Some(CaptureOutcome::Done);
                unsafe { let _ = DestroyWindow(hwnd); }
            }
            ToolbarCmd::Pin => {
                self.outcome = Some(if let Some((w, h, buf, r)) = self.finalize_cropped() {
                    CaptureOutcome::Pin {
                        width: w, height: h, bgra: buf,
                        screen_x: self.snap.origin_x + r.x, screen_y: self.snap.origin_y + r.y,
                    }
                } else { CaptureOutcome::Done });
                unsafe { let _ = DestroyWindow(hwnd); }
            }
            ToolbarCmd::Ocr | ToolbarCmd::Translate => {
                let is_tr = cmd == ToolbarCmd::Translate;
                if let Some((w, h, buf, _)) = self.finalize_cropped() {
                    unsafe { let _ = DestroyWindow(hwnd); }
                    let _ = handoff_to_ocr_or_translate(w, h, &buf, is_tr);
                    self.outcome = Some(CaptureOutcome::Done);
                    return;
                }
                self.outcome = Some(CaptureOutcome::Done);
                unsafe { let _ = DestroyWindow(hwnd); }
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
                biWidth: snap.width,
                biHeight: -snap.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
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
        let wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(overlay_wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
            lpszClassName: cls,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        let (ox, oy, w, h) = (snap.origin_x, snap.origin_y, snap.width, snap.height);
        let mut st = Box::new(OverlayState {
            snap, dimmed, snap_windows, mode,
            save_dir: cfg.capture.save_dir, hex_lower: cfg.capture.copy_hex_lowercase, rgb_mode: false,
            sel: None, hover_snap: None, tool: ToolKind::Select, color_idx: 0, thickness: 4,
            stack: AnnotationStack::default(), live_ann: None, pen_pts: Vec::new(),
            drag: DragMode::None, cursor: (w / 2, h / 2), hovered_cmd: None,
            layout: ToolbarLayout::default(), mem_dc, dib_bmp, dib_bits: bits as *mut u8, font, outcome: None,
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

unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut OverlayState;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let st = unsafe { &mut *ptr };
    let xy = || ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            st.layout = st.sel.filter(|r| r.is_valid()).map_or_else(ToolbarLayout::default, |s| {
                build_toolbar_layout(s, st.snap.width, st.snap.height, st.tool, st.color_idx, st.thickness)
            });
            let show_loupe = st.sel.is_none() || matches!(st.drag, DragMode::Creating(_, _) | DragMode::Resizing(_, _));
            let byte_len = (st.snap.width as usize) * (st.snap.height as usize) * 4;
            let dst = unsafe { std::slice::from_raw_parts_mut(st.dib_bits, byte_len) };
            compose_overlay_pixels(
                dst, &st.snap, &st.dimmed, st.sel, st.hover_snap, st.stack.items(),
                st.live_ann.as_ref(), st.cursor, show_loupe, &st.layout, st.hovered_cmd,
            );
            paint_overlay_gdi_labels(
                st.mem_dc, st.font, &st.snap, st.sel, st.cursor, show_loupe,
                &st.layout, st.hovered_cmd, st.hex_lower, st.rgb_mode,
            );
            unsafe {
                let _ = BitBlt(hdc, 0, 0, st.snap.width, st.snap.height, Some(st.mem_dc), 0, 0, SRCCOPY);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = xy();
            st.cursor = (x, y);
            if let Some(hit) = st.layout.hits.iter().find(|h| h.rect.contains(x, y)).copied() {
                st.execute_action(hwnd, hit.cmd);
                return LRESULT(0);
            }
            if st.sel.is_some() && st.tool != ToolKind::Select {
                if st.tool == ToolKind::StepNumber {
                    let n = st.stack.next_step_number();
                    st.stack.push(Annotation::StepNumber { cx: x, cy: y, number: n, color: COLOR_PRESETS[st.color_idx] });
                    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
                    return LRESULT(0);
                }
                st.pen_pts.clear();
                st.pen_pts.push((x, y));
                st.drag = DragMode::Drawing(x, y);
                unsafe { SetCapture(hwnd); }
                return LRESULT(0);
            }
            if let Some((h_idx, s)) = st.sel.and_then(|s| hit_test_handle(s, x, y).map(|i| (i, s))) {
                st.drag = DragMode::Resizing(h_idx, s);
                unsafe { SetCapture(hwnd); }
                return LRESULT(0);
            }
            if let Some(s) = st.sel {
                if s.contains(x, y) {
                    st.drag = DragMode::Moving(x, y, s);
                    unsafe { SetCapture(hwnd); }
                    return LRESULT(0);
                }
            }
            st.drag = DragMode::Creating(x, y);
            st.sel = Some(ScreenRect { x, y, w: 0, h: 0 });
            unsafe { SetCapture(hwnd); }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = xy();
            st.cursor = (x, y);
            st.hovered_cmd = if st.drag == DragMode::None {
                st.layout.hits.iter().find(|h| h.rect.contains(x, y)).map(|h| h.cmd)
            } else {
                None
            };
            match st.drag {
                DragMode::None => {
                    if st.sel.is_none() {
                        st.hover_snap = st.snap_windows.iter().copied().find(|r| r.contains(x, y));
                    }
                }
                DragMode::Creating(sx, sy) => {
                    st.sel = Some(ScreenRect::from_points(sx, sy, x, y));
                }
                DragMode::Moving(sx, sy, orig) => {
                    let nx = (orig.x + x - sx).clamp(0, (st.snap.width - orig.w).max(0));
                    let ny = (orig.y + y - sy).clamp(0, (st.snap.height - orig.h).max(0));
                    st.sel = Some(ScreenRect { x: nx, y: ny, w: orig.w, h: orig.h });
                }
                DragMode::Resizing(idx, orig) => {
                    st.sel = Some(resize_rect_by_handle(orig, idx, x, y));
                }
                DragMode::Drawing(sx, sy) => {
                    let col = COLOR_PRESETS[st.color_idx];
                    let mosaic_blk = match st.thickness { 2 => 8, 4 => 12, _ => 18 };
                    st.live_ann = match st.tool {
                        ToolKind::Rect => Some(Annotation::Rect { rect: ScreenRect::from_points(sx, sy, x, y), color: col, thickness: st.thickness }),
                        ToolKind::Ellipse => Some(Annotation::Ellipse { rect: ScreenRect::from_points(sx, sy, x, y), color: col, thickness: st.thickness }),
                        ToolKind::Arrow => Some(Annotation::Arrow { x0: sx, y0: sy, x1: x, y1: y, color: col, thickness: st.thickness }),
                        ToolKind::Mosaic => Some(Annotation::Mosaic { rect: ScreenRect::from_points(sx, sy, x, y), block: mosaic_blk }),
                        ToolKind::Pen => {
                            st.pen_pts.push((x, y));
                            Some(Annotation::Pen { points: st.pen_pts.clone(), color: col, thickness: st.thickness })
                        }
                        _ => None,
                    };
                }
            }
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let prev = st.drag;
            st.drag = DragMode::None;
            unsafe { let _ = ReleaseCapture(); }
            match prev {
                DragMode::Creating(_, _) => {
                    if !st.sel.is_some_and(ScreenRect::is_valid) {
                        st.sel = st.hover_snap;
                    }
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
                        st.stack.push(a);
                    }
                    st.pen_pts.clear();
                }
                _ => {}
            }
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            if st.sel.is_some_and(ScreenRect::is_valid) && st.tool == ToolKind::Select {
                st.execute_action(hwnd, ToolbarCmd::Copy);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            if st.sel.is_some() {
                st.sel = None;
                st.tool = ToolKind::Select;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            } else {
                st.execute_action(hwnd, ToolbarCmd::Cancel);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            let ctrl = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
            if vk == VK_ESCAPE.0 { st.execute_action(hwnd, ToolbarCmd::Cancel); }
            else if vk == VK_RETURN.0 || (ctrl && vk == b'C' as u16 && st.sel.is_some()) {
                st.execute_action(hwnd, ToolbarCmd::Copy);
            } else if ctrl && vk == b'Z' as u16 {
                st.stack.undo();
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            } else if ctrl && vk == b'Y' as u16 {
                st.stack.redo();
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            } else if ctrl && vk == b'S' as u16 {
                st.execute_action(hwnd, ToolbarCmd::Save);
            } else if vk == VK_SHIFT.0 && st.sel.is_none() {
                st.rgb_mode = !st.rgb_mode;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            } else if vk == b'C' as u16 && st.sel.is_none() {
                let (r, g, b) = st.snap.pixel_rgb(st.cursor.0, st.cursor.1);
                let txt = if st.rgb_mode {
                    format!("rgb({r}, {g}, {b})")
                } else if st.hex_lower {
                    format!("#{r:02x}{g:02x}{b:02x}")
                } else {
                    format!("#{r:02X}{g:02X}{b:02X}")
                };
                copy_text_to_clipboard(&txt);
                st.execute_action(hwnd, ToolbarCmd::Cancel);
            } else if !ctrl {
                match vk as u8 {
                    b'1' | b'R' => st.execute_action(hwnd, ToolbarCmd::Tool(ToolKind::Rect)),
                    b'2' | b'E' => st.execute_action(hwnd, ToolbarCmd::Tool(ToolKind::Ellipse)),
                    b'3' | b'A' => st.execute_action(hwnd, ToolbarCmd::Tool(ToolKind::Arrow)),
                    b'4' | b'B' => st.execute_action(hwnd, ToolbarCmd::Tool(ToolKind::Pen)),
                    b'5' | b'M' => st.execute_action(hwnd, ToolbarCmd::Tool(ToolKind::Mosaic)),
                    b'6' | b'N' => st.execute_action(hwnd, ToolbarCmd::Tool(ToolKind::StepNumber)),
                    b'P' => st.execute_action(hwnd, ToolbarCmd::Pin),
                    b'O' => st.execute_action(hwnd, ToolbarCmd::Ocr),
                    b'T' => st.execute_action(hwnd, ToolbarCmd::Translate),
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => { unsafe { PostQuitMessage(0); } LRESULT(0) }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
