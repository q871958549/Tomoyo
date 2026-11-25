//! Minimalist native Win32 Super Panel popup window (`TomoyoSuperPanelWindow`) with
//! triple-redundant click-outside dismissal, async inline translation, and 0-byte idle release.

use super::inline_trans::{spawn_inline_translate, TransOutcome};
use super::panel_layout::{apply_panel_dwm, compute_layout, place_near_cursor, SuperHit, SuperPanelLayout};
use super::panel_paint::paint_super_panel;
use super::{trigger, InlineTransState, SuperAction, SuperActionKind, SuperContext};
use crate::renderer::{DibSurface, FontPack, ThemePalette};
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Instant;
use tomoyo_core::{Lang, TypographyConfig};
use tomoyo_plugin_host::StdioOneShotSpec;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, ScreenToClient, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL, VK_DOWN,
    VK_ESCAPE, VK_RETURN, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos,
    GetWindowLongPtrW, GetWindowRect, LoadCursorW, PostMessageW, RegisterClassExW, SetCursor,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_DROPSHADOW, GWLP_USERDATA, HWND_TOPMOST,
    IDC_ARROW, IDC_HAND, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOW, WM_ACTIVATE, WM_ACTIVATEAPP,
    WM_APP, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WNDCLASSEXW, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

const WM_APP_SP_TRANS_READY: u32 = WM_APP + 60;
const WM_APP_SP_DISMISS: u32 = WM_APP + 61;

static REGISTER_CLASS: Once = Once::new();
static ACTIVE_PANEL_HWND: AtomicIsize = AtomicIsize::new(0);
static PANEL_GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct SuperPanelExec {
    pub kind: SuperActionKind,
}

pub struct SuperPanelLaunch {
    pub anchor: POINT,
    pub host_hwnd: HWND,
    pub exec_msg: u32,
    pub exec_slot: Arc<Mutex<Option<SuperPanelExec>>>,
    pub translate_spec: Option<StdioOneShotSpec>,
    pub pal: ThemePalette,
    pub lang: Lang,
    pub font_regular: &'static str,
    pub font_medium: &'static str,
    pub typography: TypographyConfig,
    pub ctx: SuperContext,
    pub actions: Vec<SuperAction>,
    pub auto_translate: bool,
    pub translate_enabled: bool,
}

struct PanelState {
    hwnd: HWND,
    host_hwnd: HWND,
    exec_msg: u32,
    exec_slot: Arc<Mutex<Option<SuperPanelExec>>>,
    translate_spec: Option<StdioOneShotSpec>,
    pal: ThemePalette,
    lang: Lang,
    fonts: FontPack,
    scale: f32,
    anchor: POINT,
    opened_at: Instant,
    seq: u64,
    ctx: SuperContext,
    trans: InlineTransState,
    trans_slot: Arc<Mutex<Option<TransOutcome>>>,
    translate_enabled: bool,
    actions: Vec<SuperAction>,
    layout: SuperPanelLayout,
    hovered: Option<SuperHit>,
    pressed_hit: Option<SuperHit>,
    selected_row: Option<usize>,
    mouse_tracked: bool,
    surface: Option<DibSurface>,
}

pub fn close_active_panel() {
    let raw = ACTIVE_PANEL_HWND.swap(0, Ordering::AcqRel);
    trigger::clear_panel_open();
    if raw != 0 {
        unsafe {
            let _ = DestroyWindow(HWND(raw as *mut c_void));
        }
    }
}

pub fn open_super_panel(cfg: SuperPanelLaunch) {
    close_active_panel();
    ensure_window_class();

    let seq = PANEL_GEN.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    let want_auto_tr = cfg.auto_translate
        && cfg.translate_enabled
        && cfg.translate_spec.is_some()
        && cfg.ctx.selected_text().is_some();
    let initial_trans = if want_auto_tr { InlineTransState::Loading } else { InlineTransState::Hidden };

    let layout = compute_layout(&cfg.ctx, &initial_trans, cfg.actions.len());
    let Ok(hinst) = (unsafe { GetModuleHandleW(None) }) else {
        return;
    };
    let (init_w, init_h) = (layout.w_dip.round() as i32, layout.h_dip.round() as i32);
    let (x, y) = place_near_cursor(cfg.anchor, init_w, init_h);

    let Ok(hwnd) = (unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("TomoyoSuperPanelWindow"),
            w!(""),
            WS_POPUP | WS_CLIPCHILDREN,
            x, y, init_w, init_h,
            None, None, Some(hinst.into()), None,
        )
    }) else {
        return;
    };

    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
    let (w_px, h_px) = ((layout.w_dip * scale).round() as i32, (layout.h_dip * scale).round() as i32);
    let (px, py) = place_near_cursor(cfg.anchor, w_px, h_px);
    apply_panel_dwm(hwnd, cfg.pal.is_dark);

    let fonts = FontPack::create(scale, cfg.font_regular, cfg.font_medium, &cfg.typography);
    let trans_slot = Arc::new(Mutex::new(None));
    let state = Box::new(PanelState {
        hwnd, host_hwnd: cfg.host_hwnd, exec_msg: cfg.exec_msg, exec_slot: cfg.exec_slot,
        translate_spec: cfg.translate_spec, pal: cfg.pal, lang: cfg.lang, fonts, scale,
        anchor: cfg.anchor, opened_at: Instant::now(), seq, ctx: cfg.ctx, trans: initial_trans,
        trans_slot, translate_enabled: cfg.translate_enabled, actions: cfg.actions, layout,
        hovered: None, pressed_hit: None, selected_row: None, mouse_tracked: false, surface: None,
    });

    if want_auto_tr
        && let Some(spec) = state.translate_spec.clone()
        && let Some(text) = state.ctx.selected_text().map(str::to_string)
    {
        spawn_inline_translate(
            text, spec, Arc::clone(&state.trans_slot),
            hwnd, WM_APP_SP_TRANS_READY, seq, state.lang,
        );
    }

    let raw_ptr = Box::into_raw(state);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw_ptr as isize);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), px, py, w_px, h_px, SWP_SHOWWINDOW);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    ACTIVE_PANEL_HWND.store(hwnd.0 as isize, Ordering::Release);
    sync_actual_panel_bounds(hwnd, px, py, w_px, h_px, true);
    crate::window::hotkey_override::force_foreground_window(hwnd);
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn sync_actual_panel_bounds(hwnd: HWND, fx: i32, fy: i32, fw: i32, fh: i32, fresh: bool) {
    let mut wr = RECT::default();
    let (bx, by, bw, bh) = if unsafe { GetWindowRect(hwnd, &mut wr).is_ok() } {
        (wr.left, wr.top, wr.right - wr.left, wr.bottom - wr.top)
    } else {
        (fx, fy, fw, fh)
    };
    trigger::set_panel_bounds(hwnd, bx, by, bw, bh, WM_APP_SP_DISMISS, fresh);
}

impl PanelState {
    fn relayout_and_repaint(&mut self) {
        self.layout = compute_layout(&self.ctx, &self.trans, self.actions.len());
        let w_px = (self.layout.w_dip * self.scale).round() as i32;
        let h_px = (self.layout.h_dip * self.scale).round() as i32;
        let (x, y) = place_near_cursor(self.anchor, w_px, h_px);
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w_px, h_px, SWP_NOACTIVATE);
        }
        sync_actual_panel_bounds(self.hwnd, x, y, w_px, h_px, false);
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt).is_ok() && ScreenToClient(self.hwnd, &mut pt).as_bool() } {
            self.hovered = self.layout.hit_test(pt.x as f32 / self.scale, pt.y as f32 / self.scale);
        }
        unsafe { let _ = InvalidateRect(Some(self.hwnd), None, false); }
    }

    fn dispatch_and_close(&mut self, kind: SuperActionKind) {
        if let Ok(mut g) = self.exec_slot.lock() {
            *g = Some(SuperPanelExec { kind });
        }
        unsafe {
            let _ = PostMessageW(Some(self.host_hwnd), self.exec_msg, WPARAM(0), LPARAM(0));
        }
        close_active_panel();
    }

    fn trigger_hit(&mut self, hit: SuperHit) {
        match hit {
            SuperHit::CopyTranslation => {
                if let InlineTransState::Ready { translated, .. } = &self.trans {
                    let txt = translated.clone();
                    let toast = self.lang.pick("✓ 已复制译文", "✓ Copied translation").to_string();
                    self.dispatch_and_close(SuperActionKind::CopyText { text: txt, toast });
                }
            }
            SuperHit::OpenTranslator => {
                let src = self.ctx.selected_text().unwrap_or_default().to_string();
                self.dispatch_and_close(SuperActionKind::OpenTranslator(src));
            }
            SuperHit::Row(idx) => {
                let Some(act) = self.actions.get(idx).cloned() else {
                    return;
                };
                match act.kind {
                    SuperActionKind::QuickTranslate => {
                        if let InlineTransState::Ready { translated, .. } = &self.trans {
                            let txt = translated.clone();
                            let toast = self.lang.pick("✓ 已复制译文", "✓ Copied translation").to_string();
                            self.dispatch_and_close(SuperActionKind::CopyText { text: txt, toast });
                        } else if self.translate_enabled
                            && let Some(spec) = self.translate_spec.clone()
                            && let Some(text) = self.ctx.selected_text().map(str::to_string)
                        {
                            self.trans = InlineTransState::Loading;
                            spawn_inline_translate(
                                text,
                                spec,
                                Arc::clone(&self.trans_slot),
                                self.hwnd,
                                WM_APP_SP_TRANS_READY,
                                self.seq,
                                self.lang,
                            );
                            self.relayout_and_repaint();
                        }
                    }
                    other => self.dispatch_and_close(other),
                }
            }
        }
    }

    fn handle_key(&mut self, vk: u16) {
        if vk == VK_ESCAPE.0 {
            close_active_panel();
            return;
        }
        let ctrl = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
        if ctrl && vk == b'C' as u16 {
            if let InlineTransState::Ready { translated, .. } = &self.trans {
                let txt = translated.clone();
                let toast = self.lang.pick("✓ 已复制译文", "✓ Copied translation").to_string();
                self.dispatch_and_close(SuperActionKind::CopyText { text: txt, toast });
            } else if let Some(raw) = self.ctx.selected_text() {
                let toast = self.lang.pick("✓ 已复制文本", "✓ Copied text").to_string();
                self.dispatch_and_close(SuperActionKind::CopyText { text: raw.to_string(), toast });
            }
            return;
        }
        if (b'1' as u16..=b'6' as u16).contains(&vk) {
            let idx = (vk - b'1' as u16) as usize;
            if idx < self.actions.len() {
                self.trigger_hit(SuperHit::Row(idx));
            }
            return;
        }
        let n = self.actions.len();
        if n == 0 {
            return;
        }
        if vk == VK_RETURN.0 {
            if let Some(idx) = self.selected_row {
                self.trigger_hit(SuperHit::Row(idx));
            } else if matches!(self.trans, InlineTransState::Ready { .. }) {
                self.trigger_hit(SuperHit::CopyTranslation);
            } else {
                self.trigger_hit(SuperHit::Row(0));
            }
            return;
        }
        let next = match vk {
            x if x == VK_DOWN.0 => self.selected_row.map(|c| (c + 1) % n).unwrap_or(0),
            x if x == VK_UP.0 => self.selected_row.map(|c| (c + n - 1) % n).unwrap_or(n - 1),
            _ => return,
        };
        self.selected_row = Some(next);
        self.hovered = Some(SuperHit::Row(next));
        unsafe { let _ = InvalidateRect(Some(self.hwnd), None, false); }
    }
}

unsafe extern "system" fn super_panel_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PanelState;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let st = unsafe { &mut *ptr };

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut rc = RECT::default();
            let _ = unsafe { GetClientRect(hwnd, &mut rc) };
            paint_super_panel(
                hdc, rc.right - rc.left, rc.bottom - rc.top, st.scale, st.pal, st.lang,
                &st.fonts, &mut st.surface, &st.ctx, &st.trans, &st.actions, &st.layout,
                st.hovered, st.selected_row,
            );
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        WM_APP_SP_TRANS_READY => {
            let taken = st.trans_slot.lock().ok().and_then(|mut g| g.take());
            if let Some(out) = taken && out.seq == st.seq {
                st.trans = out.state;
                st.relayout_and_repaint();
            }
            LRESULT(0)
        }
        WM_APP_SP_DISMISS => {
            close_active_panel();
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if !st.mouse_tracked {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                unsafe { let _ = TrackMouseEvent(&mut tme); }
                st.mouse_tracked = true;
            }
            let x_dip = ((lparam.0 & 0xFFFF) as i16) as f32 / st.scale;
            let y_dip = (((lparam.0 >> 16) & 0xFFFF) as i16) as f32 / st.scale;
            let next = st.layout.hit_test(x_dip, y_dip);
            if next != st.hovered {
                st.hovered = next;
                st.selected_row = match next {
                    Some(SuperHit::Row(i)) => Some(i),
                    _ => None,
                };
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            st.mouse_tracked = false;
            if st.hovered.is_some() {
                st.hovered = None;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let cur_id = if st.hovered.is_some() { IDC_HAND } else { IDC_ARROW };
            if let Ok(h) = unsafe { LoadCursorW(None, cur_id) } {
                unsafe { SetCursor(Some(h)); }
            }
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            unsafe { let _ = SetFocus(Some(hwnd)); }
            let x_dip = ((lparam.0 & 0xFFFF) as i16) as f32 / st.scale;
            let y_dip = (((lparam.0 >> 16) & 0xFFFF) as i16) as f32 / st.scale;
            st.pressed_hit = st.layout.hit_test(x_dip, y_dip);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x_dip = ((lparam.0 & 0xFFFF) as i16) as f32 / st.scale;
            let y_dip = (((lparam.0 >> 16) & 0xFFFF) as i16) as f32 / st.scale;
            let pressed = st.pressed_hit.take();
            if let Some(hit) = st.layout.hit_test(x_dip, y_dip)
                && (pressed.is_none() || pressed == Some(hit))
            {
                st.trigger_hit(hit);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => { st.handle_key((wparam.0 & 0xFFFF) as u16); LRESULT(0) }
        WM_ACTIVATE if (wparam.0 & 0xFFFF) == 0 => {
            if st.opened_at.elapsed().as_millis() >= 80 { close_active_panel(); }
            LRESULT(0)
        }
        WM_ACTIVATEAPP if wparam.0 == 0 => {
            if st.opened_at.elapsed().as_millis() >= 80 { close_active_panel(); }
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            if st.opened_at.elapsed().as_millis() >= 80 { close_active_panel(); }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            if ACTIVE_PANEL_HWND.load(Ordering::Acquire) == hwnd.0 as isize {
                ACTIVE_PANEL_HWND.store(0, Ordering::Release);
                trigger::clear_panel_open();
            }
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0); drop(Box::from_raw(ptr)); }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn ensure_window_class() {
    REGISTER_CLASS.call_once(|| unsafe {
        let Ok(hinst) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(super_panel_wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("TomoyoSuperPanelWindow"),
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
    });
}
