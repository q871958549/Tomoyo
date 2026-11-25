//! Pointer, wheel, cursor feedback & customizable hotkey handlers for `tomoyo-capture`'s overlay.

#![cfg(windows)]

use crate::annotate::{
    constrain_shift_line, constrain_shift_point_rect, Annotation, ToolKind, COLOR_PRESETS,
};
use crate::export::copy_text_to_clipboard;
use crate::overlay::{DragMode, OverlayState};
use crate::overlay_draw::{hit_test_handle, resize_rect_by_handle, ToolbarCmd};
use crate::overlay_text_editor::{InlineTextEditor, TextHitZone};
use crate::screen::ScreenRect;
use tomoyo_core::CaptureToolHotkeys;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetCapture, VK_BACK, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_MENU, VK_RETURN,
    VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE,
};

fn box_handle_cursor(h_idx: usize) -> PCWSTR {
    match h_idx {
        0 | 7 => IDC_SIZENWSE,
        2 | 5 => IDC_SIZENESW,
        1 | 6 => IDC_SIZENS,
        3 | 4 => IDC_SIZEWE,
        _ => IDC_SIZEALL,
    }
}

pub fn cursor_for_hover(st: &OverlayState) -> PCWSTR {
    if st.hovered_cmd.is_some() {
        return IDC_HAND;
    }
    if let DragMode::EditControl(idx, h_idx) = st.drag {
        return match st.stack.items().get(idx) {
            Some(Annotation::Rect { .. } | Annotation::Ellipse { .. } | Annotation::Mosaic { .. }) => {
                box_handle_cursor(h_idx)
            }
            Some(Annotation::Arrow { .. }) => IDC_HAND,
            _ => IDC_SIZEALL,
        };
    }
    if matches!(st.drag, DragMode::MoveAnnot(..) | DragMode::MoveTextEditor(..)) {
        return IDC_SIZEALL;
    }
    if let DragMode::Resizing(h_idx, _) = st.drag {
        return box_handle_cursor(h_idx);
    }
    if matches!(st.drag, DragMode::Moving(..)) {
        return IDC_SIZEALL;
    }
    if let Some(ed) = &st.text_editor {
        match ed.hit_test(st.cursor.0, st.cursor.1) {
            TextHitZone::FrameDrag => return IDC_SIZEALL,
            TextHitZone::Body => return IDC_IBEAM,
            TextHitZone::Outside => {}
        }
    }
    if st.drag == DragMode::None && st.sel.is_some() {
        if let Some((idx, h_idx)) = st.hovered_ctrl {
            return match st.stack.items().get(idx) {
                Some(Annotation::Rect { .. } | Annotation::Ellipse { .. } | Annotation::Mosaic { .. }) => {
                    box_handle_cursor(h_idx)
                }
                Some(Annotation::Arrow { .. }) => IDC_HAND,
                _ => IDC_SIZEALL,
            };
        }
        if st.hovered_annot.is_some() {
            return IDC_SIZEALL;
        }
        if st.tool == ToolKind::Select
            && let Some(s) = st.sel {
                if let Some(h_idx) = hit_test_handle(s, st.cursor.0, st.cursor.1) {
                    return box_handle_cursor(h_idx);
                }
                if s.contains(st.cursor.0, st.cursor.1) {
                    return IDC_SIZEALL;
                }
            }
    }
    IDC_CROSS
}

pub fn handle_mouse_wheel(st: &mut OverlayState, hwnd: HWND, wheel_delta: i32) {
    let step = if wheel_delta > 0 { 1 } else { -1 };
    if let Some(ed) = st.text_editor.as_mut() {
        ed.size = (ed.size + step * 2).clamp(12, 64);
        st.text_size = ed.size;
        ed.update_layout_and_ime_anchor(st.font_face);
        st.show_wheel_hud(hwnd, format!("字号 {} px", st.text_size));
        return;
    }
    let target = st.hovered_annot.or(st.stack.selected);
    if let Some(idx) = target.filter(|&i| i < st.stack.items().len()) {
        let sz = st.stack.items_mut()[idx].adjust_size(step);
        match st.stack.items()[idx] {
            Annotation::Rect { .. } | Annotation::Ellipse { .. } | Annotation::Arrow { .. } => {
                st.thickness = sz.clamp(1, 20);
            }
            Annotation::StepNumber { .. } => st.step_radius = sz,
            Annotation::Text { .. } => st.text_size = sz,
            _ => {}
        }
        st.show_wheel_hud(hwnd, format!("尺寸 {sz} px"));
        return;
    }
    match st.tool {
        ToolKind::Select => {}
        ToolKind::StepNumber => {
            st.step_radius = (st.step_radius + step).clamp(10, 26);
            st.show_wheel_hud(hwnd, format!("序号 {} px", st.step_radius));
        }
        ToolKind::Text => {
            st.text_size = (st.text_size + step * 2).clamp(12, 64);
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
    if let Some(hit) = st.layout.hits.iter().find(|h| h.rect.contains(x, y)) {
        let cmd = hit.cmd;
        st.execute_action(hwnd, cmd);
        return;
    }
    let mut committed_open_editor = false;
    if let Some(ed) = &st.text_editor {
        match ed.hit_test(x, y) {
            TextHitZone::FrameDrag => {
                st.drag = DragMode::MoveTextEditor(x, y, ed.x, ed.y);
                unsafe { SetCapture(hwnd); let _ = InvalidateRect(Some(hwnd), None, false); }
                return;
            }
            TextHitZone::Body => {
                st.drag = DragMode::PendingTextDrag(x, y, ed.x, ed.y);
                unsafe { SetCapture(hwnd); }
                return;
            }
            TextHitZone::Outside => {
                committed_open_editor = st.commit_text_if_open(hwnd);
            }
        }
    }
    // Always-on direct manipulation: grab ANY existing annotation's control handle or body directly
    // without needing to switch back to a separate Select tool!
    if st.sel.is_some() {
        if let Some((idx, h_idx)) = st.stack.hit_test_any_control(x, y) {
            st.stack.selected = Some(idx);
            st.stack.save_undo_snapshot();
            st.drag = DragMode::EditControl(idx, h_idx);
            unsafe { SetCapture(hwnd); let _ = InvalidateRect(Some(hwnd), None, false); }
            return;
        }
        if let Some(hit_idx) = st.stack.hit_test_top(x, y) {
            st.stack.selected = Some(hit_idx);
            st.stack.save_undo_snapshot();
            st.drag = DragMode::MoveAnnot(hit_idx, x, y);
            unsafe { SetCapture(hwnd); let _ = InvalidateRect(Some(hwnd), None, false); }
            return;
        }
        if committed_open_editor {
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            return;
        }
        st.stack.selected = None;
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
            st.text_editor = InlineTextEditor::spawn(
                hwnd, x, y, st.text_size, COLOR_PRESETS[st.color_idx], st.text_bold, st.font_face,
            );
            st.restart_text_caret_timer(hwnd);
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
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
    st.sel = None;
    unsafe { SetCapture(hwnd); }
}

pub fn handle_lbutton_dblclk(st: &mut OverlayState, hwnd: HWND, x: i32, y: i32) {
    if st.text_editor.is_some() {
        return;
    }
    if let Some((idx, 1)) = st.stack.hit_test_any_control(x, y)
        && matches!(st.stack.items().get(idx), Some(Annotation::Arrow { .. })) {
            st.stack.save_undo_snapshot();
            st.stack.items_mut()[idx].straighten_arrow();
            st.stack.selected = Some(idx);
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            return;
        }
    if let Some(idx) = st.stack.hit_test_top(x, y)
        && matches!(st.stack.items().get(idx), Some(Annotation::Text { .. }))
        && let Some(orig) = st.stack.take_at(idx)
    {
        if let Annotation::Text { size, color, bold, .. } = &orig {
            st.text_size = *size;
            st.text_bold = *bold;
            if let Some(ci) = COLOR_PRESETS.iter().position(|c| c == color) {
                st.color_idx = ci;
            }
        }
        st.tool = ToolKind::Text;
        st.text_editor = InlineTextEditor::spawn_for_reedit(hwnd, orig, st.font_face);
        st.restart_text_caret_timer(hwnd);
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
        return;
    }
    if st.sel.is_some_and(ScreenRect::is_valid)
        && st.tool == ToolKind::Select
        && st.stack.hit_test_top(x, y).is_none()
    {
        let cmd = if st.record_open { ToolbarCmd::RecStart } else { ToolbarCmd::Copy };
        st.execute_action(hwnd, cmd);
    }
}

pub fn handle_mouse_move(st: &mut OverlayState, hwnd: HWND, x: i32, y: i32) {
    st.cursor = (x, y);
    if st.drag == DragMode::None {
        st.hovered_cmd = st.layout.hits.iter().find(|h| h.rect.contains(x, y)).map(|h| h.cmd);
        if st.hovered_cmd.is_none() && st.sel.is_some() {
            st.hovered_ctrl = st.stack.hit_test_any_control(x, y);
            st.hovered_annot = st.hovered_ctrl.map(|(i, _)| i).or_else(|| st.stack.hit_test_top(x, y));
        } else {
            st.hovered_ctrl = None;
            st.hovered_annot = None;
        }
    } else {
        st.hovered_cmd = None;
        st.hovered_ctrl = None;
        st.hovered_annot = None;
    }
    let shift = unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
    match st.drag {
        DragMode::None => {
            if st.sel.is_none() {
                st.hover_snap = st.snap_ctx.hover_at(&st.snap, x, y);
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
        DragMode::PendingTextDrag(sx, sy, ox, oy) => {
            if (x - sx).abs() > 4 || (y - sy).abs() > 4 {
                st.drag = DragMode::MoveTextEditor(sx, sy, ox, oy);
                if let Some(ed) = st.text_editor.as_mut() {
                    ed.x = (ox + x - sx).clamp(4, (st.snap.width - 24).max(4));
                    ed.y = (oy + y - sy).clamp(4, (st.snap.height - 24).max(4));
                    ed.update_layout_and_ime_anchor(st.font_face);
                }
            }
        }
        DragMode::MoveTextEditor(sx, sy, ox, oy) => {
            if let Some(ed) = st.text_editor.as_mut() {
                ed.x = (ox + x - sx).clamp(4, (st.snap.width - 24).max(4));
                ed.y = (oy + y - sy).clamp(4, (st.snap.height - 24).max(4));
                ed.update_layout_and_ime_anchor(st.font_face);
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
                    color: col, thickness: st.thickness, filled: st.shape_filled,
                }),
                ToolKind::Ellipse => Some(Annotation::Ellipse {
                    rect: ScreenRect::from_points(sx, sy, ex, ey),
                    color: col, thickness: st.thickness, filled: st.shape_filled,
                }),
                ToolKind::Arrow => Some(Annotation::arrow(sx, sy, ex, ey, col, st.thickness)),
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
    let alt = unsafe { (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 };
    let shift = unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
    if vk == VK_ESCAPE.0 {
        st.execute_action(hwnd, ToolbarCmd::Cancel);
    } else if vk == VK_TAB.0 && st.sel.is_none() {
        st.hover_snap = st.snap_ctx.cycle_next(&st.snap, st.cursor.0, st.cursor.1);
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    } else if (vk == VK_DELETE.0 || vk == VK_BACK.0) && st.stack.selected.is_some() {
        st.stack.delete_selected();
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    } else if vk == VK_RETURN.0 {
        let cmd = if st.record_open { ToolbarCmd::RecStart } else { ToolbarCmd::Copy };
        st.execute_action(hwnd, cmd);
    } else if vk == VK_SPACE.0 && st.record_open && st.sel.is_some() {
        st.execute_action(hwnd, ToolbarCmd::RecStart);
    } else if ctrl && !shift && !alt && vk == b'C' as u16 && st.sel.is_some() {
        st.execute_action(hwnd, ToolbarCmd::Copy);
    } else if ctrl && !shift && !alt && vk == b'Z' as u16 {
        st.stack.undo();
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    } else if ctrl && !shift && !alt && vk == b'Y' as u16 {
        st.stack.redo();
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    } else if ctrl && !shift && !alt && vk == b'S' as u16 {
        st.execute_action(hwnd, ToolbarCmd::Save);
    } else if vk == VK_SHIFT.0 && st.sel.is_none() {
        st.rgb_mode = !st.rgb_mode;
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    } else if !ctrl && !alt && !shift && vk == b'C' as u16 && st.sel.is_none() {
        let (r, g, b) = st.snap.pixel_rgb(st.cursor.0, st.cursor.1);
        let txt = if st.rgb_mode { format!("rgb({r}, {g}, {b})") }
            else if st.hex_lower { format!("#{r:02x}{g:02x}{b:02x}") }
            else { format!("#{r:02X}{g:02X}{b:02X}") };
        copy_text_to_clipboard(&txt);
        st.execute_action(hwnd, ToolbarCmd::Cancel);
    } else if st.sel.is_some() {
        let b = vk as u8;
        let hk = &st.tool_hotkeys;
        let m = |spec: &str| CaptureToolHotkeys::matches_combo(spec, b, ctrl, alt, shift);
        let plain = !ctrl && !alt && !shift;
        let cmd = if m(&hk.ocr_copy) { Some(ToolbarCmd::OcrCopy) }
            else if m(&hk.ocr) { Some(ToolbarCmd::Ocr) }
            else if m(&hk.translate) { Some(ToolbarCmd::Translate) }
            else if m(&hk.pin) { Some(ToolbarCmd::Pin) }
            else if m(&hk.scroll) { Some(ToolbarCmd::ScrollLong) }
            else if m(&hk.record) { Some(ToolbarCmd::Record) }
            else if m(&hk.rect) || (plain && b == b'1') { Some(ToolbarCmd::Tool(ToolKind::Rect)) }
            else if m(&hk.ellipse) || (plain && b == b'2') { Some(ToolbarCmd::Tool(ToolKind::Ellipse)) }
            else if m(&hk.arrow) || (plain && b == b'3') { Some(ToolbarCmd::Tool(ToolKind::Arrow)) }
            else if m(&hk.pen) || (plain && b == b'4') { Some(ToolbarCmd::Tool(ToolKind::Pen)) }
            else if m(&hk.highlighter) { Some(ToolbarCmd::Tool(ToolKind::Highlighter)) }
            else if m(&hk.mosaic) || (plain && b == b'5') { Some(ToolbarCmd::Tool(ToolKind::Mosaic)) }
            else if m(&hk.step) || (plain && b == b'6') { Some(ToolbarCmd::Tool(ToolKind::StepNumber)) }
            else if m(&hk.text) { Some(ToolbarCmd::Tool(ToolKind::Text)) }
            else { None };
        if let Some(c) = cmd {
            st.execute_action(hwnd, c);
        }
    }
}
