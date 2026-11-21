//! Pointer, wheel, and customizable hotkey input handlers for `tomoyo-capture`'s interactive overlay.

#![cfg(windows)]

use crate::annotate::{
    constrain_shift_line, constrain_shift_point_rect, Annotation, ToolKind, COLOR_PRESETS,
};
use crate::export::copy_text_to_clipboard;
use crate::overlay::{DragMode, OverlayState};
use crate::overlay_draw::{hit_test_handle, resize_rect_by_handle, ToolbarCmd};
use crate::overlay_text::InlineTextEditor;
use crate::screen::ScreenRect;
use tomoyo_core::CaptureToolHotkeys;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetCapture, VK_BACK, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_RETURN, VK_SHIFT,
};

pub fn handle_mouse_wheel(st: &mut OverlayState, hwnd: HWND, wheel_delta: i32) {
    let step = if wheel_delta > 0 { 1 } else { -1 };
    if let Some(idx) = st.stack.selected {
        if idx < st.stack.items().len() {
            let sz = st.stack.items_mut()[idx].adjust_size(step);
            st.show_wheel_hud(hwnd, format!("尺寸 {sz} px"));
            return;
        }
    }
    match st.tool {
        ToolKind::Select => {}
        ToolKind::StepNumber => {
            st.step_radius = (st.step_radius + step).clamp(10, 26);
            st.show_wheel_hud(hwnd, format!("序号 {} px", st.step_radius));
        }
        ToolKind::Text => {
            st.text_size = (st.text_size + step * 2).clamp(12, 52);
            st.show_wheel_hud(hwnd, format!("字号 {} px", st.text_size));
        }
        _ => {
            st.thickness = (st.thickness + step).clamp(1, 20);
            st.show_wheel_hud(hwnd, format!("粗细 {} px", st.thickness));
        }
    }
}

pub fn handle_lbutton_down(st: &mut OverlayState, hwnd: HWND, x: i32, y: i32) {
    st.cursor = (x, y);
    st.commit_text_if_open(hwnd);
    if let Some(hit) = st.layout.hits.iter().find(|h| h.rect.contains(x, y)) {
        let cmd = hit.cmd;
        st.execute_action(hwnd, cmd);
        return;
    }
    if let Some(idx) = st.stack.selected {
        if let Some(h_idx) = st.stack.items().get(idx).and_then(|a| a.hit_test_control(x, y)) {
            st.stack.save_undo_snapshot();
            st.drag = DragMode::EditControl(idx, h_idx);
            unsafe { SetCapture(hwnd); }
            return;
        }
    }
    if st.sel.is_some() {
        if let Some(hit_idx) = st.stack.hit_test_top(x, y) {
            if st.tool == ToolKind::Select || st.stack.selected == Some(hit_idx) {
                st.stack.selected = Some(hit_idx);
                st.stack.save_undo_snapshot();
                st.drag = DragMode::MoveAnnot(hit_idx, x, y);
                unsafe { SetCapture(hwnd); let _ = InvalidateRect(Some(hwnd), None, false); }
                return;
            }
        } else {
            st.stack.selected = None;
        }
    }
    if st.sel.is_some() && st.tool != ToolKind::Select {
        if st.tool == ToolKind::StepNumber {
            let n = st.stack.next_step_number();
            st.stack.push(Annotation::StepNumber {
                cx: x, cy: y, number: n, radius: st.step_radius, color: COLOR_PRESETS[st.color_idx],
            });
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            return;
        }
        if st.tool == ToolKind::Text {
            st.text_editor = InlineTextEditor::spawn(hwnd, x, y, st.text_size, COLOR_PRESETS[st.color_idx]);
            return;
        }
        st.pen_pts.clear();
        st.pen_pts.push((x, y));
        st.drag = DragMode::Drawing(x, y);
        unsafe { SetCapture(hwnd); }
        return;
    }
    if let Some((h_idx, s)) = st.sel.and_then(|s| hit_test_handle(s, x, y).map(|i| (i, s))) {
        st.drag = DragMode::Resizing(h_idx, s);
        unsafe { SetCapture(hwnd); }
        return;
    }
    if let Some(s) = st.sel.filter(|r| r.contains(x, y)) {
        st.drag = DragMode::Moving(x, y, s);
        unsafe { SetCapture(hwnd); }
        return;
    }
    st.drag = DragMode::Creating(x, y);
    st.sel = Some(ScreenRect { x, y, w: 0, h: 0 });
    unsafe { SetCapture(hwnd); }
}

pub fn handle_mouse_move(st: &mut OverlayState, hwnd: HWND, x: i32, y: i32) {
    st.cursor = (x, y);
    st.hovered_cmd = if st.drag == DragMode::None {
        st.layout.hits.iter().find(|h| h.rect.contains(x, y)).map(|h| h.cmd)
    } else {
        None
    };
    let shift = unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
    match st.drag {
        DragMode::None => {
            if st.sel.is_none() {
                st.hover_snap = st.snap_windows.iter().copied().find(|r| r.contains(x, y));
            }
        }
        DragMode::Creating(sx, sy) => st.sel = Some(ScreenRect::from_points(sx, sy, x, y)),
        DragMode::Moving(sx, sy, orig) => {
            let nx = (orig.x + x - sx).clamp(0, (st.snap.width - orig.w).max(0));
            let ny = (orig.y + y - sy).clamp(0, (st.snap.height - orig.h).max(0));
            st.sel = Some(ScreenRect { x: nx, y: ny, w: orig.w, h: orig.h });
        }
        DragMode::Resizing(idx, orig) => st.sel = Some(resize_rect_by_handle(orig, idx, x, y)),
        DragMode::MoveAnnot(idx, lx, ly) => {
            if idx < st.stack.items().len() {
                st.stack.items_mut()[idx].translate(x - lx, y - ly);
                st.drag = DragMode::MoveAnnot(idx, x, y);
            }
        }
        DragMode::EditControl(idx, h_idx) => {
            if idx < st.stack.items().len() {
                st.stack.items_mut()[idx].move_control(h_idx, x, y);
            }
        }
        DragMode::Drawing(sx, sy) => {
            let col = COLOR_PRESETS[st.color_idx];
            let (ex, ey) = if shift && matches!(st.tool, ToolKind::Rect | ToolKind::Ellipse) {
                constrain_shift_point_rect(sx, sy, x, y)
            } else if shift && st.tool == ToolKind::Arrow {
                constrain_shift_line(sx, sy, x, y)
            } else {
                (x, y)
            };
            let mosaic_blk = (st.thickness * 3).clamp(6, 36);
            st.live_ann = match st.tool {
                ToolKind::Rect => Some(Annotation::Rect {
                    rect: ScreenRect::from_points(sx, sy, ex, ey),
                    color: col, thickness: st.thickness, style: st.shape_style,
                }),
                ToolKind::Ellipse => Some(Annotation::Ellipse {
                    rect: ScreenRect::from_points(sx, sy, ex, ey),
                    color: col, thickness: st.thickness, style: st.shape_style,
                }),
                ToolKind::Arrow => Some(Annotation::straight_arrow(
                    sx, sy, ex, ey, col, st.thickness, st.arrow_style,
                )),
                ToolKind::Mosaic => Some(Annotation::Mosaic {
                    rect: ScreenRect::from_points(sx, sy, ex, ey),
                    block: mosaic_blk, blur: st.mosaic_blur,
                }),
                ToolKind::Pen | ToolKind::Highlighter => {
                    st.pen_pts.push((x, y));
                    let hl = st.tool == ToolKind::Highlighter;
                    let t = if hl { (st.thickness * 3).clamp(12, 36) } else { st.thickness };
                    Some(Annotation::Pen { points: st.pen_pts.clone(), color: col, thickness: t, highlighter: hl })
                }
                _ => None,
            };
        }
    }
    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}

pub fn handle_key_down(st: &mut OverlayState, hwnd: HWND, vk: u16) {
    let ctrl = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
    if vk == VK_ESCAPE.0 {
        st.execute_action(hwnd, ToolbarCmd::Cancel);
    } else if (vk == VK_DELETE.0 || vk == VK_BACK.0) && st.stack.selected.is_some() {
        st.stack.delete_selected();
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    } else if vk == VK_RETURN.0 || (ctrl && vk == b'C' as u16 && st.sel.is_some()) {
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
        let txt = if st.rgb_mode { format!("rgb({r}, {g}, {b})") }
            else if st.hex_lower { format!("#{r:02x}{g:02x}{b:02x}") }
            else { format!("#{r:02X}{g:02X}{b:02X}") };
        copy_text_to_clipboard(&txt);
        st.execute_action(hwnd, ToolbarCmd::Cancel);
    } else if !ctrl && st.sel.is_some() {
        let b = vk as u8;
        let hk = &st.tool_hotkeys;
        let cmd = if CaptureToolHotkeys::matches_vk(&hk.rect, b) || b == b'1' { Some(ToolbarCmd::Tool(ToolKind::Rect)) }
            else if CaptureToolHotkeys::matches_vk(&hk.ellipse, b) || b == b'2' { Some(ToolbarCmd::Tool(ToolKind::Ellipse)) }
            else if CaptureToolHotkeys::matches_vk(&hk.arrow, b) || b == b'3' { Some(ToolbarCmd::Tool(ToolKind::Arrow)) }
            else if CaptureToolHotkeys::matches_vk(&hk.pen, b) || b == b'4' { Some(ToolbarCmd::Tool(ToolKind::Pen)) }
            else if CaptureToolHotkeys::matches_vk(&hk.highlighter, b) { Some(ToolbarCmd::Tool(ToolKind::Highlighter)) }
            else if CaptureToolHotkeys::matches_vk(&hk.mosaic, b) || b == b'5' { Some(ToolbarCmd::Tool(ToolKind::Mosaic)) }
            else if CaptureToolHotkeys::matches_vk(&hk.step, b) || b == b'6' { Some(ToolbarCmd::Tool(ToolKind::StepNumber)) }
            else if CaptureToolHotkeys::matches_vk(&hk.text, b) { Some(ToolbarCmd::Tool(ToolKind::Text)) }
            else if CaptureToolHotkeys::matches_vk(&hk.scroll, b) { Some(ToolbarCmd::ScrollLong) }
            else if CaptureToolHotkeys::matches_vk(&hk.pin, b) { Some(ToolbarCmd::Pin) }
            else if CaptureToolHotkeys::matches_vk(&hk.ocr, b) { Some(ToolbarCmd::Ocr) }
            else if CaptureToolHotkeys::matches_vk(&hk.translate, b) { Some(ToolbarCmd::Translate) }
            else { None };
        if let Some(c) = cmd {
            st.execute_action(hwnd, c);
        }
    }
}
