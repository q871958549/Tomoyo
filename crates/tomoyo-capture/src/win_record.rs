//! Native Windows region screen recording coordinator (`tomoyo-capture.exe`).
//!
//! Captures the chosen physical-pixel rectangle via a reusable GDI DIB (`+0` GDI leaks) with
//! optional cursor compositing, encodes MP4 (Media Foundation H.264 + WASAPI AAC) or GIF89a
//! (pure-Rust Median-Cut + dirty-rect LZW), and drives the floating `WDA_EXCLUDEFROMCAPTURE` HUD.

#![cfg(windows)]

use crate::export::default_save_dir;
use crate::screen::ScreenRect;
use crate::win_record_audio::WinAudioMixer;
use crate::win_record_gif::GifEncoder;
use crate::win_record_hud::run_recording_hud_loop;
use crate::win_record_hud_draw::copy_file_to_clipboard;
use crate::win_record_mf::MfMp4Writer;
use std::ffi::c_void;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HBITMAP, HDC,
    SRCCOPY,
};
use windows::Win32::System::Threading::{
    CreateEventW, OpenEventW, SetEvent, EVENT_MODIFY_STATE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, GetCursorInfo, GetCursorPos, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL,
    ICONINFO,
};

const STOP_EVENT_NAME: windows::core::PCWSTR = w!("Local\\TomoyoCaptureStopRec");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RecFormat {
    Mp4,
    Gif,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RecAudio {
    None,
    System,
    Mic,
    Both,
}

pub struct RecordRequest {
    pub screen_x: i32,
    pub screen_y: i32,
    pub width: i32,
    pub height: i32,
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
    pub stop_requested: AtomicBool,
    pub discard_on_stop: AtomicBool,
    pub final_path: Mutex<Option<PathBuf>>,
    pub status_note: Mutex<Option<String>>,
    pub copied_flash: AtomicBool,
}

/// If another `tomoyo-capture.exe` recording session is active, signals it to finish & save.
pub fn try_stop_active_recording() -> bool {
    unsafe {
        if let Ok(h) = OpenEventW(EVENT_MODIFY_STATE, false, STOP_EVENT_NAME)
            && !h.is_invalid()
        {
            let _ = SetEvent(h);
            let _ = CloseHandle(h);
            return true;
        }
    }
    false
}

struct ScreenGrabber {
    screen_dc: HDC,
    mem_dc: HDC,
    dib_bmp: HBITMAP,
    dib_bits: *mut u8,
    old_obj: windows::Win32::Graphics::Gdi::HGDIOBJ,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    prev_lbtn: bool,
    prev_rbtn: bool,
    ripples: Vec<(i32, i32, Instant, bool)>,
}

impl ScreenGrabber {
    fn new(x: i32, y: i32, w: i32, h: i32) -> Result<Self, String> {
        unsafe {
            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return Err("获取屏幕 DC 失败".into());
            }
            let mem_dc = CreateCompatibleDC(Some(screen_dc));
            if mem_dc.is_invalid() {
                let _ = ReleaseDC(None, screen_dc);
                return Err("创建兼容 DC 失败".into());
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down BGRA
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let dib_bmp = match CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(b) if !b.is_invalid() && !bits.is_null() => b,
                _ => {
                    let _ = DeleteDC(mem_dc);
                    let _ = ReleaseDC(None, screen_dc);
                    return Err("创建录屏 DIB 缓冲失败".into());
                }
            };
            let old_obj = SelectObject(mem_dc, dib_bmp.into());
            Ok(Self {
                screen_dc,
                mem_dc,
                dib_bmp,
                dib_bits: bits as *mut u8,
                old_obj,
                x,
                y,
                w,
                h,
                prev_lbtn: false,
                prev_rbtn: false,
                ripples: Vec::with_capacity(4),
            })
        }
    }

    fn grab(&mut self, include_cursor: bool) -> &[u8] {
        unsafe {
            let _ = BitBlt(
                self.mem_dc,
                0,
                0,
                self.w,
                self.h,
                Some(self.screen_dc),
                self.x,
                self.y,
                SRCCOPY | CAPTUREBLT,
            );
            let slice =
                std::slice::from_raw_parts_mut(self.dib_bits, (self.w as usize) * (self.h as usize) * 4);
            if include_cursor {
                self.sample_and_draw_click_ripples(slice);
                draw_cursor_onto_dc(self.mem_dc, self.x, self.y);
            }
            slice
        }
    }

    fn sample_and_draw_click_ripples(&mut self, bgra: &mut [u8]) {
        let now = Instant::now();
        let ldown = unsafe { (GetAsyncKeyState(i32::from(VK_LBUTTON.0)) as u16 & 0x8000) != 0 };
        let rdown = unsafe { (GetAsyncKeyState(i32::from(VK_RBUTTON.0)) as u16 & 0x8000) != 0 };
        if (ldown && !self.prev_lbtn) || (rdown && !self.prev_rbtn) {
            let mut pt = windows::Win32::Foundation::POINT::default();
            if unsafe { GetCursorPos(&mut pt) }.is_ok() {
                let (lx, ly) = (pt.x - self.x, pt.y - self.y);
                if lx >= -24 && lx < self.w + 24 && ly >= -24 && ly < self.h + 24 {
                    self.ripples.push((lx, ly, now, rdown && !self.prev_rbtn));
                }
            }
        }
        self.prev_lbtn = ldown;
        self.prev_rbtn = rdown;
        self.ripples.retain(|&(_, _, t, _)| now.saturating_duration_since(t).as_millis() < 320);
        for &(cx, cy, t, is_right) in &self.ripples {
            let prog = (now.saturating_duration_since(t).as_millis() as f32 / 320.0).clamp(0.0, 1.0);
            let radius = 6.0 + prog * 18.0;
            let alpha = (1.0 - prog) * 0.78;
            let (cr, cg, cb) = if is_right { (245.0, 158.0, 11.0) } else { (37.0, 99.0, 235.0) };
            let r_i = radius.ceil() as i32 + 2;
            let (y0, y1) = ((cy - r_i).max(0), (cy + r_i).min(self.h - 1));
            let (x0, x1) = ((cx - r_i).max(0), (cx + r_i).min(self.w - 1));
            for py in y0..=y1 {
                let dy = (py - cy) as f32;
                for px in x0..=x1 {
                    let dist = ((px - cx) as f32).hypot(dy);
                    let ring = (1.0 - (dist - radius).abs() / 2.2).clamp(0.0, 1.0);
                    let a = ring * alpha;
                    if a > 0.02 {
                        let off = ((py as usize) * (self.w as usize) + (px as usize)) * 4;
                        let inv = 1.0 - a;
                        bgra[off] = (bgra[off] as f32 * inv + cb * a) as u8;
                        bgra[off + 1] = (bgra[off + 1] as f32 * inv + cg * a) as u8;
                        bgra[off + 2] = (bgra[off + 2] as f32 * inv + cr * a) as u8;
                    }
                }
            }
        }
    }
}

impl Drop for ScreenGrabber {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.mem_dc, self.old_obj);
            let _ = DeleteObject(self.dib_bmp.into());
            let _ = DeleteDC(self.mem_dc);
            let _ = ReleaseDC(None, self.screen_dc);
        }
    }
}

unsafe fn draw_cursor_onto_dc(dc: HDC, origin_x: i32, origin_y: i32) {
    let mut ci = CURSORINFO {
        cbSize: std::mem::size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetCursorInfo(&mut ci) }.is_err()
        || (ci.flags.0 & CURSOR_SHOWING.0) == 0
        || ci.hCursor.is_invalid()
    {
        return;
    }
    let mut ii = ICONINFO::default();
    let (hx, hy) = if unsafe { GetIconInfo(ci.hCursor.into(), &mut ii) }.is_ok() {
        let hot = (ii.xHotspot as i32, ii.yHotspot as i32);
        unsafe {
            if !ii.hbmMask.is_invalid() { let _ = DeleteObject(ii.hbmMask.into()); }
            if !ii.hbmColor.is_invalid() { let _ = DeleteObject(ii.hbmColor.into()); }
        }
        hot
    } else {
        (0, 0)
    };
    let dx = ci.ptScreenPos.x - origin_x - hx;
    let dy = ci.ptScreenPos.y - origin_y - hy;
    let _ = unsafe { DrawIconEx(dc, dx, dy, ci.hCursor.into(), 0, 0, 0, None, DI_NORMAL) };
}

pub fn run_screen_recording(mut req: RecordRequest) {
    req.width = (req.width & !1).max(2);
    req.height = (req.height & !1).max(2);
    req.fps = match req.fps {
        0..=20 => 15,
        21..=45 => 30,
        _ => 60,
    };

    let stop_event: HANDLE = unsafe {
        CreateEventW(None, true, false, STOP_EVENT_NAME).unwrap_or(HANDLE::default())
    };
    let shared = Arc::new(SharedRecState {
        mode: AtomicU8::new(0),
        frames: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        stop_requested: AtomicBool::new(false),
        discard_on_stop: AtomicBool::new(false),
        final_path: Mutex::new(None),
        status_note: Mutex::new(None),
        copied_flash: AtomicBool::new(false),
    });

    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let ext = if req.format == RecFormat::Gif { "gif" } else { "mp4" };
    let out_path = default_save_dir(&req.save_dir).join(format!("Tomoyo_{ts}.{ext}"));
    let rect = ScreenRect { x: req.screen_x, y: req.screen_y, w: req.width, h: req.height };

    let sh_bg = Arc::clone(&shared);
    let path_bg = out_path.clone();
    let format = req.format;
    let fps = req.fps;
    let audio = req.audio;
    let include_cursor = req.include_cursor;

    let worker = std::thread::Builder::new()
        .name("tomoyo-win-rec".into())
        .spawn(move || {
            let res = match format {
                RecFormat::Mp4 => record_mp4_loop(rect, fps, audio, include_cursor, &path_bg, &sh_bg),
                RecFormat::Gif => record_gif_loop(rect, fps, include_cursor, &path_bg, &sh_bg),
            };
            if sh_bg.discard_on_stop.load(Ordering::Acquire) {
                let _ = std::fs::remove_file(&path_bg);
                sh_bg.mode.store(4, Ordering::Release);
                return;
            }
            match res {
                Ok(sz) => {
                    sh_bg.bytes.store(sz, Ordering::Relaxed);
                    if copy_file_to_clipboard(&path_bg).is_ok() {
                        sh_bg.copied_flash.store(true, Ordering::Relaxed);
                    }
                    if let Ok(mut fp) = sh_bg.final_path.lock() {
                        *fp = Some(path_bg);
                    }
                    sh_bg.mode.store(3, Ordering::Release);
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&path_bg);
                    if let Ok(mut note) = sh_bg.status_note.lock() {
                        *note = Some(e);
                    }
                    sh_bg.mode.store(4, Ordering::Release);
                }
            }
        })
        .ok();

    run_recording_hud_loop(rect, shared, worker, stop_event);

    if !stop_event.is_invalid() {
        unsafe { let _ = CloseHandle(stop_event); }
    }
}

fn record_mp4_loop(
    rect: ScreenRect,
    fps: u8,
    audio_mode: RecAudio,
    include_cursor: bool,
    out_path: &Path,
    shared: &Arc<SharedRecState>,
) -> Result<u64, String> {
    let mut grabber = ScreenGrabber::new(rect.x, rect.y, rect.w, rect.h)?;
    let mut audio_cap = match audio_mode {
        RecAudio::None => None,
        RecAudio::System => WinAudioMixer::start(true, false),
        RecAudio::Mic => WinAudioMixer::start(false, true),
        RecAudio::Both => WinAudioMixer::start(true, true),
    };
    let audio_rate = audio_cap.as_ref().map(WinAudioMixer::sample_rate);
    let mut writer = MfMp4Writer::create(out_path, rect.w as u32, rect.h as u32, fps, audio_rate)?;
    if !writer.has_audio() {
        audio_cap = None;
    }

    let frame_period = Duration::from_nanos(1_000_000_000 / (fps as u64).max(1));
    let frame_hns = 10_000_000i64 / (fps as i64).max(1);
    let mut next_tick = Instant::now();
    let mut active_hns = 0i64;
    let mut audio_written_frames = 0i64;
    let mut pcm_buf: Vec<i16> = Vec::with_capacity(4096);
    let mut stat_tick = 0u32;

    while !shared.stop_requested.load(Ordering::Acquire) {
        let paused = shared.mode.load(Ordering::Acquire) == 1;
        if let Some(ac) = audio_cap.as_mut() {
            pcm_buf.clear();
            ac.drain_mixed_pcm16(&mut pcm_buf, paused);
            if !paused && !pcm_buf.is_empty() {
                let rate = ac.sample_rate();
                let ts_hns = (audio_written_frames * 10_000_000) / (rate as i64);
                let _ = writer.write_audio_pcm16(&pcm_buf, ts_hns, rate);
                audio_written_frames += (pcm_buf.len() / 2) as i64;
            }
        }
        if !paused {
            let bgra = grabber.grab(include_cursor);
            writer.write_video_frame(bgra, (rect.w as usize) * 4, active_hns, frame_hns)?;
            active_hns += frame_hns;
            shared.frames.fetch_add(1, Ordering::Relaxed);
            stat_tick += 1;
            if stat_tick.is_multiple_of(12)
                && let Ok(meta) = std::fs::metadata(out_path)
            {
                shared.bytes.store(meta.len(), Ordering::Relaxed);
            }
        }
        next_tick += frame_period;
        let now = Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else if now.saturating_duration_since(next_tick) > frame_period * 4 {
            next_tick = now;
        }
    }

    if shared.discard_on_stop.load(Ordering::Acquire) {
        drop(writer);
        return Ok(0);
    }
    writer.finish()
}

fn record_gif_loop(
    rect: ScreenRect,
    fps: u8,
    include_cursor: bool,
    out_path: &Path,
    shared: &Arc<SharedRecState>,
) -> Result<u64, String> {
    let mut grabber = ScreenGrabber::new(rect.x, rect.y, rect.w, rect.h)?;
    let mut enc = GifEncoder::create(out_path, rect.w as u32, rect.h as u32)?;
    let eff_fps = fps.clamp(10, 30); // GIF centisecond timing caps smooth playback at <= 30fps
    let delay_cs = ((100u32 + eff_fps as u32 / 2) / eff_fps as u32).max(3) as u16;
    let frame_period = Duration::from_millis((delay_cs as u64) * 10);
    let mut next_tick = Instant::now();

    while !shared.stop_requested.load(Ordering::Acquire) {
        let paused = shared.mode.load(Ordering::Acquire) == 1;
        if !paused {
            let bgra = grabber.grab(include_cursor);
            enc.push_bgra_frame(bgra, delay_cs)?;
            shared.frames.fetch_add(1, Ordering::Relaxed);
            shared.bytes.store(enc.bytes_written(), Ordering::Relaxed);
        }
        next_tick += frame_period;
        let now = Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else if now.saturating_duration_since(next_tick) > frame_period * 4 {
            next_tick = now;
        }
    }

    if shared.discard_on_stop.load(Ordering::Acquire) {
        drop(enc);
        return Ok(0);
    }
    enc.finish()
}
