//! Frameless Fluent SDF + ClearType modal quick-add/edit dialog matching Tomoyo's launcher UI.

use super::bootstrap::{apply_dwm_attributes, read_edit_text};
use super::filter_dialog_ui::{
    format_rule_preview, hit_test_dlg, toggle_expr_token, DlgGfx, DlgHit, DlgRenderParams,
    COND_CHIPS, DLG_H_DIP, DLG_W_DIP, FIELD_RECTS, HEADER_H_DIP, PRESETS,
};
use super::{suppress_edit_change, AppShell, EM_SETCUEBANNER};
use std::ffi::c_void;
use tomoyo_core::{CustomFilterTab, Lang, TypographyConfig};
use tomoyo_protocol::SearchFilter;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, GetDC,
    InvalidateRect, ReleaseDC, ScreenToClient, SelectObject, SetBkColor, SetBkMode, SetTextColor,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_END_ELLIPSIS,
    DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL, HBRUSH, HDC, HFONT,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetMessageW, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, IsDialogMessageW,
    LoadCursorW, RegisterClassW, SendMessageW, SetCursor, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, ES_AUTOHSCROLL, ES_LEFT,
    GWLP_USERDATA, HMENU, HTCAPTION, HTCLIENT, HWND_TOP, IDC_ARROW, IDC_HAND, MSG, SW_SHOW,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN,
    WM_KILLFOCUS, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_SETFOCUS,
    WM_SETFONT, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
};

const ID_EDITS: [usize; 3] = [201, 202, 203];
const EN_SETFOCUS_CODE: u32 = 0x0100;
const EN_KILLFOCUS_CODE: u32 = 0x0200;
const EN_CHANGE_CODE: u32 = 0x0300;

struct DlgState {
    pal: crate::renderer::ThemePalette,
    lang: Lang,
    typo: TypographyConfig,
    font_regular: &'static str,
    font_medium: &'static str,
    is_edit: bool,
    scale: f32,
    gfx: DlgGfx,
    bg_brush: HBRUSH,
    edit_font: HFONT,
    bg_ref: COLORREF,
    fg_ref: COLORREF,
    muted_ref: COLORREF,
    cues: [&'static str; 3],
    edits: [HWND; 3],
    focused: Option<u8>,
    hovered: Option<DlgHit>,
    mouse_tracked: bool,
    live_exts: Vec<String>,
    live_expr: String,
    live_preview: String,
    error: Option<String>,
    saved: bool,
    done: bool,
    out_label: String,
    out_exts: Vec<String>,
    out_expr: String,
}

fn parse_extensions(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in raw.split([',', ';', ' ', '\t', '，', '；']) {
        let ext = part.trim().trim_start_matches('.').to_ascii_lowercase();
        if !ext.is_empty() && !out.contains(&ext) { out.push(ext); }
    }
    out
}

fn refresh_live_preview(st: &mut DlgState) {
    st.live_exts = parse_extensions(&read_edit_text(st.edits[1]));
    st.live_expr = read_edit_text(st.edits[2]).trim().to_string();
    st.live_preview = format_rule_preview(&st.live_exts, &st.live_expr, st.lang);
}

fn set_edit_str(hwnd: HWND, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _sup = suppress_edit_change();
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

fn try_submit(st: &mut DlgState) {
    let name = read_edit_text(st.edits[0]).trim().to_string();
    let exts = parse_extensions(&read_edit_text(st.edits[1]));
    let expr = read_edit_text(st.edits[2]).trim().to_string();
    if name.is_empty() {
        st.error = Some(st.lang.pick("请输入标签名称", "Please enter a tag name").into());
        unsafe { let _ = SetFocus(Some(st.edits[0])); }
        return;
    }
    if exts.is_empty() && expr.is_empty() {
        st.error = Some(st.lang.pick("请填写扩展名或点选上方筛选条件", "Add extensions or pick a condition chip").into());
        unsafe { let _ = SetFocus(Some(st.edits[1])); }
        return;
    }
    st.out_label = name;
    st.out_exts = exts;
    st.out_expr = expr;
    st.saved = true;
    st.done = true;
}

unsafe extern "system" fn dlg_edit_subclass_proc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, uid_subclass: usize, ref_data: usize,
) -> LRESULT {
    let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    let st_ptr = ref_data as *const DlgState;
    if st_ptr.is_null() { return res; }
    let idx = uid_subclass.saturating_sub(1);
    if matches!(msg, WM_SETFOCUS | WM_KILLFOCUS) && unsafe { GetWindowTextLengthW(hwnd) } == 0 {
        unsafe { let _ = InvalidateRect(Some(hwnd), None, true); }
    } else if msg == WM_PAINT && unsafe { GetWindowTextLengthW(hwnd) } == 0 && idx < 3 {
        let st = unsafe { &*st_ptr };
        let mut rc = RECT::default();
        unsafe { let _ = GetClientRect(hwnd, &mut rc); }
        rc.left += 2;
        let mut wide: Vec<u16> = st.cues[idx].encode_utf16().collect();
        let dc = unsafe { GetDC(Some(hwnd)) };
        if !dc.0.is_null() {
            unsafe {
                let old = SelectObject(dc, st.edit_font.into());
                let _ = SetBkMode(dc, TRANSPARENT);
                let _ = SetTextColor(dc, st.muted_ref);
                let _ = DrawTextW(dc, &mut wide, &mut rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                let _ = SelectObject(dc, old);
                let _ = ReleaseDC(Some(hwnd), dc);
            }
        }
    }
    res
}

unsafe extern "system" fn filter_dlg_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut DlgState;
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
            unsafe { let _ = GetClientRect(hwnd, &mut rc); }
            st.gfx.paint(hdc, rc.right - rc.left, rc.bottom - rc.top, &DlgRenderParams {
                pal: st.pal, lang: st.lang, typo: &st.typo,
                font_regular: st.font_regular, font_medium: st.font_medium,
                is_edit: st.is_edit, scale: st.scale,
                focused: st.focused, hovered: st.hovered,
                live_expr: &st.live_expr, preview: &st.live_preview,
                error: st.error.as_deref(),
            });
            unsafe { let _ = EndPaint(hwnd, &ps); }
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let mut pt = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
            unsafe { let _ = ScreenToClient(hwnd, &mut pt); }
            let (xd, yd) = (pt.x as f32 / st.scale, pt.y as f32 / st.scale);
            if yd <= HEADER_H_DIP && hit_test_dlg(xd, yd).is_none() {
                LRESULT(HTCAPTION as isize)
            } else {
                LRESULT(HTCLIENT as isize)
            }
        }
        WM_SETCURSOR => {
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                let mut pt = POINT::default();
                unsafe { let _ = GetCursorPos(&mut pt); let _ = ScreenToClient(hwnd, &mut pt); }
                if hit_test_dlg(pt.x as f32 / st.scale, pt.y as f32 / st.scale).is_some()
                    && let Ok(cur) = unsafe { LoadCursorW(None, IDC_HAND) }
                {
                    unsafe { SetCursor(Some(cur)); }
                    return LRESULT(1);
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_MOUSEMOVE => {
            if !st.mouse_tracked {
                st.mouse_tracked = true;
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                unsafe { let _ = TrackMouseEvent(&mut tme); }
            }
            let (x, y) = ((lparam.0 & 0xFFFF) as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
            let next = hit_test_dlg(x / st.scale, y / st.scale);
            if next != st.hovered {
                st.hovered = next;
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            st.mouse_tracked = false;
            if st.hovered.take().is_some() {
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = ((lparam.0 & 0xFFFF) as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
            match hit_test_dlg(x / st.scale, y / st.scale) {
                Some(DlgHit::Close | DlgHit::Cancel) => st.done = true,
                Some(DlgHit::Save) => {
                    try_submit(st);
                    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
                }
                Some(DlgHit::Preset(idx)) => {
                    let (lz, le, exts) = PRESETS[idx];
                    set_edit_str(st.edits[0], if st.lang == Lang::Zh { lz } else { le });
                    set_edit_str(st.edits[1], exts);
                    st.error = None;
                    refresh_live_preview(st);
                    unsafe { let _ = SetFocus(Some(st.edits[1])); let _ = InvalidateRect(Some(hwnd), None, false); }
                }
                Some(DlgHit::CondChip(idx)) => {
                    let next = toggle_expr_token(&read_edit_text(st.edits[2]), COND_CHIPS[idx].2);
                    set_edit_str(st.edits[2], &next);
                    st.error = None;
                    refresh_live_preview(st);
                    unsafe { let _ = SetFocus(Some(st.edits[2])); let _ = InvalidateRect(Some(hwnd), None, false); }
                }
                None => {}
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let dc = HDC(wparam.0 as *mut c_void);
            unsafe { let _ = SetBkColor(dc, st.bg_ref); let _ = SetTextColor(dc, st.fg_ref); }
            LRESULT(st.bg_brush.0 as isize)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            if let Some(fi) = ID_EDITS.iter().position(|&eid| eid == id) {
                if code == EN_SETFOCUS_CODE {
                    st.focused = Some(fi as u8);
                    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
                } else if code == EN_KILLFOCUS_CODE && st.focused == Some(fi as u8) {
                    st.focused = None;
                    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
                } else if code == EN_CHANGE_CODE {
                    st.error = None;
                    refresh_live_preview(st);
                    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => { st.done = true; LRESULT(0) }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn create_input_font(scale: f32, face: &str) -> HFONT {
    let h = -(15.5 * scale).round() as i32;
    let fw: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateFontW(
            h, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(fw.as_ptr()),
        )
    }
}

impl AppShell {
    pub(super) fn open_custom_filter_dialog(&mut self, edit_idx: Option<usize>) {
        let _modal = super::enter_modal_overlay();
        let existing = edit_idx.and_then(|i| self.config.read().ok().and_then(|c| c.custom_filters.get(i).cloned()));
        let pal = self.theme;
        let lang = self.lang;
        let scale = self.dpi_scale.max(1.0);
        let (dw, dh) = ((DLG_W_DIP * scale).round() as i32, (DLG_H_DIP * scale).round() as i32);

        let mut pr = RECT::default();
        unsafe { let _ = GetWindowRect(self.hwnd, &mut pr); }
        let dx = pr.left + ((pr.right - pr.left) - dw) / 2;
        let dy = pr.top + ((pr.bottom - pr.top) - dh).max(16) / 2;

        let cls = w!("TomoyoFluentFilterDlg");
        let hinst = unsafe { GetModuleHandleW(None).unwrap() };
        unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(filter_dlg_proc),
                hInstance: hinst.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: cls,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);
        }

        let Ok(dlg) = (unsafe {
            CreateWindowExW(
                Default::default(), cls, w!("Tomoyo Tag"),
                WS_POPUP | WS_CLIPCHILDREN,
                dx, dy, dw, dh, Some(self.hwnd), None, Some(hinst.into()), None,
            )
        }) else {
            return;
        };
        apply_dwm_attributes(dlg, pal.is_dark);

        let gfx = DlgGfx::new();
        let edit_font = create_input_font(scale, gfx.edit_font_face(&self.typography, self.font_regular, self.font_medium));
        let bg_brush = unsafe { CreateSolidBrush(pal.bg.to_colorref()) };
        let initial_vals = [
            existing.as_ref().map(|t| t.label.clone()).unwrap_or_default(),
            existing.as_ref().map(|t| t.extensions.join(", ")).unwrap_or_default(),
            existing.as_ref().map(|t| t.everything_expr.clone()).unwrap_or_default(),
        ];
        let cues = if lang == Lang::Zh {
            ["例如：代码、视频、合同", "逗号或空格分隔，如 rs, ts, py", "可选，留空按扩展名筛选或点下方条件"]
        } else {
            ["e.g. Code, Videos, Contracts", "Comma/space separated, e.g. rs, ts, py", "Optional, or click condition chips below"]
        };

        let mut edits = [HWND::default(); 3];
        for i in 0..3 {
            let (x0, y0, x1, y1) = FIELD_RECTS[i];
            let vw: Vec<u16> = initial_vals[i].encode_utf16().chain(std::iter::once(0)).collect();
            let cw: Vec<u16> = cues[i].encode_utf16().chain(std::iter::once(0)).collect();
            let ed = unsafe {
                CreateWindowExW(
                    Default::default(), w!("EDIT"), PCWSTR(vw.as_ptr()),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE((ES_LEFT | ES_AUTOHSCROLL) as u32),
                    ((x0 + 11.0) * scale).round() as i32, ((y0 + 7.0) * scale).round() as i32,
                    ((x1 - x0 - 22.0) * scale).round() as i32, ((y1 - y0 - 14.0) * scale).round() as i32,
                    Some(dlg), Some(HMENU(ID_EDITS[i] as *mut c_void)), Some(hinst.into()), None,
                ).unwrap_or_default()
            };
            unsafe {
                SendMessageW(ed, WM_SETFONT, Some(WPARAM(edit_font.0 as usize)), Some(LPARAM(0)));
                SendMessageW(ed, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(cw.as_ptr() as isize)));
            }
            edits[i] = ed;
        }
        let init_exts = parse_extensions(&initial_vals[1]);
        let init_expr = initial_vals[2].trim().to_string();
        let init_preview = format_rule_preview(&init_exts, &init_expr, lang);
        let mut st = Box::new(DlgState {
            pal, lang, typo: self.typography.clone(), font_regular: self.font_regular, font_medium: self.font_medium,
            is_edit: existing.is_some(), scale, gfx, bg_brush, edit_font,
            bg_ref: pal.bg.to_colorref(), fg_ref: pal.text_primary.to_colorref(), muted_ref: pal.text_muted.to_colorref(),
            cues, edits, focused: Some(0), hovered: None, mouse_tracked: false,
            live_exts: init_exts, live_expr: init_expr, live_preview: init_preview,
            error: None, saved: false, done: false, out_label: String::new(), out_exts: Vec::new(), out_expr: String::new(),
        });
        let st_ptr = &mut *st as *mut DlgState;
        unsafe {
            for (i, &ed) in edits.iter().enumerate() { let _ = SetWindowSubclass(ed, Some(dlg_edit_subclass_proc), i + 1, st_ptr as usize); }
            SetWindowLongPtrW(dlg, GWLP_USERDATA, st_ptr as isize);
            let _ = EnableWindow(self.hwnd, false);
            let _ = ShowWindow(dlg, SW_SHOW);
            let _ = SetWindowPos(dlg, Some(HWND_TOP), dx, dy, dw, dh, Default::default());
            let _ = SetFocus(Some(edits[0]));
            let mut msg = MSG::default();
            while !(*st_ptr).done && GetMessageW(&mut msg, None, 0, 0).into() {
                if msg.message == WM_KEYDOWN {
                    let vk = msg.wParam.0 as u32;
                    if vk == 0x1B { (*st_ptr).done = true; continue; }
                    if vk == 0x0D { try_submit(&mut *st_ptr); let _ = InvalidateRect(Some(dlg), None, false); continue; }
                }
                if !IsDialogMessageW(dlg, &msg).as_bool() { let _ = TranslateMessage(&msg); DispatchMessageW(&msg); }
            }
            for (i, &ed) in edits.iter().enumerate() { let _ = RemoveWindowSubclass(ed, Some(dlg_edit_subclass_proc), i + 1); }
            let _ = EnableWindow(self.hwnd, true);
            SetWindowLongPtrW(dlg, GWLP_USERDATA, 0);
            let _ = DestroyWindow(dlg);
            let _ = DeleteObject(bg_brush.into());
            let _ = DeleteObject(edit_font.into());
        }

        if st.saved {
            let new_idx = self.config.write().ok().map(|mut cfg| {
                if let Some(i) = edit_idx && let Some(tab) = cfg.custom_filters.get_mut(i) {
                    tab.label = st.out_label.clone();
                    tab.extensions = st.out_exts.clone();
                    tab.everything_expr = st.out_expr.clone();
                    tab.enabled = true;
                    let _ = cfg.sanitize();
                    let _ = cfg.save();
                    i
                } else {
                    let idx = cfg.custom_filters.len();
                    let slug = format!("custom_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() % 100000).unwrap_or(idx as u128));
                    let tag_hint = st.out_exts.first().cloned().unwrap_or_else(|| slug.clone());
                    cfg.custom_filters.push(CustomFilterTab {
                        id: slug, label: st.out_label.clone(), tag: format!("@{tag_hint}"),
                        extensions: st.out_exts.clone(), everything_expr: st.out_expr.clone(), enabled: true,
                    });
                    let _ = cfg.sanitize();
                    let _ = cfg.save();
                    cfg.custom_filters.len().saturating_sub(1)
                }
            });
            if let Some(idx) = new_idx {
                self.active_filter = SearchFilter::Custom(idx as u8);
                self.dispatch_query();
                self.resize_to_fit_results();
                self.invalidate();
                self.show_toast(&format!("{}: {}", self.lang.pick("已保存标签", "Saved tag"), st.out_label));
            }
        }
        unsafe { let _ = SetForegroundWindow(self.hwnd); let _ = SetFocus(Some(self.edit_hwnd)); }
    }
}
