//! Toolbar action execution, final crop/bake, and HUD timer helpers for `OverlayState`.

#![cfg(windows)]

use crate::annotate::{Annotation, ToolKind, COLOR_PRESETS};
use crate::annotate_render::render_annotations;
use crate::export::{
    copy_bgra_to_clipboard, default_save_path, handoff_ocr_copy_direct,
    handoff_to_ocr_or_translate, save_bgra_as_png,
};
use crate::overlay::{CaptureOutcome, OverlayState, TIMER_WHEEL_HUD};
use crate::overlay_draw::ToolbarCmd;
use crate::overlay_text::bake_text_annotations_into_bgra;
use crate::screen::ScreenRect;
use crate::win_record::{RecAudio, RecFormat, RecordRequest};
use tomoyo_core::TomoyoConfig;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, KillTimer, SetTimer};

impl OverlayState {
    pub(crate) fn commit_text_if_open(&mut self, hwnd: HWND) {
        if let Some(ed) = self.text_editor.take() {
            if let Some(ann) = ed.commit_and_close() {
                self.stack.push(ann);
            }
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
        }
    }

    fn finalize_cropped(&mut self, hwnd: HWND) -> Option<(u32, u32, Vec<u8>, ScreenRect)> {
        self.commit_text_if_open(hwnd);
        let sel = self.sel?.clamp_to(self.snap.width, self.snap.height);
        if !sel.is_valid() {
            return None;
        }
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
                self.record_open = false;
                self.tool = if self.tool == t { ToolKind::Select } else { t };
                if self.tool != ToolKind::Select {
                    self.stack.selected = None;
                }
            }
            ToolbarCmd::Color(i) => {
                self.color_idx = i.min(COLOR_PRESETS.len() - 1);
                if let Some(sel_i) = self.stack.selected
                    && sel_i < self.stack.items().len()
                {
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
                            *thickness = if *highlighter {
                                (self.thickness * 3).clamp(12, 36)
                            } else {
                                self.thickness
                            };
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
                    && let Annotation::Mosaic { blur, .. } = &mut self.stack.items_mut()[sel_i]
                {
                    *blur = b;
                }
            }
            ToolbarCmd::Undo => {
                self.stack.undo();
            }
            ToolbarCmd::Record => {
                self.record_open = !self.record_open;
                if self.record_open {
                    self.tool = ToolKind::Select;
                    self.stack.selected = None;
                }
            }
            ToolbarCmd::RecFormat(g) => { self.rec_is_gif = g; }
            ToolbarCmd::RecFps(f) => { self.rec_fps = f; }
            ToolbarCmd::RecAudio(a) => { self.rec_audio = a; }
            ToolbarCmd::RecCursor => { self.rec_cursor = !self.rec_cursor; }
            ToolbarCmd::RecStart => {
                if let Some(s) = self.sel.filter(|r| r.is_valid()) {
                    let r = s.clamp_to(self.snap.width, self.snap.height);
                    let mut cfg = TomoyoConfig::load_or_create();
                    cfg.capture.record_format = if self.rec_is_gif { "gif" } else { "mp4" }.into();
                    cfg.capture.record_fps = self.rec_fps;
                    cfg.capture.record_audio = match self.rec_audio { 1 => "system", 2 => "mic", 3 => "both", _ => "none" }.into();
                    cfg.capture.record_cursor = self.rec_cursor;
                    let _ = cfg.save();
                    self.outcome = Some(CaptureOutcome::Record(RecordRequest {
                        screen_x: self.snap.origin_x + r.x,
                        screen_y: self.snap.origin_y + r.y,
                        width: r.w,
                        height: r.h,
                        format: if self.rec_is_gif { RecFormat::Gif } else { RecFormat::Mp4 },
                        fps: self.rec_fps,
                        audio: match self.rec_audio { 1 => RecAudio::System, 2 => RecAudio::Mic, 3 => RecAudio::Both, _ => RecAudio::None },
                        include_cursor: self.rec_cursor,
                        save_dir: self.save_dir.clone(),
                    }));
                    unsafe { let _ = DestroyWindow(hwnd); }
                    return;
                }
            }
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
                        width: w,
                        height: h,
                        bgra: buf,
                        screen_x: self.snap.origin_x + r.x,
                        screen_y: self.snap.origin_y + r.y,
                    }
                } else {
                    CaptureOutcome::Done
                });
                unsafe { let _ = DestroyWindow(hwnd); }
                return;
            }
            ToolbarCmd::OcrCopy => {
                if let Some((w, h, buf, _)) = self.finalize_cropped(hwnd) {
                    unsafe { let _ = DestroyWindow(hwnd); }
                    let _ = handoff_ocr_copy_direct(w, h, &buf);
                    self.outcome = Some(CaptureOutcome::Done);
                    return;
                }
                self.outcome = Some(CaptureOutcome::Done);
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
