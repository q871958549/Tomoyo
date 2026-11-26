//! Mouse, wheel, keyboard & smart snap input handling for the Linux capture overlay.

use crate::annotate::{constrain_shift_line, constrain_shift_point_rect, Annotation, ToolKind, COLOR_PRESETS};
use crate::export::copy_text_to_clipboard;
use crate::linux_overlay::{
    tomoyo_cap_free, tomoyo_cap_prompt_text, tomoyo_cap_win_redraw, CaptureMode, Drag, OverlayState,
};
use crate::overlay_draw::{hit_test_handle, resize_rect_by_handle, ToolbarCmd};
use crate::screen::ScreenRect;
use std::ffi::{c_int, c_void, CStr, CString};
use tomoyo_core::CaptureToolHotkeys;

fn gdk_key_to_vk(kv: u32) -> Option<u8> {
    match kv {
        0x0030..=0x0039 | 0x0041..=0x005A => Some(kv as u8),
        0x0061..=0x007A => Some((kv - 32) as u8),
        _ => None,
    }
}

pub(super) unsafe extern "C" fn on_key_cb(ctx: *mut c_void, kv: u32, state: u32) -> c_int {
    let st = unsafe { &mut *(ctx as *mut OverlayState) };
    let shift = (state & (1 << 0)) != 0;
    let ctrl = (state & (1 << 2)) != 0;
    let alt = (state & (1 << 3)) != 0;

    if kv == 0xFF1B {
        return c_int::from(st.exec_cmd(ToolbarCmd::Cancel));
    }
    if kv == 0xFF09 && st.sel.is_none() {
        let now_ms = st.now_ms();
        let tick = st.snap_motion.on_cycle_next(&mut st.snap_ctx, &st.snap, st.cursor.0, st.cursor.1, now_ms);
        st.schedule_snap_tick(tick);
        unsafe { tomoyo_cap_win_redraw(); }
        return 0;
    }
    if matches!(kv, 0xFFFF | 0xFF08) && st.ann_stack.selected.is_some() {
        st.ann_stack.delete_selected();
        unsafe { tomoyo_cap_win_redraw(); }
        return 0;
    }
    if matches!(kv, 0xFF0D | 0xFF8D) && st.sel.is_some() {
        let cmd = if st.record_open { ToolbarCmd::RecStart } else { ToolbarCmd::Copy };
        return c_int::from(st.exec_cmd(cmd));
    }
    if kv == 0x0020 && st.record_open && st.sel.is_some() {
        return c_int::from(st.exec_cmd(ToolbarCmd::RecStart));
    }
    if ctrl && !shift && !alt && matches!(kv, 0x0063 | 0x0043) && st.sel.is_some() {
        return c_int::from(st.exec_cmd(ToolbarCmd::Copy));
    }
    if ctrl && !shift && !alt && matches!(kv, 0x007A | 0x005A) {
        return c_int::from(st.exec_cmd(ToolbarCmd::Undo));
    }
    if ctrl && !shift && !alt && matches!(kv, 0x0079 | 0x0059) {
        st.ann_stack.redo();
        unsafe { tomoyo_cap_win_redraw(); }
        return 0;
    }
    if ctrl && !shift && !alt && matches!(kv, 0x0073 | 0x0053) {
        return c_int::from(st.exec_cmd(ToolbarCmd::Save));
    }
    if matches!(kv, 0xFFE1 | 0xFFE2) && st.sel.is_none() {
        st.rgb_mode = !st.rgb_mode;
        unsafe { tomoyo_cap_win_redraw(); }
        return 0;
    }
    if !ctrl && !alt && !shift && matches!(kv, 0x0063 | 0x0043) && st.sel.is_none() {
        let (r, g, b) = st.snap.pixel_rgb(st.cursor.0, st.cursor.1);
        let txt = if st.rgb_mode { format!("rgb({r}, {g}, {b})") }
            else if st.hex_lower { format!("#{r:02x}{g:02x}{b:02x}") }
            else { format!("#{r:02X}{g:02X}{b:02X}") };
        copy_text_to_clipboard(&txt);
        return c_int::from(st.exec_cmd(ToolbarCmd::Cancel));
    }
    if st.sel.is_some() && let Some(b) = gdk_key_to_vk(kv) {
        let hk = &st.hotkeys;
        let m = |spec: &str| CaptureToolHotkeys::matches_combo(spec, b, ctrl, alt, shift);
        let plain = !ctrl && !alt && !shift;
        let cmd = if m(&hk.ocr_copy) { Some(ToolbarCmd::OcrCopy) }
            else if m(&hk.ocr) { Some(ToolbarCmd::Ocr) }
            else if m(&hk.translate) { Some(ToolbarCmd::Translate) }
            else if m(&hk.record) { Some(ToolbarCmd::Record) }
            else if m(&hk.pin) { Some(ToolbarCmd::Pin) }
            else if m(&hk.scroll) { Some(ToolbarCmd::ScrollLong) }
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
            return c_int::from(st.exec_cmd(c));
        }
    }
    0
}

pub(super) unsafe extern "C" fn on_mouse_cb(
    ctx: *mut c_void, kind: c_int, wx: c_int, wy: c_int, delta: c_int, btn: c_int,
) -> c_int {
    let st = unsafe { &mut *(ctx as *mut OverlayState) };
    let (x, y) = st.map_pt(wx, wy);
    st.cursor = (x, y);
    match kind {
        6 => handle_wheel(st, delta),
        2 => {
            if let Some((idx, 1)) = st.ann_stack.hit_test_any_control(x, y)
                && matches!(st.ann_stack.items().get(idx), Some(Annotation::Arrow { .. }))
            {
                st.ann_stack.save_undo_snapshot();
                st.ann_stack.items_mut()[idx].straighten_arrow();
                st.ann_stack.selected = Some(idx);
                unsafe { tomoyo_cap_win_redraw(); }
                return 0;
            }
            if st.sel.is_none() { st.sel = st.snap_motion.target().map(|c| c.rect); }
            if st.sel.is_some_and(ScreenRect::is_valid) {
                let cmd = if st.record_open { ToolbarCmd::RecStart } else { ToolbarCmd::Copy };
                return c_int::from(st.exec_cmd(cmd));
            }
        }
        1 if btn == 3 => return handle_right_click(st, x, y),
        1 if btn == 1 => if handle_left_down(st, x, y) != 0 { return 1; },
        4 => return handle_left_up(st),
        5 => handle_move(st, x, y, delta),
        _ => {}
    }
    unsafe { tomoyo_cap_win_redraw(); }
    0
}

fn handle_wheel(st: &mut OverlayState, delta: c_int) {
    let step = if delta > 0 { 1 } else { -1 };
    let target = st.hovered_annot.or(st.ann_stack.selected);
    if let Some(idx) = target.filter(|&i| i < st.ann_stack.items().len()) {
        let sz = st.ann_stack.items_mut()[idx].adjust_size(step);
        st.show_wheel_hud(format!("尺寸 {sz} px"));
        return;
    }
    match st.tool {
        ToolKind::Select => {}
        ToolKind::StepNumber => {
            st.step_radius = (st.step_radius + step).clamp(10, 26);
            st.show_wheel_hud(format!("序号 {} px", st.step_radius));
        }
        ToolKind::Text => {
            st.text_size = (st.text_size + step * 2).clamp(12, 52);
            st.show_wheel_hud(format!("字号 {} px", st.text_size));
        }
        _ => {
            st.thickness = (st.thickness + step).clamp(1, 20);
            st.show_wheel_hud(format!("粗细 {} px", st.thickness));
        }
    }
}

fn handle_right_click(st: &mut OverlayState, x: i32, y: i32) -> c_int {
    if st.ann_stack.selected.take().is_some() {
        return 0;
    }
    if st.sel.is_some() {
        st.sel = None;
        st.ann_stack = Default::default();
        st.tool = ToolKind::Select;
        st.record_open = st.mode == CaptureMode::Record;
        let now_ms = st.now_ms();
        st.snap_motion.reset_at(&mut st.snap_ctx, &st.snap, x, y, now_ms);
        st.snap_timer_deadline = None;
        st.rebuild_layout();
        st.update_hover_cursor();
        return 0;
    }
    c_int::from(st.exec_cmd(ToolbarCmd::Cancel))
}

fn handle_left_down(st: &mut OverlayState, x: i32, y: i32) -> c_int {
    if let Some(hit) = st.layout.hits.iter().find(|h| h.rect.contains(x, y)) {
        return c_int::from(st.exec_cmd(hit.cmd));
    }
    if st.sel.is_some() {
        if let Some((idx, h_idx)) = st.ann_stack.hit_test_any_control(x, y) {
            st.ann_stack.selected = Some(idx);
            st.ann_stack.save_undo_snapshot();
            st.drag = Drag::EditControl(idx, h_idx);
            return 0;
        }
        if let Some(idx) = st.ann_stack.hit_test_top(x, y) {
            st.ann_stack.selected = Some(idx);
            st.ann_stack.save_undo_snapshot();
            st.drag = Drag::MoveAnnot(idx, x, y);
            return 0;
        }
        st.ann_stack.selected = None;
    }
    if st.sel.is_some() && st.tool != ToolKind::Select {
        let col = COLOR_PRESETS[st.color_idx];
        if st.tool == ToolKind::Text {
            let pt = CString::new("输入标注文字").unwrap();
            let raw = unsafe { tomoyo_cap_prompt_text(pt.as_ptr()) };
            if !raw.is_null() {
                let txt = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
                unsafe { tomoyo_cap_free(raw.cast()); }
                if !txt.trim().is_empty() {
                    st.ann_stack.push(Annotation::Text { x, y, text: txt, size: st.text_size, color: col, bold: false });
                }
            }
            return 0;
        }
        if st.tool == ToolKind::StepNumber {
            let n = st.ann_stack.next_step_number();
            st.ann_stack.push(Annotation::StepNumber { cx: x, cy: y, number: n, radius: st.step_radius, color: col });
            return 0;
        }
        st.pen_pts.clear();
        st.pen_pts.push((x, y));
        st.drag = Drag::Annotating(x, y);
        return 0;
    }
    if let Some(sel) = st.sel && let Some(h) = hit_test_handle(sel, x, y) {
        st.drag = Drag::Resizing(h, sel);
        return 0;
    }
    if let Some(sel) = st.sel && sel.contains(x, y) {
        st.drag = Drag::Moving(x, y, sel);
        return 0;
    }
    st.sel = None;
    st.drag = Drag::Creating(x, y);
    st.snap_motion.clear_pending();
    st.snap_timer_deadline = None;
    st.rebuild_layout();
    0
}

fn handle_left_up(st: &mut OverlayState) -> c_int {
    if matches!(st.drag, Drag::Annotating(..)) && let Some(a) = st.live_ann.take() {
        st.ann_stack.push(a);
    }
    let was_creating = matches!(st.drag, Drag::Creating(..));
    st.drag = Drag::None;
    st.pen_pts.clear();
    if was_creating && !st.sel.is_some_and(ScreenRect::is_valid) {
        st.sel = st.snap_motion.target().map(|c| c.rect);
    }
    if st.sel.is_some_and(ScreenRect::is_valid) && st.mode == CaptureMode::Record {
        st.record_open = true;
    }
    st.rebuild_layout();
    st.update_hover_cursor();
    if was_creating && st.sel.is_some_and(ScreenRect::is_valid) {
        match st.mode {
            CaptureMode::QuickOcr => return c_int::from(st.exec_cmd(ToolbarCmd::Ocr)),
            CaptureMode::QuickTranslate => return c_int::from(st.exec_cmd(ToolbarCmd::Translate)),
            CaptureMode::Region | CaptureMode::Record => {}
        }
    }
    0
}

fn handle_move(st: &mut OverlayState, x: i32, y: i32, mod_state: c_int) {
    let shift = (mod_state & 1) != 0;
    let alt = (mod_state & 8) != 0;
    match st.drag {
        Drag::Creating(sx, sy) => {
            st.sel = Some(ScreenRect::from_points(sx, sy, x, y).clamp_to(st.snap.width, st.snap.height));
            st.rebuild_layout();
        }
        Drag::Moving(sx, sy, orig) => {
            let nx = (orig.x + x - sx).clamp(0, (st.snap.width - orig.w).max(0));
            let ny = (orig.y + y - sy).clamp(0, (st.snap.height - orig.h).max(0));
            st.sel = Some(ScreenRect { x: nx, y: ny, w: orig.w, h: orig.h });
            st.rebuild_layout();
        }
        Drag::Resizing(idx, orig) => {
            st.sel = Some(resize_rect_by_handle(orig, idx, x, y).clamp_to(st.snap.width, st.snap.height));
            st.rebuild_layout();
        }
        Drag::MoveAnnot(idx, lx, ly) => {
            if idx < st.ann_stack.items().len() {
                st.ann_stack.items_mut()[idx].translate(x - lx, y - ly);
                st.drag = Drag::MoveAnnot(idx, x, y);
            }
        }
        Drag::EditControl(idx, h_idx) => {
            if idx < st.ann_stack.items().len() { st.ann_stack.items_mut()[idx].move_control(h_idx, x, y); }
        }
        Drag::Annotating(sx, sy) => {
            let col = COLOR_PRESETS[st.color_idx];
            let (ex, ey) = if shift && matches!(st.tool, ToolKind::Rect | ToolKind::Ellipse) {
                constrain_shift_point_rect(sx, sy, x, y)
            } else if shift && st.tool == ToolKind::Arrow {
                constrain_shift_line(sx, sy, x, y)
            } else { (x, y) };
            let mosaic_blk = (st.thickness * 3).clamp(6, 36);
            st.live_ann = Some(match st.tool {
                ToolKind::Rect => Annotation::Rect { rect: ScreenRect::from_points(sx, sy, ex, ey), color: col, thickness: st.thickness, filled: st.shape_filled },
                ToolKind::Ellipse => Annotation::Ellipse { rect: ScreenRect::from_points(sx, sy, ex, ey), color: col, thickness: st.thickness, filled: st.shape_filled },
                ToolKind::Arrow => Annotation::arrow(sx, sy, ex, ey, col, st.thickness),
                ToolKind::Mosaic => Annotation::Mosaic { rect: ScreenRect::from_points(sx, sy, x, y), block: mosaic_blk, blur: st.mosaic_blur },
                _ => {
                    st.pen_pts.push((x, y));
                    let hl = st.tool == ToolKind::Highlighter;
                    let t = if hl { (st.thickness * 3).clamp(12, 36) } else { st.thickness };
                    Annotation::Pen { points: st.pen_pts.clone(), color: col, thickness: t, highlighter: hl }
                }
            });
        }
        Drag::None => {
            st.hovered_cmd = st.layout.hits.iter().find(|h| h.rect.contains(x, y)).map(|h| h.cmd);
            if st.sel.is_none() {
                let now_ms = st.now_ms();
                let tick = st.snap_motion.on_pointer_move(&mut st.snap_ctx, &st.snap, x, y, alt, now_ms);
                st.schedule_snap_tick(tick);
            } else if st.hovered_cmd.is_none() {
                st.hovered_ctrl = st.ann_stack.hit_test_any_control(x, y);
                st.hovered_annot = st.hovered_ctrl.map(|(i, _)| i).or_else(|| st.ann_stack.hit_test_top(x, y));
            } else {
                st.hovered_ctrl = None;
                st.hovered_annot = None;
            }
            st.update_hover_cursor();
        }
    }
}
