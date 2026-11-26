//! Theme-matched Win32 modal quick-add/edit dialog for left sidebar custom filter tags.

use super::bootstrap::{apply_dwm_attributes, read_edit_text};
use super::AppShell;
use std::ffi::c_void;
use tomoyo_core::CustomFilterTab;
use tomoyo_protocol::SearchFilter;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreateSolidBrush, DeleteObject, FillRect, SetBkColor, SetBkMode, SetTextColor,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE,
    FW_MEDIUM, FW_NORMAL, HBRUSH, HDC, HFONT, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, GetWindowRect, IsDialogMessageW, LoadCursorW, RegisterClassW,
    SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
    ShowWindow, TranslateMessage, BM_CLICK, BN_CLICKED, BS_DEFPUSHBUTTON, BS_PUSHBUTTON,
    ES_AUTOHSCROLL, ES_LEFT, GWLP_USERDATA, HMENU, HWND_TOP, IDC_ARROW, MSG, SW_SHOW, WINDOW_STYLE,
    WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_ERASEBKGND,
    WM_KEYDOWN, WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_POPUP, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE,
};

const ID_NAME_EDIT: usize = 201;
const ID_EXT_EDIT: usize = 202;
const ID_EXPR_EDIT: usize = 203;
const ID_STATUS_LBL: usize = 204;
const ID_OK_BTN: usize = 1;
const ID_CANCEL_BTN: usize = 2;

struct FilterDlgState {
    bg_brush: HBRUSH,
    edit_brush: HBRUSH,
    bg_ref: COLORREF,
    edit_ref: COLORREF,
    fg_ref: COLORREF,
    err_ref: COLORREF,
    name_hwnd: HWND,
    ext_hwnd: HWND,
    expr_hwnd: HWND,
    status_hwnd: HWND,
    saved: bool,
    done: bool,
    out_label: String,
    out_exts: Vec<String>,
    out_expr: String,
    zh: bool,
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

fn make_ui_font(scale: f32, bold: bool) -> HFONT {
    let h = -(14.5 * scale).round() as i32;
    let wt = if bold { FW_MEDIUM.0 } else { FW_NORMAL.0 } as i32;
    unsafe {
        CreateFontW(
            h, 0, 0, 0, wt, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, w!("Segoe UI"),
        )
    }
}

unsafe extern "system" fn filter_dlg_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut FilterDlgState;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let st = unsafe { &mut *ptr };
    match msg {
        WM_ERASEBKGND => {
            let dc = HDC(wparam.0 as *mut c_void);
            let mut rc = RECT::default();
            unsafe { let _ = GetClientRect(hwnd, &mut rc); let _ = FillRect(dc, &rc, st.bg_brush); }
            LRESULT(1)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let dc = HDC(wparam.0 as *mut c_void);
            let ctl = HWND(lparam.0 as *mut c_void);
            unsafe {
                let _ = SetBkMode(dc, TRANSPARENT);
                let _ = SetBkColor(dc, st.bg_ref);
                let _ = SetTextColor(dc, if ctl == st.status_hwnd { st.err_ref } else { st.fg_ref });
            }
            LRESULT(st.bg_brush.0 as isize)
        }
        WM_CTLCOLOREDIT => {
            let dc = HDC(wparam.0 as *mut c_void);
            unsafe { let _ = SetBkColor(dc, st.edit_ref); let _ = SetTextColor(dc, st.fg_ref); }
            LRESULT(st.edit_brush.0 as isize)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            if id == ID_OK_BTN && (code == BN_CLICKED || code == 0) {
                let name = read_edit_text(st.name_hwnd).trim().to_string();
                let exts = parse_extensions(&read_edit_text(st.ext_hwnd));
                let expr = read_edit_text(st.expr_hwnd).trim().to_string();
                if name.is_empty() {
                    let msg = if st.zh { w!("请输入标签名称") } else { w!("Please enter a tag name") };
                    unsafe { let _ = SetWindowTextW(st.status_hwnd, msg); let _ = SetFocus(Some(st.name_hwnd)); }
                    return LRESULT(0);
                }
                if exts.is_empty() && expr.is_empty() {
                    let msg = if st.zh {
                        w!("请填写扩展名（如 rs,ts）或 Everything 表达式")
                    } else {
                        w!("Provide extensions or Everything expression")
                    };
                    unsafe { let _ = SetWindowTextW(st.status_hwnd, msg); let _ = SetFocus(Some(st.ext_hwnd)); }
                    return LRESULT(0);
                }
                st.out_label = name;
                st.out_exts = exts;
                st.out_expr = expr;
                st.saved = true;
                st.done = true;
                return LRESULT(0);
            }
            if id == ID_CANCEL_BTN && (code == BN_CLICKED || code == 0) {
                st.done = true;
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            st.done = true;
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

impl AppShell {
    pub(super) fn open_custom_filter_dialog(&mut self, edit_idx: Option<usize>) {
        let _modal = super::enter_modal_overlay();
        let existing = edit_idx.and_then(|i| self.config.read().ok().and_then(|c| c.custom_filters.get(i).cloned()));
        let zh = self.lang == tomoyo_core::Lang::Zh;
        let pal = self.theme;
        let scale = self.dpi_scale.max(1.0);
        let (dw, dh) = ((380.0 * scale).round() as i32, (268.0 * scale).round() as i32);

        let mut parent_rc = RECT::default();
        unsafe { let _ = GetWindowRect(self.hwnd, &mut parent_rc); }
        let dx = parent_rc.left + ((parent_rc.right - parent_rc.left) - dw) / 2;
        let dy = parent_rc.top + ((parent_rc.bottom - parent_rc.top) - dh).max(20) / 2;

        let cls = w!("TomoyoCustomFilterDlg");
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

        let title = match (existing.is_some(), zh) {
            (true, true) => w!("编辑自定义标签"),
            (true, false) => w!("Edit Custom Tag"),
            (false, true) => w!("快速添加搜索标签"),
            (false, false) => w!("Quick Add Search Tag"),
        };
        let Ok(dlg) = (unsafe {
            CreateWindowExW(
                Default::default(), cls, title,
                WS_POPUP | WS_CAPTION | WS_SYSMENU,
                dx, dy, dw, dh, Some(self.hwnd), None, Some(hinst.into()), None,
            )
        }) else {
            return;
        };
        apply_dwm_attributes(dlg, pal.is_dark);

        let font = make_ui_font(scale, false);
        let bold_font = make_ui_font(scale, true);
        let bg_brush = unsafe { CreateSolidBrush(pal.bg.to_colorref()) };
        let edit_brush = unsafe { CreateSolidBrush(pal.pop_bg.to_colorref()) };

        let px = |v: f32| (v * scale).round() as i32;
        let mk_ctrl = |cls_name: PCWSTR, txt: &str, style: WINDOW_STYLE, x: f32, y: f32, w: f32, h: f32, id: usize, fnt: HFONT| -> HWND {
            let tw: Vec<u16> = txt.encode_utf16().chain(std::iter::once(0)).collect();
            let ctl = unsafe {
                CreateWindowExW(
                    Default::default(), cls_name, PCWSTR(tw.as_ptr()), style,
                    px(x), px(y), px(w), px(h), Some(dlg), Some(HMENU(id as *mut c_void)),
                    Some(hinst.into()), None,
                ).unwrap_or_default()
            };
            unsafe { SendMessageW(ctl, WM_SETFONT, Some(WPARAM(fnt.0 as usize)), Some(LPARAM(0))); }
            ctl
        };

        let cw = 340.0;
        let edit_st = WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WINDOW_STYLE((ES_LEFT | ES_AUTOHSCROLL) as u32);
        let _ = mk_ctrl(w!("STATIC"), if zh { "标签名称（如：代码 / 视频）" } else { "Tag Label (e.g. Code / Video)" }, WS_CHILD | WS_VISIBLE, 18.0, 14.0, cw, 18.0, 301, bold_font);
        let name_hwnd = mk_ctrl(w!("EDIT"), existing.as_ref().map(|t| t.label.as_str()).unwrap_or(""), edit_st, 18.0, 34.0, cw, 26.0, ID_NAME_EDIT, font);
        let _ = mk_ctrl(w!("STATIC"), if zh { "文件扩展名（逗号/空格分隔，如 rs, ts, py）" } else { "Extensions (comma/space separated, e.g. rs, ts)" }, WS_CHILD | WS_VISIBLE, 18.0, 68.0, cw, 18.0, 302, bold_font);
        let init_ext = existing.as_ref().map(|t| t.extensions.join(", ")).unwrap_or_default();
        let ext_hwnd = mk_ctrl(w!("EDIT"), &init_ext, edit_st, 18.0, 88.0, cw, 26.0, ID_EXT_EDIT, font);
        let _ = mk_ctrl(w!("STATIC"), if zh { "Everything 表达式（可选，留空自动按扩展名匹配）" } else { "Everything Expr (optional, e.g. ext:mp4;mkv)" }, WS_CHILD | WS_VISIBLE, 18.0, 122.0, cw, 18.0, 303, bold_font);
        let expr_hwnd = mk_ctrl(w!("EDIT"), existing.as_ref().map(|t| t.everything_expr.as_str()).unwrap_or(""), edit_st, 18.0, 142.0, cw, 26.0, ID_EXPR_EDIT, font);
        let status_hwnd = mk_ctrl(w!("STATIC"), "", WS_CHILD | WS_VISIBLE, 18.0, 174.0, cw, 18.0, ID_STATUS_LBL, font);
        let ok_btn = mk_ctrl(w!("BUTTON"), if zh { "保存并应用" } else { "Save" }, WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32), 166.0, 200.0, 96.0, 28.0, ID_OK_BTN, bold_font);
        let _cancel = mk_ctrl(w!("BUTTON"), if zh { "取消" } else { "Cancel" }, WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), 270.0, 200.0, 88.0, 28.0, ID_CANCEL_BTN, font);

        let mut st = Box::new(FilterDlgState {
            bg_brush, edit_brush,
            bg_ref: pal.bg.to_colorref(),
            edit_ref: pal.pop_bg.to_colorref(),
            fg_ref: pal.text_primary.to_colorref(),
            err_ref: COLORREF(0x004444EF),
            name_hwnd, ext_hwnd, expr_hwnd, status_hwnd,
            saved: false, done: false,
            out_label: String::new(), out_exts: Vec::new(), out_expr: String::new(),
            zh,
        });
        let st_ptr = &mut *st as *mut FilterDlgState;
        unsafe {
            SetWindowLongPtrW(dlg, GWLP_USERDATA, st_ptr as isize);
            let _ = EnableWindow(self.hwnd, false);
            let _ = ShowWindow(dlg, SW_SHOW);
            let _ = SetWindowPos(dlg, Some(HWND_TOP), dx, dy, dw, dh, Default::default());
            let _ = SetFocus(Some(name_hwnd));

            let mut msg = MSG::default();
            while !(*st_ptr).done && GetMessageW(&mut msg, None, 0, 0).into() {
                if msg.message == WM_KEYDOWN {
                    let vk = msg.wParam.0 as u32;
                    if vk == 0x1B { (*st_ptr).done = true; continue; }
                    if vk == 0x0D { let _ = SendMessageW(ok_btn, BM_CLICK, Some(WPARAM(0)), Some(LPARAM(0))); continue; }
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
            let _ = DeleteObject(edit_brush.into());
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(bold_font.into());
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

