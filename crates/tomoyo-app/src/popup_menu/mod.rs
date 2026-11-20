//! Theme-matched SDF + ClearType GDI popup context menu replacing legacy Win32 `CreatePopupMenu`.
//!
//! Renders with Tomoyo's active `ThemePalette`, Alibaba PuHuiTi 3.0 fonts, Win11 rounded DWM corners,
//! keyboard navigation, safe `WM_QUIT` propagation, and deterministic RAII GDI/USER cleanup.

mod paint;

use crate::renderer::{DibSurface, FontPack, ThemePalette};
use paint::paint_popup;
use std::ffi::c_void;
use std::sync::OnceLock;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetMessageW, GetWindowLongPtrW, LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TranslateMessage, GWLP_USERDATA, IDC_ARROW,
    MSG, SW_SHOW, WM_ACTIVATE, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NULL,
    WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const ROW_H_DIP: f32 = 31.0;
const SEP_H_DIP: f32 = 9.0;
const PAD_Y_DIP: f32 = 6.0;

#[derive(Debug, Clone)]
pub enum PopupEntry {
    Item {
        id: u32,
        label: String,
        shortcut: Option<String>,
        checked: bool,
    },
    Separator,
}

impl PopupEntry {
    pub fn item(id: u32, label: impl Into<String>, shortcut: Option<&str>, checked: bool) -> Self {
        Self::Item {
            id,
            label: label.into(),
            shortcut: shortcut.map(ToString::to_string),
            checked,
        }
    }
}

pub(super) struct RowMetric {
    pub y0: f32,
    pub y1: f32,
    pub is_sep: bool,
}

pub(super) struct PopupState<'a> {
    pub pal: ThemePalette,
    pub scale: f32,
    pub w_dip: f32,
    pub h_dip: f32,
    pub entries: &'a [PopupEntry],
    pub metrics: Vec<RowMetric>,
    pub hovered: Option<usize>,
    pub mouse_tracked: bool,
    pub chosen: Option<u32>,
    pub closed: bool,
    pub surface: Option<DibSurface>,
    pub fonts: FontPack,
}

static MENU_CLASS_OK: OnceLock<bool> = OnceLock::new();

pub fn show_themed_popup_menu(
    pal: ThemePalette,
    dpi_scale: f32,
    font_regular: &str,
    font_medium: &str,
    entries: &[PopupEntry],
    align_bottom: bool,
) -> Option<u32> {
    if entries.is_empty() {
        return None;
    }
    let scale = dpi_scale.max(1.0);
    let (w_dip, h_dip, metrics) = measure_menu(entries);
    let w_px = (w_dip * scale).round() as i32;
    let h_px = (h_dip * scale).round() as i32;

    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let (x, y) = place_on_monitor(pt, w_px, h_px, align_bottom);

    let cls = w!("TomoyoThemedPopupMenu");
    let hinst = unsafe { GetModuleHandleW(None).ok()? };
    MENU_CLASS_OK.get_or_init(|| unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(popup_wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: cls,
            ..Default::default()
        };
        RegisterClassW(&wc) != 0
    });

    let fonts = FontPack::create(scale, font_regular, font_medium);
    let mut state = Box::new(PopupState {
        pal,
        scale,
        w_dip,
        h_dip,
        entries,
        metrics,
        hovered: None,
        mouse_tracked: false,
        chosen: None,
        closed: false,
        surface: None,
        fonts,
    });

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            cls,
            w!(""),
            WS_POPUP | WS_CLIPCHILDREN,
            x,
            y,
            w_px,
            h_px,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .ok()?
    };

    unsafe {
        let dark: i32 = if pal.is_dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as *const c_void,
            std::mem::size_of_val(&round) as u32,
        );

        let ptr = &mut *state as *mut PopupState<'_> as isize;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        let mut msg = MSG::default();
        while !state.closed {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 <= 0 {
                if ret.0 == 0 {
                    // Propagate WM_QUIT back to outer launcher loop instead of swallowing it.
                    PostQuitMessage(msg.wParam.0 as i32);
                }
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
    }

    state.chosen
}

fn measure_menu(entries: &[PopupEntry]) -> (f32, f32, Vec<RowMetric>) {
    let mut y = PAD_Y_DIP;
    let mut max_w = 190.0f32;
    let mut metrics = Vec::with_capacity(entries.len());

    for ent in entries {
        match ent {
            PopupEntry::Separator => {
                metrics.push(RowMetric {
                    y0: y,
                    y1: y + SEP_H_DIP,
                    is_sep: true,
                });
                y += SEP_H_DIP;
            }
            PopupEntry::Item {
                label, shortcut, ..
            } => {
                let lw: f32 = label
                    .chars()
                    .map(|c| if c.is_ascii() { 7.5 } else { 13.0 })
                    .sum();
                let sw: f32 = shortcut
                    .as_deref()
                    .map(|s| s.chars().count() as f32 * 7.2 + 20.0)
                    .unwrap_or(0.0);
                max_w = max_w.max(lw + sw + 52.0);
                metrics.push(RowMetric {
                    y0: y,
                    y1: y + ROW_H_DIP,
                    is_sep: false,
                });
                y += ROW_H_DIP;
            }
        }
    }
    (max_w.clamp(200.0, 360.0), y + PAD_Y_DIP, metrics)
}

fn place_on_monitor(pt: POINT, w_px: i32, h_px: i32, align_bottom: bool) -> (i32, i32) {
    unsafe {
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(hmon, &mut mi);
        let work = mi.rcWork;
        let raw_y = if align_bottom { pt.y - h_px } else { pt.y };
        let x = pt.x.min(work.right - w_px - 6).max(work.left + 6);
        let y = raw_y.min(work.bottom - h_px - 6).max(work.top + 6);
        (x, y)
    }
}

unsafe extern "system" fn popup_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PopupState<'_>;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let st = unsafe { &mut *ptr };

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) == 0 {
                close_popup(hwnd, st);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if !st.mouse_tracked {
                st.mouse_tracked = true;
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
            }
            let y_dip = (((lparam.0 >> 16) & 0xFFFF) as i16 as f32) / st.scale;
            let hit = hit_test_item(&st.metrics, y_dip);
            if hit != st.hovered {
                st.hovered = hit;
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            st.mouse_tracked = false;
            if st.hovered.take().is_some() {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let y_dip = (((lparam.0 >> 16) & 0xFFFF) as i16 as f32) / st.scale;
            if let Some(idx) = hit_test_item(&st.metrics, y_dip) {
                if let Some(PopupEntry::Item { id, .. }) = st.entries.get(idx) {
                    st.chosen = Some(*id);
                }
            }
            close_popup(hwnd, st);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            close_popup(hwnd, st);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            if vk == VK_ESCAPE.0 {
                close_popup(hwnd, st);
            } else if vk == VK_RETURN.0 {
                if let Some(idx) = st.hovered {
                    if let Some(PopupEntry::Item { id, .. }) = st.entries.get(idx) {
                        st.chosen = Some(*id);
                    }
                }
                close_popup(hwnd, st);
            } else if vk == VK_DOWN.0 || vk == VK_UP.0 {
                step_selection(st, vk == VK_DOWN.0);
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut rc = RECT::default();
            let _ = unsafe { GetClientRect(hwnd, &mut rc) };
            paint_popup(hdc, rc.right - rc.left, rc.bottom - rc.top, st);
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn close_popup(hwnd: HWND, st: &mut PopupState<'_>) {
    st.closed = true;
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    }
}

fn hit_test_item(metrics: &[RowMetric], y_dip: f32) -> Option<usize> {
    metrics
        .iter()
        .enumerate()
        .find(|(_, m)| !m.is_sep && y_dip >= m.y0 && y_dip < m.y1)
        .map(|(i, _)| i)
}

fn step_selection(st: &mut PopupState<'_>, down: bool) {
    let valid: Vec<usize> = st
        .metrics
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.is_sep)
        .map(|(i, _)| i)
        .collect();
    if valid.is_empty() {
        return;
    }
    st.hovered = match st.hovered.and_then(|cur| valid.iter().position(|&v| v == cur)) {
        Some(pos) => {
            let next = if down {
                (pos + 1) % valid.len()
            } else if pos == 0 {
                valid.len() - 1
            } else {
                pos - 1
            };
            Some(valid[next])
        }
        None => {
            if down {
                valid.first().copied()
            } else {
                valid.last().copied()
            }
        }
    };
}
