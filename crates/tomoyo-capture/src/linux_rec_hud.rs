//! Floating Wayland layer-shell / X11 recording control & completion toast HUD.

use crate::annotate::AnnotColor;
use crate::export::default_save_dir;
use crate::linux_clip::copy_file_uri_to_clipboard;
use crate::linux_record::{concat_and_export_recording, RecFormat, RecordRequest, SharedRecState};
use crate::overlay_icons::{draw_capsule, fill_circle_aa, fill_rounded_rect_aa, stroke_rounded_rect_aa};
use crate::screen::ScreenRect;
use std::ffi::{c_char, c_int, c_void, CString};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

unsafe extern "C" {
    fn tomoyo_cap_rec_borders_hide();
    fn tomoyo_cap_rec_hud_run(
        title: *const c_char, w: c_int, h: c_int, x: c_int, y: c_int, sw: c_int, sh: c_int, ctx: *mut c_void,
        on_draw: unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, c_int),
        on_mouse: unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int) -> c_int,
        on_tick: unsafe extern "C" fn(*mut c_void) -> c_int,
    );
    fn tomoyo_cap_rec_hud_resize(w: c_int, h: c_int);
    fn tomoyo_cap_rec_hud_quit();
    fn tomoyo_cap_signal_action() -> c_int;
    fn tomoyo_cap_cairo_blit_bgra(cr: *mut c_void, bgra: *const u8, w: c_int, h: c_int, dw: c_int, dh: c_int);
    fn tomoyo_cap_pango_draw(
        cr: *mut c_void, font: *const c_char, rgb: u32, align: c_int, wrap: c_int,
        text: *const c_char, x: f64, y: f64, w: f64, h: f64,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HudCmd { PauseResume, Finish, Cancel, CopyFile, Play, Reveal }

struct HudState {
    req: RecordRequest,
    shared: Arc<SharedRecState>,
    pump: Option<JoinHandle<()>>,
    tmp_dir: PathBuf,
    segments: Arc<Mutex<Vec<PathBuf>>>,
    win_w: i32,
    win_h: i32,
    buf: Vec<u8>,
    hovered: Option<usize>,
    tick_phase: u32,
    active_ms: u64,
    last_tick: Instant,
    buttons: Vec<(HudCmd, ScreenRect, &'static str, bool, bool)>,
    resized_mode: u8,
}

pub fn run_recording_hud_loop(
    req: RecordRequest,
    shared: Arc<SharedRecState>,
    mut pump: Option<JoinHandle<()>>,
    tmp_dir: PathBuf,
    segments: Arc<Mutex<Vec<PathBuf>>>,
    hud_x: i32,
    hud_y: i32,
) {
    let (hud_w, hud_h) = (412, 46);
    let (sw, sh) = (req.snap_w, req.snap_h);
    let mut hud = Box::new(HudState {
        req,
        shared: Arc::clone(&shared),
        pump: pump.take(),
        tmp_dir,
        segments,
        win_w: hud_w,
        win_h: hud_h,
        buf: vec![0u8; (hud_w * hud_h * 4) as usize],
        hovered: None,
        tick_phase: 0,
        active_ms: 0,
        last_tick: Instant::now(),
        buttons: Vec::new(),
        resized_mode: 0,
    });
    hud.rebuild_buttons();
    let title = CString::new("Tomoyo 录屏控制").unwrap();
    let ptr = &mut *hud as *mut HudState as *mut c_void;
    unsafe {
        tomoyo_cap_rec_hud_run(
            title.as_ptr(), hud_w, hud_h, hud_x, hud_y, sw, sh, ptr, hud_draw_cb, hud_mouse_cb, hud_tick_cb,
        );
        tomoyo_cap_rec_borders_hide();
    }
    shared.aborted.store(true, Ordering::Release);
    if let Some(h) = hud.pump.take() {
        let _ = h.join();
    }
}

impl HudState {
    fn rebuild_buttons(&mut self) {
        self.buttons.clear();
        let m = self.shared.mode.load(Ordering::Acquire);
        if m <= 1 {
            self.buttons.push((HudCmd::PauseResume, ScreenRect { x: self.win_w - 166, y: 9, w: 54, h: 28 }, if m == 1 { "继续" } else { "暂停" }, false, false));
            self.buttons.push((HudCmd::Finish, ScreenRect { x: self.win_w - 106, y: 9, w: 68, h: 28 }, "■ 完成", true, false));
            self.buttons.push((HudCmd::Cancel, ScreenRect { x: self.win_w - 32, y: 9, w: 24, h: 28 }, "✕", false, true));
        } else if m >= 3 {
            if m == 3 {
                self.buttons.push((HudCmd::CopyFile, ScreenRect { x: self.win_w - 224, y: 9, w: 68, h: 28 }, "复制文件", true, false));
                self.buttons.push((HudCmd::Play, ScreenRect { x: self.win_w - 150, y: 9, w: 46, h: 28 }, "播放", false, false));
                self.buttons.push((HudCmd::Reveal, ScreenRect { x: self.win_w - 98, y: 9, w: 60, h: 28 }, "定位目录", false, false));
            }
            self.buttons.push((HudCmd::Cancel, ScreenRect { x: self.win_w - 32, y: 9, w: 24, h: 28 }, "✕", false, true));
        }
    }

    fn start_finalize(&mut self) {
        if self.shared.mode.load(Ordering::Acquire) >= 2 { return; }
        self.shared.mode.store(2, Ordering::Release);
        self.shared.aborted.store(true, Ordering::Release);
        unsafe { tomoyo_cap_rec_borders_hide(); }
        self.rebuild_buttons();
        let pump = self.pump.take();
        let sh = Arc::clone(&self.shared);
        let segs_lock = Arc::clone(&self.segments);
        let tmp = self.tmp_dir.clone();
        let save_dir = self.req.save_dir.clone();
        let is_gif = self.req.format == RecFormat::Gif;
        let fps = self.req.fps;
        let _ = std::thread::Builder::new().name("tomoyo-rec-export".into()).spawn(move || {
            if let Some(h) = pump { let _ = h.join(); }
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let ext = if is_gif { "gif" } else { "mp4" };
            let final_path = default_save_dir(&save_dir).join(format!("Tomoyo_{ts}.{ext}"));
            let segs = segs_lock.lock().map(|g| g.clone()).unwrap_or_default();
            match concat_and_export_recording(&segs, &tmp, &final_path, is_gif, fps) {
                Ok(sz) => {
                    sh.bytes.store(sz, Ordering::Relaxed);
                    if let Ok(mut fp) = sh.final_path.lock() { *fp = Some(final_path.clone()); }
                    sh.mode.store(3, Ordering::Release);
                    let _ = Command::new("notify-send").args(["Tomoyo 录屏完成", &final_path.to_string_lossy()]).spawn();
                }
                Err(e) => {
                    if let Ok(mut n) = sh.status_note.lock() { *n = Some(e); }
                    sh.mode.store(4, Ordering::Release);
                }
            }
            let _ = std::fs::remove_dir_all(&tmp);
        });
    }
}

unsafe extern "C" fn hud_tick_cb(ctx: *mut c_void) -> c_int {
    let st = unsafe { &mut *(ctx as *mut HudState) };
    let now = Instant::now();
    let dt = now.saturating_duration_since(st.last_tick).as_millis() as u64;
    st.last_tick = now;
    st.tick_phase = st.tick_phase.wrapping_add(1);

    match unsafe { tomoyo_cap_signal_action() } {
        1 => st.start_finalize(),
        2 => {
            st.shared.aborted.store(true, Ordering::Release);
            unsafe { tomoyo_cap_rec_borders_hide(); tomoyo_cap_rec_hud_quit(); }
            return 0;
        }
        _ => {}
    }

    let m = st.shared.mode.load(Ordering::Acquire);
    if m == 0 {
        st.active_ms = st.active_ms.saturating_add(dt.min(500));
    }
    if (m == 3 || m == 4) && st.resized_mode != m {
        st.resized_mode = m;
        if m == 4 { unsafe { tomoyo_cap_rec_borders_hide(); } }
        st.win_w = if m == 3 { 496 } else { 440 };
        st.buf.resize((st.win_w * st.win_h * 4) as usize, 0);
        unsafe { tomoyo_cap_rec_hud_resize(st.win_w, st.win_h); }
        st.rebuild_buttons();
    }
    1
}

unsafe extern "C" fn hud_draw_cb(ctx: *mut c_void, cr: *mut c_void, w: c_int, h: c_int) {
    let st = unsafe { &mut *(ctx as *mut HudState) };
    let needed = (w.max(1) as usize) * (h.max(1) as usize) * 4;
    if st.buf.len() != needed { st.buf.resize(needed, 0); }
    st.buf.fill(0);
    let r = ScreenRect { x: 1, y: 1, w: w - 2, h: h - 2 };
    draw_capsule(&mut st.buf, w, h, r, 11.0);
    let m = st.shared.mode.load(Ordering::Acquire);
    let dot_col = match m {
        0 if (st.tick_phase % 6) < 3 => AnnotColor(0xEF, 0x44, 0x44),
        0 => AnnotColor(0xB9, 0x1C, 0x1C),
        1 => AnnotColor(0xF5, 0x9E, 0x0B),
        3 => AnnotColor(0x10, 0xB9, 0x81),
        4 => AnnotColor(0xEF, 0x44, 0x44),
        _ => AnnotColor(0x60, 0xA5, 0xFA),
    };
    fill_circle_aa(&mut st.buf, w, h, 18.0, h as f32 * 0.5, 5.0, dot_col, 1.0);
    for (i, &(_, br, _, accent, danger)) in st.buttons.iter().enumerate() {
        let hov = st.hovered == Some(i);
        if accent {
            fill_rounded_rect_aa(&mut st.buf, w, h, br, 6.0, if hov { AnnotColor(0x1D, 0x4E, 0xD8) } else { AnnotColor(0x25, 0x63, 0xEB) }, 1.0);
        } else if danger {
            fill_rounded_rect_aa(&mut st.buf, w, h, br, 6.0, AnnotColor(0xEF, 0x44, 0x44), if hov { 0.35 } else { 0.16 });
        } else {
            fill_rounded_rect_aa(&mut st.buf, w, h, br, 6.0, AnnotColor(0xFF, 0xFF, 0xFF), if hov { 0.16 } else { 0.08 });
            stroke_rounded_rect_aa(&mut st.buf, w, h, br, 6.0, 1.0, AnnotColor(0x38, 0x3D, 0x4F), 0.9);
        }
    }
    unsafe { tomoyo_cap_cairo_blit_bgra(cr, st.buf.as_ptr(), w, h, w, h); }
    let secs = st.active_ms / 1000;
    let mb = st.shared.bytes.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0);
    let label = match m {
        0 => format!("{:02}:{:02} · {}×{} · {:.1} MB", secs / 60, secs % 60, st.req.rect.w, st.req.rect.h, mb),
        1 => format!("已暂停 {:02}:{:02} · {:.1} MB", secs / 60, secs % 60, mb),
        2 => "正在封装视频...".into(),
        3 if st.shared.copied_flash.load(Ordering::Relaxed) => "✓ 已复制文件！直接 Ctrl+V 粘贴".into(),
        3 => format!("已保存 · {:02}:{:02} · {:.1} MB", secs / 60, secs % 60, mb),
        _ => st.shared.status_note.lock().ok().and_then(|g| g.clone()).unwrap_or_else(|| "录屏已取消".into()),
    };
    let fnt = CString::new("阿里巴巴普惠体 3.0 55 Regular 12px").unwrap();
    let txt_w = match m { 3 => (w - 260).max(80), 4 => (w - 74).max(120), _ => (w - 208).max(80) };
    if let Ok(cs) = CString::new(label) {
        unsafe { tomoyo_cap_pango_draw(cr, fnt.as_ptr(), 0xF8FAFC, 0, 0, cs.as_ptr(), 30.0, 0.0, txt_w as f64, h as f64); }
    }
    for &(_, br, txt, accent, danger) in &st.buttons {
        let rgb = if accent { 0xFFFFFF } else if danger { 0xFCA5A5 } else { 0xE2E8F0 };
        if let Ok(cs) = CString::new(txt) {
            unsafe { tomoyo_cap_pango_draw(cr, fnt.as_ptr(), rgb, 1, 0, cs.as_ptr(), br.x as f64, br.y as f64, br.w as f64, br.h as f64); }
        }
    }
}

unsafe extern "C" fn hud_mouse_cb(ctx: *mut c_void, kind: c_int, x: c_int, y: c_int, btn: c_int) -> c_int {
    let st = unsafe { &mut *(ctx as *mut HudState) };
    let hit = st.buttons.iter().position(|(_, r, _, _, _)| r.contains(x, y));
    if kind == 5 { st.hovered = hit; return 0; }
    if kind == 1 && btn == 1 {
        let Some(idx) = hit else { return 2; };
        match st.buttons[idx].0 {
            HudCmd::PauseResume => {
                let cur = st.shared.mode.load(Ordering::Acquire);
                st.shared.mode.store(if cur == 0 { 1 } else { 0 }, Ordering::Release);
                st.rebuild_buttons();
            }
            HudCmd::Finish => st.start_finalize(),
            HudCmd::Cancel => return 1,
            HudCmd::CopyFile => {
                if let Ok(fp) = st.shared.final_path.lock() && let Some(p) = fp.as_ref() {
                    let _ = copy_file_uri_to_clipboard(p);
                    st.shared.copied_flash.store(true, Ordering::Release);
                }
            }
            HudCmd::Play => {
                if let Ok(fp) = st.shared.final_path.lock() && let Some(p) = fp.as_ref() {
                    let _ = tomoyo_core::linux_launch::spawn_isolated(
                        "xdg-open",
                        &[p.to_string_lossy().into_owned()],
                        None,
                        Some("xdg-open"),
                    );
                }
            }
            HudCmd::Reveal => {
                if let Ok(fp) = st.shared.final_path.lock() && let Some(p) = fp.as_ref()
                    && !tomoyo_core::linux_sys::dbus_reveal_file(&p.to_string_lossy())
                    && let Some(par) = p.parent()
                {
                    let _ = tomoyo_core::linux_launch::spawn_isolated(
                        "xdg-open",
                        &[par.to_string_lossy().into_owned()],
                        None,
                        Some("xdg-open"),
                    );
                }
            }
        }
    }
    0
}
