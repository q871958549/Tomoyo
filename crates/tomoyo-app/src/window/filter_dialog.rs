//! Frameless Fluent SDF + ClearType modal quick-add/edit dialog matching Tomoyo's launcher UI.

use super::bootstrap::{apply_dwm_attributes, read_edit_text};
use super::filter_dialog_ui::{
    hit_test_dlg, DlgGfx, DlgHit, DlgRenderParams, DLG_H_DIP, DLG_W_DIP, FIELD_RECTS, HEADER_H_DIP,
    PRESETS,
};
use super::{suppress_edit_change, AppShell, EM_SETCUEBANNER};
use std::ffi::c_void;
use tomoyo_core::{CustomFilterTab, Lang, TypographyConfig};
use tomoyo_protocol::SearchFilter;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, InvalidateRect,
    ScreenToClient, SetBkColor, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FW_NORMAL, HBRUSH, HDC, HFONT, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetMessageW, GetWindowLongPtrW, GetWindowRect, IsDialogMessageW, LoadCursorW, RegisterClassW,
    SendMessageW, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
    ShowWindow, TranslateMessage, ES_AUTOHSCROLL, ES_LEFT, GWLP_USERDATA, HMENU, HTCAPTION,
    HTCLIENT, HWND_TOP, IDC_ARROW, IDC_HAND, MSG, SW_SHOW, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_SETFONT, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN,
    WS_POPUP, WS_TABSTOP, WS_VISIBLE,
};

const ID_EDITS: [usize; 3] = [201, 202, 203];
const EN_SETFOCUS_CODE: u32 = 0x0100;
const EN_KILLFOCUS_CODE: u32 = 0x0200;

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
    bg_ref: COLORREF,
    fg_ref: COLORREF,
    edits: [HWND; 3],
    focused: Option<u8>,
    hovered: Option<DlgHit>,
    mouse_tracked: bool,
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
        if !ext.is_empty() && !out.contains(&ext) {
            out.push(ext);
        }
    }
    out
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
        st.error = Some(st.lang.pick("请填写至少一个扩展名或 Everything 表达式", "Provide extensions or an Everything expression").into());
        unsafe { let _ = SetFocus(Some(st.edits[1])); }
        return;
    }
    st.out_label = name;
    st.out_exts = exts;
    st.out_expr = expr;
    st.saved = true;
    st.done = true;
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
                focused: st.focused, hovered: st.hovered, error: st.error.as_deref(),
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
                    let lbl = if st.lang == Lang::Zh { lz } else { le };
                    let lw: Vec<u16> = lbl.encode_utf16().chain(std::iter::once(0)).collect();
                    let ew: Vec<u16> = exts.encode_utf16().chain(std::iter::once(0)).collect();
                    {
                        let _sup = suppress_edit_change();
                        unsafe {
                            let _ = SetWindowTextW(st.edits[0], PCWSTR(lw.as_ptr()));
                            let _ = SetWindowTextW(st.edits[1], PCWSTR(ew.as_ptr()));
                        }
                    }
                    st.error = None;
                    unsafe { let _ = SetFocus(Some(st.edits[1])); let _ = InvalidateRect(Some(hwnd), None, false); }
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
            ["例如：代码、视频、设计稿", "逗号或空格分隔，如 rs, ts, py, md", "留空自动按扩展名生成，如 ext:mp4;mkv"]
        } else {
            ["e.g. Code, Videos, Design", "Comma/space separated, e.g. rs, ts, py", "Optional, auto-built from extensions"]
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
                    ((x0 + 11.0) * scale).round() as i32,
                    ((y0 + 7.0) * scale).round() as i32,
                    ((x1 - x0 - 22.0) * scale).round() as i32,
                    ((y1 - y0 - 14.0) * scale).round() as i32,
                    Some(dlg), Some(HMENU(ID_EDITS[i] as *mut c_void)), Some(hinst.into()), None,
                ).unwrap_or_default()
            };
            unsafe {
                SendMessageW(ed, WM_SETFONT, Some(WPARAM(edit_font.0 as usize)), Some(LPARAM(0)));
                SendMessageW(ed, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(cw.as_ptr() as isize)));
            }
            edits[i] = ed;
        }

        let mut st = Box::new(DlgState {
            pal, lang, typo: self.typography.clone(), font_regular: self.font_regular, font_medium: self.font_medium,
            is_edit: existing.is_some(), scale, gfx, bg_brush, bg_ref: pal.bg.to_colorref(), fg_ref: pal.text_primary.to_colorref(),
            edits, focused: Some(0), hovered: None, mouse_tracked: false, error: None, saved: false, done: false,
            out_label: String::new(), out_exts: Vec::new(), out_expr: String::new(),
        });
        let st_ptr = &mut *st as *mut DlgState;
        unsafe {
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
                    if vk == 0x0D {
                        try_submit(&mut *st_ptr);
                        let _ = InvalidateRect(Some(dlg), None, false);
                        continue;
                    }
                }
                if !IsDialogMessageW(dlg, &msg).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
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
