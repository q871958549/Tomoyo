//! Native Linux Wayland (`libpipewire-0.3`) & X11 screen recording engine (MP4 H.264 & crisp GIF).

use crate::linux_rec_hud::run_recording_hud_loop;
pub use crate::linux_record_export::concat_and_export_recording;
use crate::linux_record_export::{close_ffmpeg, spawn_ffmpeg_segment, update_bytes_stat, SegmentCfg};
use crate::screen::ScreenRect;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tomoyo_core::logs_dir;

unsafe extern "C" {
    fn tomoyo_cap_portal_screencast(
        cursor_mode: c_int, restore_tok: *const c_char,
        out_node: *mut u32, out_w: *mut c_int, out_h: *mut c_int,
        out_tok: *mut *mut c_char, out_err: *mut *mut c_char,
    ) -> c_int;
    fn tomoyo_cap_portal_close_session();
    fn tomoyo_cap_rec_borders_show(x: c_int, y: c_int, w: c_int, h: c_int, sw: c_int, sh: c_int);
    fn tomoyo_cap_rec_borders_hide();
    fn tomoyo_cap_install_rec_signals();
    fn tomoyo_cap_pid_alive(pid: c_int) -> c_int;
    fn tomoyo_cap_send_usr1(pid: c_int);
    fn tomoyo_cap_free(p: *mut c_void);
    fn tomoyo_pw_rec_start(pw_fd: c_int, node_id: u32, cx: c_int, cy: c_int, cw: c_int, ch: c_int, fps: c_int) -> *mut c_void;
    fn tomoyo_pw_rec_wait_first(h: *mut c_void, timeout_ms: c_int, ow: *mut c_int, oh: *mut c_int, is_rgb0: *mut c_int) -> c_int;
    fn tomoyo_pw_rec_copy_latest(h: *mut c_void, dst: *mut u8, dst_len: usize) -> c_int;
    fn tomoyo_pw_rec_error(h: *mut c_void) -> *const c_char;
    fn tomoyo_pw_rec_stop(h: *mut c_void);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RecFormat { Mp4, Gif }
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RecAudio { None, System, Mic }

pub struct RecordRequest {
    pub rect: ScreenRect,
    pub snap_w: i32,
    pub snap_h: i32,
    pub monitors: Vec<ScreenRect>,
    pub format: RecFormat,
    pub fps: u8,
    pub audio: RecAudio,
    pub include_cursor: bool,
    pub save_dir: String,
}

pub(crate) struct SharedRecState {
    pub mode: AtomicU8, // 0=rec, 1=paused, 2=saving, 3=done, 4=error/cancelled
    pub frames: AtomicU64,
    pub bytes: AtomicU64,
    pub aborted: AtomicBool,
    pub final_path: Mutex<Option<PathBuf>>,
    pub status_note: Mutex<Option<String>>,
    pub copied_flash: AtomicBool,
}

fn take_cstr(p: *mut c_char) -> Option<String> {
    if p.is_null() { return None; }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { tomoyo_cap_free(p.cast()); }
    Some(s)
}

fn pid_file_path() -> PathBuf { logs_dir().join("active_rec.pid") }

struct ActivePidGuard(PathBuf);
impl Drop for ActivePidGuard {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

pub fn try_stop_active_recording() -> bool {
    let Ok(raw) = std::fs::read_to_string(pid_file_path()) else { return false; };
    let Ok(pid) = raw.trim().parse::<i32>() else { return false; };
    if unsafe { tomoyo_cap_pid_alive(pid) } != 0 {
        unsafe { tomoyo_cap_send_usr1(pid); }
        return true;
    }
    let _ = std::fs::remove_file(pid_file_path());
    false
}

pub fn has_valid_monitor_token() -> bool {
    std::fs::read_to_string(logs_dir().join("screencast_token"))
        .map(|s| s.trim().starts_with("v2:"))
        .unwrap_or(false)
}

fn load_matching_token(snap_w: i32, snap_h: i32, monitors: &[ScreenRect]) -> String {
    let tok_path = logs_dir().join("screencast_token");
    let raw = std::fs::read_to_string(&tok_path).unwrap_or_default();
    let trimmed = raw.trim();
    let Some(rest) = trimmed.strip_prefix("v2:") else {
        if !trimmed.is_empty() { let _ = std::fs::remove_file(&tok_path); }
        return String::new();
    };
    let Some((dims, tok)) = rest.split_once(':') else { return String::new(); };
    let Some((ws, hs)) = dims.split_once('x') else { return String::new(); };
    let (Ok(tw), Ok(th)) = (ws.parse::<i32>(), hs.parse::<i32>()) else { return String::new(); };
    let matches_desktop = (tw - snap_w).abs() <= 4 && (th - snap_h).abs() <= 4;
    let matches_mon = monitors.iter().any(|m| (m.w - tw).abs() <= 4 && (m.h - th).abs() <= 4);
    if (matches_desktop || matches_mon) && !tok.is_empty() { tok.to_string() } else { String::new() }
}

fn clamp_even_crop(r: ScreenRect, max_w: i32, max_h: i32) -> ScreenRect {
    let cw = (r.w & !1).clamp(16, (max_w & !1).max(16));
    let ch = (r.h & !1).clamp(16, (max_h & !1).max(16));
    ScreenRect { x: r.x.clamp(0, (max_w - cw).max(0)), y: r.y.clamp(0, (max_h - ch).max(0)), w: cw, h: ch }
}

pub(crate) fn resolve_portal_crop(
    rect: ScreenRect, snap_w: i32, snap_h: i32, sw: i32, sh: i32, monitors: &[ScreenRect],
) -> (ScreenRect, Option<ScreenRect>, bool) {
    if (sw - snap_w).abs() <= 4 && (sh - snap_h).abs() <= 4 {
        return (clamp_even_crop(rect, sw, sh), Some(rect), true);
    }
    let cx = rect.x + rect.w / 2;
    let cy = rect.y + rect.h / 2;
    if let Some(m) = monitors.iter().find(|m| (m.w - sw).abs() <= 4 && (m.h - sh).abs() <= 4 && m.contains(cx, cy)) {
        let local = ScreenRect { x: rect.x - m.x, y: rect.y - m.y, w: rect.w, h: rect.h };
        return (clamp_even_crop(local, sw, sh), Some(rect), true);
    }
    let full = ScreenRect { x: 0, y: 0, w: (sw & !1).max(16), h: (sh & !1).max(16) };
    (full, None, false)
}

fn pulse_source_for(audio: RecAudio) -> Option<String> {
    let arg = match audio {
        RecAudio::None => return None,
        RecAudio::System => "get-default-sink",
        RecAudio::Mic => "get-default-source",
    };
    let out = Command::new("pactl").arg(arg).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { return None; }
    Some(if audio == RecAudio::System { format!("{s}.monitor") } else { s })
}

pub fn run_screen_recording(mut req: RecordRequest) -> Result<(), String> {
    unsafe { tomoyo_cap_install_rec_signals(); }
    let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    let _ = std::fs::create_dir_all(logs_dir());
    let _ = std::fs::write(pid_file_path(), std::process::id().to_string());
    let _pid_guard = ActivePidGuard(pid_file_path());

    let tmp_dir = std::env::temp_dir().join(format!("tomoyo_rec_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let prev_tok = load_matching_token(req.snap_w, req.snap_h, &req.monitors);

    let (pw_fd, node_id, stream_w, stream_h, tok_opt) = if has_wayland {
        let mut node = 0u32;
        let (mut sw, mut sh) = (req.snap_w, req.snap_h);
        let mut new_tok: *mut c_char = std::ptr::null_mut();
        let mut err_p: *mut c_char = std::ptr::null_mut();
        let c_tok = CString::new(prev_tok.as_str()).unwrap_or_default();
        let fd = unsafe {
            tomoyo_cap_portal_screencast(
                c_int::from(req.include_cursor),
                if prev_tok.is_empty() { std::ptr::null() } else { c_tok.as_ptr() },
                &mut node, &mut sw, &mut sh, &mut new_tok, &mut err_p,
            )
        };
        let ntok = take_cstr(new_tok).filter(|s| !s.is_empty());
        if fd < 0 || node == 0 {
            let msg = take_cstr(err_p).unwrap_or_else(|| "Wayland ScreenCast 启动失败".into());
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(msg);
        }
        let _ = take_cstr(err_p);
        (fd, node, sw.max(16), sh.max(16), ntok)
    } else {
        (-1, 0, req.snap_w.max(16), req.snap_h.max(16), None)
    };

    let (crop, border_rect, should_save_tok) =
        resolve_portal_crop(req.rect, req.snap_w, req.snap_h, stream_w, stream_h, &req.monitors);
    if should_save_tok && let Some(t) = tok_opt {
        let _ = std::fs::write(logs_dir().join("screencast_token"), format!("v2:{stream_w}x{stream_h}:{t}"));
    } else if !should_save_tok {
        let _ = std::fs::remove_file(logs_dir().join("screencast_token"));
    }
    req.rect = border_rect.unwrap_or(crop);

    let audio_src = if req.format == RecFormat::Mp4 { pulse_source_for(req.audio) } else { None };
    let shared = Arc::new(SharedRecState {
        mode: AtomicU8::new(0),
        frames: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        aborted: AtomicBool::new(false),
        final_path: Mutex::new(None),
        status_note: Mutex::new(None),
        copied_flash: AtomicBool::new(false),
    });
    let segments: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));

    let (hud_w, hud_h) = (412, 46);
    let hud_x = (req.rect.x + (req.rect.w - hud_w) / 2).clamp(12, (req.snap_w - hud_w - 12).max(12));
    let hud_y = if req.rect.y >= hud_h + 18 { req.rect.y - hud_h - 14 }
        else if req.rect.bottom() + hud_h + 18 <= req.snap_h { req.rect.bottom() + 14 }
        else { 16 };

    if let Some(br) = border_rect {
        unsafe { tomoyo_cap_rec_borders_show(br.x, br.y, br.w, br.h, req.snap_w, req.snap_h); }
    }

    let sh_bg = Arc::clone(&shared);
    let seg_bg = Arc::clone(&segments);
    let dir_bg = tmp_dir.clone();
    let (fps, cur) = (req.fps, req.include_cursor);
    let pump = std::thread::Builder::new().name("tomoyo-rec-pump".into()).spawn(move || {
        if has_wayland {
            run_wayland_pump(pw_fd, node_id, crop, fps, audio_src.as_deref(), &dir_bg, &sh_bg, &seg_bg);
        } else {
            let scfg = SegmentCfg {
                w: crop.w, h: crop.h, fps, pixel_fmt: "bgr0",
                audio_src: audio_src.as_deref(), x11_origin: Some((crop.x, crop.y)), cursor: cur,
            };
            run_x11_pump(&scfg, &dir_bg, &sh_bg, &seg_bg);
        }
    }).ok();

    run_recording_hud_loop(req, Arc::clone(&shared), pump, tmp_dir.clone(), segments, hud_x, hud_y);
    unsafe {
        tomoyo_cap_rec_borders_hide();
        tomoyo_cap_portal_close_session();
    }
    if shared.mode.load(Ordering::Acquire) != 3 {
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_wayland_pump(
    pw_fd: c_int, node_id: u32, crop: ScreenRect, fps: u8, audio_src: Option<&str>,
    tmp_dir: &Path, shared: &Arc<SharedRecState>, segments: &Arc<Mutex<Vec<PathBuf>>>,
) {
    let pw = unsafe { tomoyo_pw_rec_start(pw_fd, node_id, crop.x, crop.y, crop.w, crop.h, i32::from(fps)) };
    if pw.is_null() {
        fail_pump(shared, "PipeWire 流初始化失败".into());
        return;
    }
    let (mut aw, mut ah, mut is_rgb0) = (0 as c_int, 0 as c_int, 0 as c_int);
    let got = unsafe { tomoyo_pw_rec_wait_first(pw, 4000, &mut aw, &mut ah, &mut is_rgb0) };
    if got == 0 || aw < 16 || ah < 16 {
        let msg = unsafe {
            let ep = tomoyo_pw_rec_error(pw);
            if ep.is_null() { "未收到桌面画面帧，请重试录屏".into() } else { CStr::from_ptr(ep).to_string_lossy().into_owned() }
        };
        unsafe { tomoyo_pw_rec_stop(pw); }
        fail_pump(shared, msg);
        return;
    }

    let cfg = SegmentCfg {
        w: aw, h: ah, fps, pixel_fmt: if is_rgb0 != 0 { "rgb0" } else { "bgr0" },
        audio_src, x11_origin: None, cursor: false,
    };
    let mut frame = vec![0u8; (aw as usize) * (ah as usize) * 4];
    let mut enc: Option<Child> = None;
    let mut seg_idx = 0usize;
    let step = Duration::from_secs_f64(1.0 / f64::from(fps.max(1)));
    let mut next_tick = Instant::now();

    while !shared.aborted.load(Ordering::Acquire) && shared.mode.load(Ordering::Acquire) < 2 {
        if shared.mode.load(Ordering::Acquire) == 1 {
            close_ffmpeg(enc.take(), false);
            std::thread::sleep(Duration::from_millis(50));
            next_tick = Instant::now();
            continue;
        }
        if enc.is_none() {
            let seg = tmp_dir.join(format!("part_{seg_idx}.mkv"));
            seg_idx += 1;
            enc = spawn_ffmpeg_segment(&cfg, &seg);
            if enc.is_some() && let Ok(mut g) = segments.lock() { g.push(seg); }
        }
        if unsafe { tomoyo_pw_rec_copy_latest(pw, frame.as_mut_ptr(), frame.len()) } != 0
            && let Some(ref mut c) = enc
            && let Some(ref mut sin) = c.stdin
        {
            if sin.write_all(&frame).is_err() { break; }
            let fno = shared.frames.fetch_add(1, Ordering::Relaxed) + 1;
            if fno.is_multiple_of(8) { update_bytes_stat(segments, shared); }
        }
        next_tick += step;
        let now = Instant::now();
        if next_tick > now { std::thread::sleep(next_tick - now); }
        else if now.duration_since(next_tick) > step * 4 { next_tick = now; }
    }
    close_ffmpeg(enc.take(), false);
    unsafe { tomoyo_pw_rec_stop(pw); }
}

fn fail_pump(shared: &Arc<SharedRecState>, msg: String) {
    if let Ok(mut n) = shared.status_note.lock() { *n = Some(msg); }
    shared.mode.store(4, Ordering::Release);
}

fn run_x11_pump(
    cfg: &SegmentCfg<'_>, tmp_dir: &Path, shared: &Arc<SharedRecState>, segments: &Arc<Mutex<Vec<PathBuf>>>,
) {
    let mut enc: Option<Child> = None;
    let mut seg_idx = 0usize;
    let step = Duration::from_millis((1000 / u64::from(cfg.fps.max(1))).max(16));
    while !shared.aborted.load(Ordering::Acquire) && shared.mode.load(Ordering::Acquire) < 2 {
        if shared.mode.load(Ordering::Acquire) == 1 {
            close_ffmpeg(enc.take(), true);
            std::thread::sleep(Duration::from_millis(60));
            continue;
        }
        if enc.is_none() {
            let seg = tmp_dir.join(format!("part_{seg_idx}.mkv"));
            seg_idx += 1;
            enc = spawn_ffmpeg_segment(cfg, &seg);
            if enc.is_some() && let Ok(mut g) = segments.lock() { g.push(seg); }
        }
        std::thread::sleep(step);
        let fno = shared.frames.fetch_add(1, Ordering::Relaxed) + 1;
        if fno.is_multiple_of(8) { update_bytes_stat(segments, shared); }
    }
    close_ffmpeg(enc.take(), true);
}
