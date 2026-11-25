//! Cascading Level-1 / Level-2 Win32 popup controller with try-in-place persistence.

use super::paint::paint_panel;
use super::panel::{
    apply_panel_dwm, create_panel_window, hit_test_item, place_child_on_monitor, place_on_monitor,
    step_selection, PanelState,
};
use super::{MenuStepOutcome, PopupEntry};
use crate::renderer::{FontPack, ThemePalette};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT,
    VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageW,
    GetWindowLongPtrW, GetWindowThreadProcessId, IsWindow, KillTimer, LoadCursorW, PostMessageW,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW, MSG, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_SHOW, SW_SHOWNOACTIVATE, WM_ACTIVATE, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NULL, WM_PAINT, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

const SUBMENU_SWITCH_TIMER: usize = 701;

static SUPPRESS_DEACTIVATE_DEPTH: AtomicUsize = AtomicUsize::new(0);
struct SuppressDeactivateGuard;
impl Drop for SuppressDeactivateGuard {
    fn drop(&mut self) { SUPPRESS_DEACTIVATE_DEPTH.fetch_sub(1, Ordering::SeqCst); }
}
fn enter_suppress_deactivate() -> SuppressDeactivateGuard {
    SUPPRESS_DEACTIVATE_DEPTH.fetch_add(1, Ordering::SeqCst);
    SuppressDeactivateGuard
}
fn is_deactivate_suppressed() -> bool { SUPPRESS_DEACTIVATE_DEPTH.load(Ordering::SeqCst) > 0 }

pub(super) struct MenuSession<'a> {
    pub pal: ThemePalette,
    pub scale: f32,
    pub fonts: FontPack,
    pub owner: Option<HWND>,
    pub lvl1: PanelState,
    pub lvl2: Option<PanelState>,
    pub pending_sub_idx: Option<usize>,
    pub external_deactivate: AtomicBool,
    pub closed: AtomicBool,
    pub on_command: &'a mut dyn FnMut(u32) -> MenuStepOutcome,
}

static MENU_CLASS_OK: OnceLock<bool> = OnceLock::new();

pub(super) fn run_session(
    style: super::PopupMenuStyle<'_>,
    entries: Vec<PopupEntry>,
    on_command: &mut dyn FnMut(u32) -> MenuStepOutcome,
) -> bool {
    if entries.is_empty() {
        return false;
    }
    let scale = style.dpi_scale.max(1.0);
    ensure_popup_class();

    let mut pt = POINT::default();
    unsafe { let _ = GetCursorPos(&mut pt); }

    let fonts = FontPack::create(
        scale,
        style.font_regular,
        style.font_medium,
        &tomoyo_core::TypographyConfig::default(),
    );
    let pal = style.pal;
    let owner = style.owner;
    let align_bottom = style.align_bottom;
    let sess = Box::new(MenuSession {
        pal,
        scale,
        fonts,
        owner,
        lvl1: PanelState::from_entries(entries, false),
        lvl2: None,
        pending_sub_idx: None,
        external_deactivate: AtomicBool::new(false),
        closed: AtomicBool::new(false),
        on_command,
    });

    let w_px = (sess.lvl1.w_dip * scale).round() as i32;
    let h_px = (sess.lvl1.h_dip * scale).round() as i32;
    let (x, y) = place_on_monitor(pt, w_px, h_px, align_bottom);
    let sess_ptr = Box::into_raw(sess);
    let Some(hwnd1) = create_panel_window(x, y, w_px, h_px, pal.is_dark, owner, sess_ptr as isize) else {
        unsafe { drop(Box::from_raw(sess_ptr)); }
        return false;
    };
    unsafe { (*sess_ptr).lvl1.hwnd = hwnd1; }

    unsafe {
        let _ = ShowWindow(hwnd1, SW_SHOW);
        let _ = SetForegroundWindow(hwnd1);

        let mut msg = MSG::default();
        loop {
            if (*sess_ptr).closed.load(Ordering::SeqCst) {
                break;
            }
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 <= 0 {
                if ret.0 == 0 {
                    PostQuitMessage(msg.wParam.0 as i32);
                }
                break;
            }
            let _ = TranslateMessage(&msg);
            windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
        }

        let _guard = enter_suppress_deactivate();
        close_child_panel(&mut *sess_ptr);
        let ext = (*sess_ptr).external_deactivate.load(Ordering::SeqCst);
        if !ext
            && let Some(own) = (*sess_ptr).owner
            && IsWindow(Some(own)).as_bool()
        {
            let _ = SetForegroundWindow(own);
        }
        SetWindowLongPtrW(hwnd1, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd1);
        drop(Box::from_raw(sess_ptr));
        ext
    }
}

fn ensure_popup_class() {
    MENU_CLASS_OK.get_or_init(|| unsafe {
        let Ok(hinst) = GetModuleHandleW(None) else { return false };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(popup_wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("TomoyoThemedPopupMenu"),
            ..Default::default()
        };
        RegisterClassW(&wc) != 0
    });
}

fn close_child_panel(sess: &mut MenuSession<'_>) {
    sess.lvl1.active_sub_idx = None;
    sess.pending_sub_idx = None;
    unsafe {
        let _ = KillTimer(Some(sess.lvl1.hwnd), SUBMENU_SWITCH_TIMER);
    }
    if let Some(child) = sess.lvl2.take() {
        let _guard = enter_suppress_deactivate();
        unsafe {
            SetWindowLongPtrW(child.hwnd, GWLP_USERDATA, 0);
            if IsWindow(Some(child.hwnd)).as_bool() {
                let _ = DestroyWindow(child.hwnd);
            }
        }
    }
}

fn open_child_submenu(sess: &mut MenuSession<'_>, idx: usize) {
    let Some(PopupEntry::Submenu { children, .. }) = sess.lvl1.entries.get(idx) else {
        close_child_panel(sess);
        return;
    };
    if children.is_empty() {
        close_child_panel(sess);
        return;
    }
    let children_clone = children.clone();
    if sess.lvl1.active_sub_idx == Some(idx)
        && let Some(child) = sess.lvl2.as_mut()
    {
        child.update_entries(children_clone, sess.scale);
        unsafe { let _ = InvalidateRect(Some(child.hwnd), None, false); }
        return;
    }

    close_child_panel(sess);
    let mut panel = PanelState::from_entries(children_clone, true);
    let cw = (panel.w_dip * sess.scale).round() as i32;
    let ch = (panel.h_dip * sess.scale).round() as i32;
    let row_y0_px = (sess.lvl1.metrics[idx].y0 * sess.scale).round() as i32;
    let (cx, cy) = place_child_on_monitor(sess.lvl1.hwnd, row_y0_px, cw, ch, sess.scale);
    let sess_ptr = sess as *mut MenuSession<'_> as isize;
    let Some(hwnd2) = create_panel_window(cx, cy, cw, ch, sess.pal.is_dark, Some(sess.lvl1.hwnd), sess_ptr) else {
        return;
    };
    panel.hwnd = hwnd2;
    sess.lvl1.active_sub_idx = Some(idx);
    sess.lvl2 = Some(panel);
    unsafe {
        let _ = SetWindowPos(hwnd2, Some(HWND_TOPMOST), cx, cy, cw, ch, SWP_NOACTIVATE);
        let _ = ShowWindow(hwnd2, SW_SHOWNOACTIVATE);
        let _ = InvalidateRect(Some(sess.lvl1.hwnd), None, false);
    }
}

fn is_menu_family_hwnd(sess: &MenuSession<'_>, other: HWND) -> bool {
    !other.is_invalid()
        && (other == sess.lvl1.hwnd || sess.lvl2.as_ref().is_some_and(|c| c.hwnd == other))
}

fn invoke_command(sess: &mut MenuSession<'_>, cmd_id: u32) {
    let _guard = enter_suppress_deactivate();
    let outcome = (sess.on_command)(cmd_id);
    match outcome {
        MenuStepOutcome::Close => close_session(sess),
        MenuStepOutcome::StayOpen { entries, pal } => {
            let theme_changed = sess.pal.is_dark != pal.is_dark
                || sess.pal.pop_bg.0 != pal.pop_bg.0
                || sess.pal.pop_bg.2 != pal.pop_bg.2;
            sess.pal = pal;
            if theme_changed {
                apply_panel_dwm(sess.lvl1.hwnd, pal.is_dark);
                if let Some(c) = &sess.lvl2 {
                    apply_panel_dwm(c.hwnd, pal.is_dark);
                }
            }
            let active_sub = sess.lvl1.active_sub_idx;
            sess.lvl1.update_entries(entries, sess.scale);
            if let Some(sub_idx) = active_sub {
                if let Some(PopupEntry::Submenu { children, .. }) = sess.lvl1.entries.get(sub_idx).cloned() {
                    if let Some(child) = sess.lvl2.as_mut() {
                        child.update_entries(children, sess.scale);
                        unsafe { let _ = InvalidateRect(Some(child.hwnd), None, false); }
                    }
                } else {
                    close_child_panel(sess);
                }
            }
            unsafe {
                let _ = SetWindowPos(sess.lvl1.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                if let Some(c) = &sess.lvl2 {
                    let _ = SetWindowPos(c.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
                let _ = SetForegroundWindow(sess.lvl1.hwnd);
                let _ = InvalidateRect(Some(sess.lvl1.hwnd), None, false);
            }
        }
    }
}

unsafe extern "system" fn popup_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut MenuSession<'_>;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let sess = unsafe { &mut *ptr };
    let is_child = sess.lvl2.as_ref().is_some_and(|c| c.hwnd == hwnd);

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) == 0 && !is_deactivate_suppressed() {
                let other = HWND(lparam.0 as *mut c_void);
                if !is_menu_family_hwnd(sess, other) {
                    let target = if !other.is_invalid() { other } else { unsafe { GetForegroundWindow() } };
                    let mut pid = 0u32;
                    if !target.is_invalid() {
                        unsafe { GetWindowThreadProcessId(target, Some(&mut pid)); }
                    }
                    if pid != 0 && pid != std::process::id() {
                        sess.external_deactivate.store(true, Ordering::SeqCst);
                    }
                    close_session(sess);
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == SUBMENU_SWITCH_TIMER && !is_child {
                unsafe { let _ = KillTimer(Some(hwnd), SUBMENU_SWITCH_TIMER); }
                if sess.lvl2.as_ref().is_some_and(|c| c.mouse_tracked) {
                    return LRESULT(0);
                }
                match sess.pending_sub_idx.take() {
                    Some(idx) => open_child_submenu(sess, idx),
                    None => {
                        close_child_panel(sess);
                        unsafe { let _ = InvalidateRect(Some(sess.lvl1.hwnd), None, false); }
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let y_dip = (((lparam.0 >> 16) & 0xFFFF) as i16 as f32) / sess.scale;
            let panel = panel_mut(sess, is_child);
            if !panel.mouse_tracked {
                panel.mouse_tracked = true;
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                unsafe { let _ = TrackMouseEvent(&mut tme); }
            }
            let hit = hit_test_item(&panel.metrics, y_dip);
            if hit != panel.hovered {
                panel.hovered = hit;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            if !is_child {
                let wants_sub = hit.filter(|&i| matches!(sess.lvl1.entries.get(i), Some(PopupEntry::Submenu { .. })));
                if wants_sub != sess.lvl1.active_sub_idx {
                    sess.pending_sub_idx = wants_sub;
                    let delay = if wants_sub.is_some() { 90 } else { 180 };
                    unsafe { let _ = SetTimer(Some(hwnd), SUBMENU_SWITCH_TIMER, delay, None); }
                } else {
                    unsafe { let _ = KillTimer(Some(hwnd), SUBMENU_SWITCH_TIMER); }
                }
            } else {
                unsafe { let _ = KillTimer(Some(sess.lvl1.hwnd), SUBMENU_SWITCH_TIMER); }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let keep_sub_hover = !is_child && sess.lvl2.is_some();
            let active_sub = sess.lvl1.active_sub_idx;
            let panel = panel_mut(sess, is_child);
            panel.mouse_tracked = false;
            if keep_sub_hover && panel.hovered == active_sub {
                return LRESULT(0);
            }
            if panel.hovered.take().is_some() {
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let y_dip = (((lparam.0 >> 16) & 0xFFFF) as i16 as f32) / sess.scale;
            let panel = panel_mut(sess, is_child);
            if let Some(idx) = hit_test_item(&panel.metrics, y_dip) {
                match panel.entries.get(idx).cloned() {
                    Some(PopupEntry::Submenu { .. }) if !is_child => open_child_submenu(sess, idx),
                    Some(PopupEntry::Item { id, .. }) => invoke_command(sess, id),
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => LRESULT(0),
        WM_KEYDOWN => {
            handle_menu_keydown(sess, wparam.0 as u16);
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut rc = RECT::default();
            let _ = unsafe { GetClientRect(hwnd, &mut rc) };
            let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
            let (pal, scale) = (sess.pal, sess.scale);
            let fonts_ptr = &sess.fonts as *const FontPack;
            let panel = panel_mut(sess, is_child);
            paint_panel(hdc, w, h, pal, scale, unsafe { &*fonts_ptr }, panel);
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn handle_menu_keydown(sess: &mut MenuSession<'_>, vk: u16) {
    if vk == VK_ESCAPE.0 {
        close_session(sess);
        return;
    }
    let child_active = sess.lvl2.as_ref().is_some_and(|c| c.hovered.is_some());
    if vk == VK_LEFT.0 && sess.lvl2.is_some() {
        close_child_panel(sess);
        unsafe { let _ = InvalidateRect(Some(sess.lvl1.hwnd), None, false); }
        return;
    }
    if vk == VK_RIGHT.0
        && let Some(idx) = sess.lvl1.hovered
        && matches!(sess.lvl1.entries.get(idx), Some(PopupEntry::Submenu { .. }))
    {
        open_child_submenu(sess, idx);
        if let Some(c) = sess.lvl2.as_mut() {
            step_selection(c, true);
            unsafe { let _ = InvalidateRect(Some(c.hwnd), None, false); }
        }
        return;
    }
    if vk == VK_RETURN.0 {
        let target = if child_active {
            sess.lvl2.as_ref().and_then(|c| c.hovered.and_then(|i| c.entries.get(i).cloned()))
        } else {
            sess.lvl1.hovered.and_then(|i| sess.lvl1.entries.get(i).cloned())
        };
        match target {
            Some(PopupEntry::Submenu { .. }) => if let Some(idx) = sess.lvl1.hovered { open_child_submenu(sess, idx); },
            Some(PopupEntry::Item { id, .. }) => invoke_command(sess, id),
            _ => {}
        }
        return;
    }
    if vk == VK_DOWN.0 || vk == VK_UP.0 {
        let panel = if child_active { sess.lvl2.as_mut().unwrap() } else { &mut sess.lvl1 };
        step_selection(panel, vk == VK_DOWN.0);
        unsafe { let _ = InvalidateRect(Some(panel.hwnd), None, false); }
    }
}

fn panel_mut<'a>(sess: &'a mut MenuSession<'_>, is_child: bool) -> &'a mut PanelState {
    if is_child
        && let Some(child) = sess.lvl2.as_mut()
    {
        child
    } else {
        &mut sess.lvl1
    }
}

fn close_session(sess: &mut MenuSession<'_>) {
    sess.closed.store(true, Ordering::SeqCst);
    unsafe { let _ = PostMessageW(Some(sess.lvl1.hwnd), WM_NULL, WPARAM(0), LPARAM(0)); }
}
