//! Native Linux GTK3/Cairo fullscreen capture overlay with smart pre-selection & Pango HUD.

use crate::annotate::{Annotation, AnnotationStack, ToolKind};
use crate::annotate_render::render_annotations;
use crate::export::{
    copy_bgra_to_clipboard, default_save_path, handoff_ocr_copy_direct,
    handoff_to_ocr_or_translate, save_bgra_as_png,
};
use crate::linux_overlay_input::{on_key_cb, on_mouse_cb};
use crate::linux_record::{has_valid_monitor_token, RecAudio, RecFormat, RecordRequest};
use crate::overlay_draw::{
    build_toolbar_layout, compose_overlay_pixels_ex, dim_badge_rect, loupe_rect, precompute_dimmed,
    rec_hint_rect, snap_badge_rect, tooltip_rect_for, wheel_hud_rect, ToolbarCmd, ToolbarLayout,
    ToolbarStyleState,
};
use crate::screen::{capture_virtual_desktop, DesktopSnapshot, ScreenRect};
use crate::snap_detect::{build_linux_snap_context, SnapCandidate, SnapContext};
use std::ffi::{c_char, c_int, c_void, CString};
use tomoyo_core::{CaptureToolHotkeys, TomoyoConfig};

unsafe extern "C" {
    fn tomoyo_cap_win_run(
        w: c_int, h: c_int, fullscreen: c_int, x: c_int, y: c_int, ctx: *mut c_void,
        on_draw: unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, c_int),
        on_key: unsafe extern "C" fn(*mut c_void, u32, u32) -> c_int,
        on_mouse: unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int, c_int) -> c_int,
    ) -> *mut c_void;
    pub(super) fn tomoyo_cap_win_redraw();
    pub(super) fn tomoyo_cap_schedule_redraw(ms: c_int);
    pub(super) fn tomoyo_cap_set_cursor(kind: c_int);
    fn tomoyo_cap_cairo_blit_bgra(cr: *mut c_void, bgra: *const u8, w: c_int, h: c_int, dw: c_int, dh: c_int);
    fn tomoyo_cap_pango_draw(
        cr: *mut c_void, font: *const c_char, rgb: u32, align: c_int, wrap: c_int,
        text: *const c_char, x: f64, y: f64, w: f64, h: f64,
    );
    fn tomoyo_cap_bake_pango_text(
        bgra: *mut u8, w: c_int, h: c_int, font_desc: *const c_char, rgb: u32, text: *const c_char, x: f64, y: f64,
    );
    pub(super) fn tomoyo_cap_prompt_text(title: *const c_char) -> *mut c_char;
    pub(super) fn tomoyo_cap_free(p: *mut c_void);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode { Region, QuickOcr, QuickTranslate, Record }

pub enum CaptureOutcome {
    Done,
    Pin { width: u32, height: u32, bgra: Vec<u8>, screen_x: i32, screen_y: i32 },
    Record(RecordRequest),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Drag {
    None,
    Creating(i32, i32),
    Moving(i32, i32, ScreenRect),
    Resizing(usize, ScreenRect),
    Annotating(i32, i32),
    MoveAnnot(usize, i32, i32),
    EditControl(usize, usize),
}

pub(super) struct OverlayState {
    pub mode: CaptureMode,
    pub snap: DesktopSnapshot,
    pub snap_ctx: SnapContext,
    pub dimmed: Vec<u8>,
    pub frame_buf: Vec<u8>,
    pub sel: Option<ScreenRect>,
    pub hover_snap: Option<SnapCandidate>,
    pub drag: Drag,
    pub cursor: (i32, i32),
    pub tool: ToolKind,
    pub color_idx: usize,
    pub thickness: i32,
    pub step_radius: i32,
    pub text_size: i32,
    pub shape_filled: bool,
    pub mosaic_blur: bool,
    pub hex_lower: bool,
    pub rgb_mode: bool,
    pub record_open: bool,
    pub rec_is_gif: bool,
    pub rec_fps: u8,
    pub rec_audio: u8,
    pub rec_cursor: bool,
    pub has_monitor_token: bool,
    pub ann_stack: AnnotationStack,
    pub live_ann: Option<Annotation>,
    pub pen_pts: Vec<(i32, i32)>,
    pub hovered_annot: Option<usize>,
    pub hovered_ctrl: Option<(usize, usize)>,
    pub wheel_hud: Option<(String, std::time::Instant)>,
    pub layout: ToolbarLayout,
    pub hovered_cmd: Option<ToolbarCmd>,
    pub save_dir: String,
    pub hotkeys: CaptureToolHotkeys,
    pub outcome: CaptureOutcome,
    pub win_dims: (i32, i32),
}

impl OverlayState {
    pub(super) fn map_pt(&self, wx: i32, wy: i32) -> (i32, i32) {
        let (ww, wh) = (self.win_dims.0.max(1) as i64, self.win_dims.1.max(1) as i64);
        (
            ((((wx as i64) * (self.snap.width as i64)) / ww) as i32).clamp(0, self.snap.width - 1),
            ((((wy as i64) * (self.snap.height as i64)) / wh) as i32).clamp(0, self.snap.height - 1),
        )
    }

    pub(super) fn rebuild_layout(&mut self) {
        self.layout = match self.sel.filter(|r| r.is_valid()) {
            Some(sel) => build_toolbar_layout(
                sel,
                self.snap.width,
                self.snap.height,
                &ToolbarStyleState {
                    active_tool: self.tool,
                    active_color: self.color_idx,
                    active_thickness: self.thickness,
                    shape_filled: self.shape_filled,
                    mosaic_blur: self.mosaic_blur,
                    allow_record: true,
                    record_open: self.record_open,
                    rec_is_gif: self.rec_is_gif,
                    rec_fps: self.rec_fps,
                    rec_audio: self.rec_audio,
                    rec_cursor: self.rec_cursor,
                },
                &self.hotkeys,
            ),
            None => ToolbarLayout::default(),
        };
    }

    pub(super) fn show_wheel_hud(&mut self, msg: String) {
        self.wheel_hud = Some((msg, std::time::Instant::now() + std::time::Duration::from_millis(900)));
        unsafe { tomoyo_cap_schedule_redraw(920); }
    }

    pub(super) fn update_hover_cursor(&self) {
        let kind = if self.hovered_cmd.is_some() { 1 }
        else if matches!(self.drag, Drag::Moving(..) | Drag::MoveAnnot(..)) || self.hovered_annot.is_some() { 2 }
        else if let Drag::Resizing(h, _) = self.drag { cursor_kind_for_handle(h) }
        else if let Some(s) = self.sel.filter(|_| self.drag == Drag::None && self.tool == ToolKind::Select) {
            if let Some(h) = crate::overlay_draw::hit_test_handle(s, self.cursor.0, self.cursor.1) { cursor_kind_for_handle(h) }
            else if s.contains(self.cursor.0, self.cursor.1) { 2 } else { 0 }
        } else { 0 };
        unsafe { tomoyo_cap_set_cursor(kind); }
    }

    fn save_record_prefs(&self) {
        let mut cfg = TomoyoConfig::load_or_create();
        cfg.capture.record_format = if self.rec_is_gif { "gif" } else { "mp4" }.into();
        cfg.capture.record_fps = self.rec_fps;
        cfg.capture.record_audio = match self.rec_audio { 1 => "system", 2 => "mic", 3 => "both", _ => "none" }.into();
        cfg.capture.record_cursor = self.rec_cursor;
        let _ = cfg.save();
    }

    fn export_sel(&self) -> Option<(u32, u32, Vec<u8>, ScreenRect)> {
        let r = self.sel?.clamp_to(self.snap.width, self.snap.height);
        let (w, h, mut buf) = self.snap.crop_bgra(r)?;
        if !self.ann_stack.items().is_empty() {
            render_annotations(&mut buf, w as i32, h as i32, r.x, r.y, self.ann_stack.items());
            for ann in self.ann_stack.items() {
                if let Annotation::Text { x, y, text, size, color } = ann
                    && !text.trim().is_empty()
                    && let Ok(cs) = CString::new(text.as_str())
                {
                    let f = CString::new(format!("阿里巴巴普惠体 3.0 55 Regular {}px", (*size).max(12))).unwrap();
                    let rgb = ((color.0 as u32) << 16) | ((color.1 as u32) << 8) | (color.2 as u32);
                    unsafe {
                        tomoyo_cap_bake_pango_text(
                            buf.as_mut_ptr(), w as i32, h as i32, f.as_ptr(), rgb, cs.as_ptr(),
                            (x - r.x) as f64, (y - r.y) as f64,
                        );
                    }
                }
            }
        }
        Some((w, h, buf, r))
    }

    pub(super) fn exec_cmd(&mut self, cmd: ToolbarCmd) -> bool {
        match cmd {
            ToolbarCmd::Tool(t) => {
                self.record_open = false;
                self.tool = if self.tool == t { ToolKind::Select } else { t };
                if self.tool != ToolKind::Select { self.ann_stack.selected = None; }
                self.rebuild_layout();
            }
            ToolbarCmd::Record => {
                self.record_open = !self.record_open;
                if self.record_open { self.tool = ToolKind::Select; self.ann_stack.selected = None; }
                self.rebuild_layout();
            }
            ToolbarCmd::RecFormat(gif) => { self.rec_is_gif = gif; if gif && self.rec_fps > 30 { self.rec_fps = 15; } self.rebuild_layout(); }
            ToolbarCmd::RecFps(f) => { self.rec_fps = f; self.rebuild_layout(); }
            ToolbarCmd::RecAudio(a) => { self.rec_audio = a; self.rebuild_layout(); }
            ToolbarCmd::RecCursor => { self.rec_cursor = !self.rec_cursor; self.rebuild_layout(); }
            ToolbarCmd::RecStart => {
                if let Some(r) = self.sel.filter(|s| s.is_valid()) {
                    self.save_record_prefs();
                    self.outcome = CaptureOutcome::Record(RecordRequest {
                        rect: r.clamp_to(self.snap.width, self.snap.height),
                        snap_w: self.snap.width,
                        snap_h: self.snap.height,
                        monitors: self.snap_ctx.monitors.clone(),
                        format: if self.rec_is_gif { RecFormat::Gif } else { RecFormat::Mp4 },
                        fps: self.rec_fps,
                        audio: match self.rec_audio { 1 => RecAudio::System, 2 => RecAudio::Mic, 3 => RecAudio::Both, _ => RecAudio::None },
                        include_cursor: self.rec_cursor,
                        save_dir: self.save_dir.clone(),
                    });
                    return true;
                }
            }
            ToolbarCmd::Color(i) => { self.color_idx = i; self.rebuild_layout(); }
            ToolbarCmd::Thickness(t) => { self.thickness = t; self.show_wheel_hud(format!("粗细 {t} px")); self.rebuild_layout(); }
            ToolbarCmd::ShapeFilled(f) => { self.shape_filled = f; self.rebuild_layout(); }
            ToolbarCmd::MosaicBlur(b) => { self.mosaic_blur = b; self.rebuild_layout(); }
            ToolbarCmd::Undo => { self.ann_stack.undo(); }
            ToolbarCmd::Cancel => { self.outcome = CaptureOutcome::Done; return true; }
            ToolbarCmd::Copy => {
                if let Some((w, h, buf, _)) = self.export_sel() { let _ = copy_bgra_to_clipboard(w, h, &buf); }
                self.outcome = CaptureOutcome::Done; return true;
            }
            ToolbarCmd::Save => {
                if let Some((w, h, buf, _)) = self.export_sel() {
                    let _ = save_bgra_as_png(&default_save_path(&self.save_dir), w, h, &buf);
                    let _ = copy_bgra_to_clipboard(w, h, &buf);
                }
                self.outcome = CaptureOutcome::Done; return true;
            }
            ToolbarCmd::Pin => {
                if let Some((w, h, buf, r)) = self.export_sel() {
                    self.outcome = CaptureOutcome::Pin { width: w, height: h, bgra: buf, screen_x: r.x, screen_y: r.y };
                    return true;
                }
            }
            ToolbarCmd::Ocr => { if let Some((w, h, buf, _)) = self.export_sel() { let _ = handoff_to_ocr_or_translate(w, h, &buf, false); } self.outcome = CaptureOutcome::Done; return true; }
            ToolbarCmd::OcrCopy => { if let Some((w, h, buf, _)) = self.export_sel() { let _ = handoff_ocr_copy_direct(w, h, &buf); } self.outcome = CaptureOutcome::Done; return true; }
            ToolbarCmd::Translate => { if let Some((w, h, buf, _)) = self.export_sel() { let _ = handoff_to_ocr_or_translate(w, h, &buf, true); } self.outcome = CaptureOutcome::Done; return true; }
            ToolbarCmd::ScrollLong => { if let Some((w, h, buf, _)) = self.export_sel() { let _ = save_bgra_as_png(&default_save_path(&self.save_dir), w, h, &buf); } self.outcome = CaptureOutcome::Done; return true; }
        }
        unsafe { tomoyo_cap_win_redraw(); }
        false
    }
}

fn cursor_kind_for_handle(idx: usize) -> c_int {
    match idx { 0 | 7 => 3, 2 | 5 => 4, 1 | 6 => 5, 3 | 4 => 6, _ => 2 }
}

unsafe extern "C" fn on_draw(ctx: *mut c_void, cr: *mut c_void, ww: c_int, wh: c_int) {
    let st = unsafe { &mut *(ctx as *mut OverlayState) };
    st.win_dims = (ww.max(1), wh.max(1));
    let show_loupe = st.sel.is_none() || matches!(st.drag, Drag::Creating(..) | Drag::Resizing(..));
    let hud_str = st.wheel_hud.as_ref().filter(|(_, exp)| std::time::Instant::now() < *exp).map(|(s, _)| s.as_str());
    let rec_first_hint = st.record_open && !st.has_monitor_token && st.layout.sub_bar.is_some();
    compose_overlay_pixels_ex(
        &mut st.frame_buf, &st.snap, &st.dimmed, st.sel, st.hover_snap.map(|c| c.rect),
        st.ann_stack.items(), st.ann_stack.selected, st.hovered_annot, st.live_ann.as_ref(),
        st.cursor, show_loupe, &st.layout, st.hovered_cmd, hud_str, rec_first_hint,
    );
    unsafe { tomoyo_cap_cairo_blit_bgra(cr, st.frame_buf.as_ptr(), st.snap.width, st.snap.height, ww, wh); }
    let sx = ww as f64 / st.snap.width.max(1) as f64;
    let sy = wh as f64 / st.snap.height.max(1) as f64;
    let fnt = CString::new("阿里巴巴普惠体 3.0 55 Regular 12px").unwrap();
    let sm_fnt = CString::new("阿里巴巴普惠体 3.0 55 Regular 10.5px").unwrap();

    if let Some(r) = st.sel.filter(|s| s.is_valid()) {
        let db = dim_badge_rect(r, st.snap.width);
        let badge = if st.record_open { format!("录制 {} × {}", r.w, r.h) } else { format!("{} × {}", r.w, r.h) };
        draw_txt(cr, &fnt, 0xF3F5FA, 1, &badge, [db.x as f64 * sx, db.y as f64 * sy, db.w as f64 * sx, db.h as f64 * sy]);
    } else if let Some(h) = st.hover_snap {
        let sb = snap_badge_rect(h.rect, st.snap.width);
        let msg = if st.snap_ctx.cycle_count <= 1 {
            format!("{} {}×{} · 拖拽框选 / 单击全屏", h.kind.label(), h.rect.w, h.rect.h)
        } else {
            format!("{} {}×{} · 单击选中 · Tab切层", h.kind.label(), h.rect.w, h.rect.h)
        };
        draw_txt(cr, &fnt, 0xE0F2FE, 1, &msg, [sb.x as f64 * sx, sb.y as f64 * sy, sb.w as f64 * sx, sb.h as f64 * sy]);
    }
    if show_loupe {
        let lr = loupe_rect(st.cursor, st.snap.width, st.snap.height);
        let (r, g, b) = st.snap.pixel_rgb(st.cursor.0, st.cursor.1);
        let ctxt = if st.rgb_mode { format!("rgb({r},{g},{b})") }
            else if st.hex_lower { format!("#{r:02x}{g:02x}{b:02x}") }
            else { format!("#{r:02X}{g:02X}{b:02X}") };
        draw_txt(cr, &sm_fnt, 0xF8FAFC, 0, &ctxt, [(lr.x + 33) as f64 * sx, (lr.y + 148) as f64 * sy, 124.0 * sx, 18.0 * sy]);
        draw_txt(cr, &sm_fnt, 0x94A3B8, 0, &format!("({},{}) Shift/C", st.cursor.0, st.cursor.1), [(lr.x + 12) as f64 * sx, (lr.y + 168) as f64 * sy, 144.0 * sx, 16.0 * sy]);
    }
    for ann in st.ann_stack.items() {
        if let Annotation::Text { x, y, text, size, color } = ann {
            let rgb = ((color.0 as u32) << 16) | ((color.1 as u32) << 8) | (color.2 as u32);
            let tf = CString::new(format!("阿里巴巴普惠体 3.0 55 Regular {}px", (*size).max(12))).unwrap();
            draw_txt(cr, &tf, rgb, 0, text, [*x as f64 * sx, *y as f64 * sy, 480.0 * sx, 60.0 * sy]);
        }
    }
    for b in &st.layout.hits {
        if !b.pill_text.is_empty() {
            let rgb = if b.danger { 0xFFFFFF } else { 0xF3F5FA };
            draw_txt(cr, &sm_fnt, rgb, 1, b.pill_text, [b.rect.x as f64 * sx, b.rect.y as f64 * sy, b.rect.w as f64 * sx, b.rect.h as f64 * sy]);
        }
    }
    if let Some(cmd) = st.hovered_cmd
        && let Some(btn) = st.layout.hits.iter().find(|b| b.cmd == cmd)
        && let Some(tr) = tooltip_rect_for(btn, st.snap.width, st.snap.height)
    {
        draw_txt(cr, &fnt, 0xF3F5FA, 1, &btn.tooltip, [tr.x as f64 * sx, tr.y as f64 * sy, tr.w as f64 * sx, tr.h as f64 * sy]);
    }
    if let Some(msg) = hud_str {
        let wr = wheel_hud_rect(st.cursor, st.snap.width, st.snap.height);
        draw_txt(cr, &fnt, 0x93C5FD, 1, msg, [wr.x as f64 * sx, wr.y as f64 * sy, wr.w as f64 * sx, wr.h as f64 * sy]);
    }
    if rec_first_hint && let Some(sr) = st.layout.sub_bar {
        let hr = rec_hint_rect(sr, st.snap.width, st.snap.height);
        draw_txt(
            cr, &sm_fnt, 0xFCD34D, 1,
            "提示：首次录屏请在系统弹窗选「整个屏幕」，无需重复框选",
            [hr.x as f64 * sx, hr.y as f64 * sy, hr.w as f64 * sx, hr.h as f64 * sy],
        );
    }
}

fn draw_txt(cr: *mut c_void, fnt: &CString, rgb: u32, align: c_int, txt: &str, r: [f64; 4]) {
    if let Ok(cs) = CString::new(txt) {
        unsafe { tomoyo_cap_pango_draw(cr, fnt.as_ptr(), rgb, align, 0, cs.as_ptr(), r[0], r[1], r[2], r[3]); }
    }
}

pub fn run_capture_overlay(mode: CaptureMode) -> Result<CaptureOutcome, String> {
    let snap = capture_virtual_desktop()?;
    let (mut snap_ctx, init_cursor) = build_linux_snap_context(&snap);
    let hover_snap = snap_ctx.hover_at(&snap, init_cursor.0, init_cursor.1);
    let dimmed = precompute_dimmed(&snap.bgra);
    let cfg = TomoyoConfig::load_or_create();
    let frame_buf = vec![0u8; snap.bgra.len()];
    let (w, h) = (snap.width, snap.height);
    let mut state = Box::new(OverlayState {
        mode,
        snap,
        snap_ctx,
        dimmed,
        frame_buf,
        sel: None,
        hover_snap,
        drag: Drag::None,
        cursor: init_cursor,
        tool: ToolKind::Select,
        color_idx: 0,
        thickness: 4,
        step_radius: 13,
        text_size: 20,
        shape_filled: false,
        mosaic_blur: false,
        hex_lower: cfg.capture.copy_hex_lowercase,
        rgb_mode: false,
        record_open: mode == CaptureMode::Record,
        rec_is_gif: cfg.capture.record_format.eq_ignore_ascii_case("gif"),
        rec_fps: cfg.capture.record_fps,
        rec_audio: match cfg.capture.record_audio.as_str() { "system" => 1, "mic" => 2, "both" => 3, _ => 0 },
        rec_cursor: cfg.capture.record_cursor,
        has_monitor_token: has_valid_monitor_token(),
        ann_stack: AnnotationStack::default(),
        live_ann: None,
        pen_pts: Vec::new(),
        hovered_annot: None,
        hovered_ctrl: None,
        wheel_hud: None,
        layout: ToolbarLayout::default(),
        hovered_cmd: None,
        save_dir: cfg.capture.save_dir,
        hotkeys: cfg.capture.tool_hotkeys,
        outcome: CaptureOutcome::Done,
        win_dims: (w.max(1), h.max(1)),
    });
    let ptr = &mut *state as *mut OverlayState as *mut c_void;
    unsafe { tomoyo_cap_win_run(w, h, 1, 0, 0, ptr, on_draw, on_key_cb, on_mouse_cb); }
    Ok(state.outcome)
}
