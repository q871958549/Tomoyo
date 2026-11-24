//! Recording HUD button layout, capsule painting, and Windows `CF_HDROP` clipboard copy.

#![cfg(windows)]

use crate::annotate::AnnotColor;
use crate::overlay_icons::{draw_capsule, fill_circle_aa, fill_rounded_rect_aa, stroke_rounded_rect_aa};
use crate::screen::ScreenRect;
use crate::win_record::SharedRecState;
use std::path::Path;
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, DrawTextW, EndPaint, SelectObject, SetBkMode, SetTextColor, DT_CENTER,
    DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HDC, HFONT, PAINTSTRUCT, SRCCOPY,
    TRANSPARENT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
use windows::Win32::UI::Shell::DROPFILES;

pub(crate) const HUD_H: i32 = 46;
pub(crate) const HUD_W_REC: i32 = 416;
pub(crate) const HUD_W_DONE: i32 = 504;
pub(crate) const HUD_W_ERR: i32 = 456;
const CF_HDROP_FMT: u32 = 15;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HudCmd {
    PauseResume,
    Finish,
    Cancel,
    CopyFile,
    Play,
    Reveal,
}

pub(crate) type HudButton = (HudCmd, ScreenRect, &'static str, bool, bool);

pub(crate) fn build_rec_hud_buttons(win_w: i32, mode: u8) -> Vec<HudButton> {
    let mut buttons = Vec::with_capacity(4);
    if mode <= 1 {
        buttons.push((HudCmd::PauseResume, ScreenRect { x: win_w - 168, y: 9, w: 54, h: 28 }, if mode == 1 { "继续" } else { "暂停" }, false, false));
        buttons.push((HudCmd::Finish, ScreenRect { x: win_w - 108, y: 9, w: 70, h: 28 }, "■ 完成", true, false));
        buttons.push((HudCmd::Cancel, ScreenRect { x: win_w - 32, y: 9, w: 24, h: 28 }, "✕", false, true));
    } else if mode >= 3 {
        if mode == 3 {
            buttons.push((HudCmd::CopyFile, ScreenRect { x: win_w - 226, y: 9, w: 68, h: 28 }, "复制文件", true, false));
            buttons.push((HudCmd::Play, ScreenRect { x: win_w - 152, y: 9, w: 46, h: 28 }, "播放", false, false));
            buttons.push((HudCmd::Reveal, ScreenRect { x: win_w - 100, y: 9, w: 62, h: 28 }, "定位目录", false, false));
        }
        buttons.push((HudCmd::Cancel, ScreenRect { x: win_w - 32, y: 9, w: 24, h: 28 }, "✕", false, true));
    }
    buttons
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_rec_hud(
    hwnd: HWND,
    dib_bits: *mut u8,
    mem_dc: HDC,
    font: HFONT,
    win_w: i32,
    rec_rect: ScreenRect,
    shared: &SharedRecState,
    buttons: &[HudButton],
    hovered: Option<usize>,
    tick_phase: u32,
    active_ms: u64,
) {
    let h = HUD_H;
    let dst = unsafe { std::slice::from_raw_parts_mut(dib_bits, (win_w * h * 4) as usize) };
    for px in dst.as_chunks_mut::<4>().0 { px.copy_from_slice(&[0x1E, 0x17, 0x15, 255]); }
    draw_capsule(dst, win_w, h, ScreenRect { x: 0, y: 0, w: win_w, h }, 10.0);

    let m = shared.mode.load(Ordering::Acquire);
    let dot_col = match m {
        0 if (tick_phase % 6) < 3 => AnnotColor(0xEF, 0x44, 0x44),
        0 => AnnotColor(0xB9, 0x1C, 0x1C),
        1 => AnnotColor(0xF5, 0x9E, 0x0B),
        3 => AnnotColor(0x10, 0xB9, 0x81),
        4 => AnnotColor(0xEF, 0x44, 0x44),
        _ => AnnotColor(0x60, 0xA5, 0xFA),
    };
    fill_circle_aa(dst, win_w, h, 18.0, h as f32 * 0.5, 5.0, dot_col, 1.0);

    for (i, &(_, br, _, accent, danger)) in buttons.iter().enumerate() {
        let hov = hovered == Some(i);
        if accent {
            fill_rounded_rect_aa(dst, win_w, h, br, 6.0, if hov { AnnotColor(0x1D, 0x4E, 0xD8) } else { AnnotColor(0x25, 0x63, 0xEB) }, 1.0);
        } else if danger {
            fill_rounded_rect_aa(dst, win_w, h, br, 6.0, AnnotColor(0xEF, 0x44, 0x44), if hov { 0.36 } else { 0.16 });
        } else {
            fill_rounded_rect_aa(dst, win_w, h, br, 6.0, AnnotColor(0xFF, 0xFF, 0xFF), if hov { 0.16 } else { 0.08 });
            stroke_rounded_rect_aa(dst, win_w, h, br, 6.0, 1.0, AnnotColor(0x38, 0x3D, 0x4F), 0.9);
        }
    }

    let secs = active_ms / 1000;
    let mb = shared.bytes.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0);
    let label = match m {
        0 => format!("{:02}:{:02} · {}×{} · {:.1} MB", secs / 60, secs % 60, rec_rect.w, rec_rect.h, mb),
        1 => format!("已暂停 {:02}:{:02} · {:.1} MB", secs / 60, secs % 60, mb),
        2 => "正在封装视频...".into(),
        3 if shared.copied_flash.load(Ordering::Relaxed) => "✓ 已复制文件！直接 Ctrl+V 粘贴".into(),
        3 => format!("已保存 · {:02}:{:02} · {:.1} MB", secs / 60, secs % 60, mb),
        _ => shared.status_note.lock().ok().and_then(|g| g.clone()).unwrap_or_else(|| "录屏已取消".into()),
    };

    unsafe {
        let old = SelectObject(mem_dc, font.into());
        SetBkMode(mem_dc, TRANSPARENT);
        SetTextColor(mem_dc, COLORREF(0x00FC_FAF8));
        let txt_right = match m { 3 => win_w - 232, 4 => win_w - 40, _ => win_w - 176 };
        let mut trc = RECT { left: 30, top: 0, right: txt_right.max(120), bottom: h };
        let mut lw: Vec<u16> = label.encode_utf16().collect();
        DrawTextW(mem_dc, &mut lw, &mut trc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);

        for &(_, br, txt, accent, danger) in buttons {
            let col = if accent { COLORREF(0x00FF_FFFF) } else if danger { COLORREF(0x00A5_A5FC) } else { COLORREF(0x00F0_E8E2) };
            SetTextColor(mem_dc, col);
            let mut brc = RECT { left: br.x, top: br.y, right: br.right(), bottom: br.bottom() };
            let mut bw: Vec<u16> = txt.encode_utf16().collect();
            DrawTextW(mem_dc, &mut bw, &mut brc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        let _ = SelectObject(mem_dc, old);

        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let _ = BitBlt(hdc, 0, 0, win_w, h, Some(mem_dc), 0, 0, SRCCOPY);
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Places a single file path on the Windows clipboard as `CF_HDROP` for direct Ctrl+V pasting.
pub(crate) fn copy_file_to_clipboard(path: &Path) -> Result<(), String> {
    let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain([0u16, 0u16]).collect();
    let hdr_sz = std::mem::size_of::<DROPFILES>();
    let total = hdr_sz + wide.len() * 2;
    unsafe {
        if OpenClipboard(None).is_err() { return Err("OpenClipboard failed".into()); }
        let _ = EmptyClipboard();
        let hmem = match GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total) {
            Ok(h) => h,
            Err(e) => { let _ = CloseClipboard(); return Err(format!("GlobalAlloc: {e}")); }
        };
        let ptr = GlobalLock(hmem) as *mut u8;
        if !ptr.is_null() {
            let df = DROPFILES { pFiles: hdr_sz as u32, pt: POINT::default(), fNC: false.into(), fWide: true.into() };
            std::ptr::write_unaligned(ptr.cast::<DROPFILES>(), df);
            std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), ptr.add(hdr_sz), wide.len() * 2);
            let _ = GlobalUnlock(hmem);
            let _ = SetClipboardData(CF_HDROP_FMT, Some(HANDLE(hmem.0)));
        }
        let _ = CloseClipboard();
    }
    Ok(())
}
