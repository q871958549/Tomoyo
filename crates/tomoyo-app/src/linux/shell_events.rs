//! Linux event handlers: keyboard nav, mouse hit-tests, popup menus, clipboard capture & flows.

use crate::actions::{
    clipboard_has_rich_attachment, read_clipboard_files, read_clipboard_image_png, shell_open,
};
use crate::linux::{
    tomoyo_linux_ui_begin_drag, tomoyo_linux_ui_get_entry_text, tomoyo_linux_ui_quit,
};
use crate::linux::ipc::schedule_on_ui;
use crate::linux::shell_state::{with_current_core, AppCore};
use crate::renderer::{
    compute_filter_hitboxes, FILTER_BAR_HEIGHT_DIP, HEADER_HEIGHT_DIP, ROW_HEIGHT_DIP,
};
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::time::Duration;
use tomoyo_core::TomoyoConfig;
use tomoyo_protocol::{AttachedContext, SearchFilter};

unsafe extern "C" {
    fn tomoyo_linux_show_popup(
        labels: *const *const c_char,
        cmds: *const u32,
        checks: *const c_int,
        n: usize,
        cb: unsafe extern "C" fn(*mut c_void, u32),
        ctx: *mut c_void,
    );
}

#[inline]
fn borrow_core(ctx: *mut c_void) -> Option<&'static RefCell<AppCore>> {
    (!ctx.is_null()).then(|| unsafe { &*(ctx as *const RefCell<AppCore>) })
}

pub unsafe extern "C" fn on_draw_cb(ctx: *mut c_void, cr: *mut c_void, w: c_int, h: c_int) {
    if let Some(cell) = borrow_core(ctx)
        && let Ok(mut st) = cell.try_borrow_mut()
    {
        st.render_with_cr(cr, w, h);
    }
}

pub unsafe extern "C" fn on_text_changed_cb(ctx: *mut c_void) {
    if let Some(cell) = borrow_core(ctx)
        && let Ok(mut st) = cell.try_borrow_mut()
    {
        let ptr = unsafe { tomoyo_linux_ui_get_entry_text(st.ui) };
        let txt = if ptr.is_null() { String::new() } else { unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned() };
        st.current_query = txt;
        st.action_menu_open = false;
        st.dispatch_query();
    }
}

pub unsafe extern "C" fn on_paste_cb(ctx: *mut c_void) {
    if let Some(cell) = borrow_core(ctx)
        && let Ok(mut st) = cell.try_borrow_mut()
        && clipboard_has_rich_attachment()
    {
        if let Some(files) = read_clipboard_files() {
            st.attached = Some(AttachedContext::Files { paths: files });
            st.dispatch_query();
        } else if let Some((path, width, height, _)) = read_clipboard_image_png() {
            st.attached = Some(AttachedContext::Image { path, width, height });
            st.dispatch_query();
        }
    }
}

pub unsafe extern "C" fn on_focus_out_cb(ctx: *mut c_void) {
    if let Some(cell) = borrow_core(ctx)
        && let Ok(mut st) = cell.try_borrow_mut()
        && !st.menu_open
        && st.shown_at.elapsed() > Duration::from_millis(180)
    {
        st.hide_launcher();
    }
}

pub unsafe extern "C" fn on_clip_change_cb(_ctx: *mut c_void) {
    schedule_on_ui(|| with_current_core(|st| st.on_clipboard_update()));
}

pub unsafe extern "C" fn on_wl_clip_cb() {
    schedule_on_ui(|| with_current_core(|st| st.on_clipboard_update()));
}

pub unsafe extern "C" fn on_tray_cmd_cb(ctx: *mut c_void, cmd: u32) {
    if let Some(cell) = borrow_core(ctx)
        && let Ok(mut st) = cell.try_borrow_mut()
    {
        match cmd {
            1 => st.toggle_launcher(),
            2 => st.open_settings_center(),
            3 => st.open_clipboard_workbench(None),
            4 => st.open_translator_window("", None),
            5 => st.trigger_capture("region"),
            6 => st.trigger_capture("record"),
            9 => {
                st.shutdown_clean();
                unsafe { tomoyo_linux_ui_quit(); }
            }
            _ => {}
        }
    }
}

pub unsafe extern "C" fn on_key_cb(ctx: *mut c_void, keyval: u32, state: u32) -> c_int {
    let Some(cell) = borrow_core(ctx) else { return 0; };
    let Ok(mut st) = cell.try_borrow_mut() else { return 0; };
    let ctrl = (state & (1 << 2)) != 0;
    let shift = (state & (1 << 0)) != 0;
    let alt = (state & (1 << 3)) != 0;

    if ctrl && (keyval == 0x002C || keyval == 0x003B) && keyval == 0x002C {
        st.open_settings_center();
        return 1;
    }
    if ctrl && matches!(keyval, 0x0070 | 0x0050) {
        st.preview_enabled = !st.preview_enabled;
        st.resize_to_fit();
        st.invalidate();
        return 1;
    }
    if ctrl && matches!(keyval, 0x006B | 0x004B) {
        if !st.results.is_empty() {
            st.action_menu_open = !st.action_menu_open;
            st.selected_action = 0;
            st.invalidate();
        }
        return 1;
    }
    if ctrl && keyval == 0xFF09 {
        let tabs = st.visible_filter_tabs();
        let next = st.active_filter.cycle_visible(&tabs, !shift);
        st.set_active_filter(next);
        return 1;
    }
    if ctrl && (0x0031..=0x0039).contains(&keyval) {
        let idx = (keyval - 0x0031) as usize;
        let tabs = st.visible_filter_tabs();
        if let Some(t) = tabs.get(idx) {
            st.set_active_filter(t.filter);
        }
        return 1;
    }
    if alt && (0x0031..=0x0039).contains(&keyval) {
        let idx = st.scroll_offset + (keyval - 0x0031) as usize;
        if idx < st.results.len() {
            st.selected = idx;
            st.execute_selected(0);
        }
        return 1;
    }
    match keyval {
        0xFF1B => {
            if st.action_menu_open { st.action_menu_open = false; st.invalidate(); }
            else if !st.current_query.is_empty() { st.set_query_text(""); }
            else if st.attached.take().is_some() { st.dispatch_query(); }
            else if st.active_filter != SearchFilter::All { st.set_active_filter(SearchFilter::All); }
            else { st.hide_launcher(); }
            1
        }
        0xFF52 | 0xFF54 => {
            let up = keyval == 0xFF52;
            if st.action_menu_open {
                let n = st.results.get(st.selected).map(|r| r.actions.len().min(6)).unwrap_or(1);
                st.selected_action = if up { (st.selected_action + n - 1) % n } else { (st.selected_action + 1) % n };
            } else if !st.results.is_empty() {
                st.selected = if up { (st.selected + st.results.len() - 1) % st.results.len() } else { (st.selected + 1) % st.results.len() };
                st.ensure_selected_visible();
                st.refresh_selected_preview();
            }
            st.invalidate();
            1
        }
        0xFF09 => {
            if !st.results.is_empty() {
                st.action_menu_open = !st.action_menu_open;
                st.selected_action = 0;
                st.invalidate();
            }
            1
        }
        0xFF0D | 0xFF8D => {
            let idx = if st.action_menu_open { st.selected_action } else { 0 };
            st.execute_selected(idx);
            1
        }
        _ => 0,
    }
}

pub unsafe extern "C" fn on_mouse_cb(ctx: *mut c_void, kind: c_int, x: c_int, y: c_int, delta: c_int) {
    let Some(cell) = borrow_core(ctx) else { return; };
    let Ok(mut st) = cell.try_borrow_mut() else { return; };
    let xd = x as f32;
    let yd = y as f32;
    let header_h = HEADER_HEIGHT_DIP as f32;
    let filter_h = if st.is_filter_bar_visible() { FILTER_BAR_HEIGHT_DIP as f32 } else { 0.0 };
    let list_top = header_h + filter_h + 4.0;
    let row_from_y = (yd >= list_top).then(|| st.scroll_offset + ((yd - list_top) / ROW_HEIGHT_DIP as f32) as usize).filter(|&i| i < st.results.len());

    match kind {
        6 => {
            let max_off = st.results.len().saturating_sub(st.max_visible_rows);
            st.scroll_offset = if delta > 0 { st.scroll_offset.saturating_sub(1) } else { (st.scroll_offset + 1).min(max_off) };
            st.invalidate();
        }
        5 => {
            st.hovered = row_from_y;
            st.hovered_filter = if st.is_filter_bar_visible() && yd >= header_h && yd < header_h + filter_h {
                let tabs = st.visible_filter_tabs();
                compute_filter_hitboxes(st.list_width_dip as f32, header_h, &tabs, st.filter_counts.as_ref())
                    .into_iter().find(|b| xd >= b.x0 && xd <= b.x1 && yd >= b.y0 && yd <= b.y1).map(|b| b.filter)
            } else { None };
            st.invalidate();
        }
        1 | 2 => {
            if yd < header_h && xd < 56.0 {
                st.show_logo_menu();
                return;
            }
            if kind == 1 && yd < header_h {
                unsafe { tomoyo_linux_ui_begin_drag(st.ui); }
                return;
            }
            if st.is_filter_bar_visible() && yd >= header_h && yd < header_h + filter_h {
                let tabs = st.visible_filter_tabs();
                if let Some(b) = compute_filter_hitboxes(st.list_width_dip as f32, header_h, &tabs, st.filter_counts.as_ref())
                    .into_iter().find(|b| xd >= b.x0 && xd <= b.x1 && yd >= b.y0 && yd <= b.y1)
                {
                    st.set_active_filter(b.filter);
                }
                return;
            }
            if let Some(i) = row_from_y {
                st.selected = i;
                st.refresh_selected_preview();
                if kind == 2 { st.execute_selected(0); } else { st.invalidate(); }
            } else if kind == 1 {
                unsafe { tomoyo_linux_ui_begin_drag(st.ui); }
            }
        }
        3 => {
            if yd < header_h && xd < 56.0 { st.show_logo_menu(); }
            else if let Some(i) = row_from_y {
                st.selected = i;
                st.action_menu_open = true;
                st.selected_action = 0;
                st.invalidate();
            }
        }
        _ => {}
    }
}

unsafe extern "C" fn on_menu_picked(_ctx: *mut c_void, id: u32) {
    with_current_core(|st| {
        st.menu_open = false;
        match id {
            101 => st.open_settings_center(),
            102 => st.apply_theme("light", true),
            103 => st.apply_theme("dark", true),
            104 => st.apply_theme("midnight", true),
            105 => st.apply_language("zh", true),
            106 => st.apply_language("en", true),
            107 => { st.preview_enabled = !st.preview_enabled; st.resize_to_fit(); }
            108 => { st.custom_pos = None; st.resize_to_fit(); }
            109 => st.set_query_text(":stats"),
            110 => { st.router.reload_all(); st.dispatch_query(); st.show_toast("已热重载全部插件与索引"); }
            111 => { let _ = st.run_hibernate(); st.show_toast("已完成内存休眠整理"); }
            112 => { let _ = shell_open(&TomoyoConfig::config_path().to_string_lossy(), None, None); }
            113 => { st.shutdown_clean(); unsafe { tomoyo_linux_ui_quit(); } }
            _ => {}
        }
    });
}

impl AppCore {
    fn ensure_selected_visible(&mut self) {
        if self.selected < self.scroll_offset { self.scroll_offset = self.selected; }
        else if self.selected >= self.scroll_offset + self.max_visible_rows {
            self.scroll_offset = self.selected + 1 - self.max_visible_rows;
        }
    }

    fn show_logo_menu(&mut self) {
        self.menu_open = true;
        let is_l = self.theme_name.eq_ignore_ascii_case("light");
        let is_m = self.theme_name.eq_ignore_ascii_case("midnight");
        let entries: Vec<(&str, u32, bool)> = vec![
            ("设置中心 (Ctrl+,)", 101, false), ("---", 0, false),
            ("主题：浅色 (Light)", 102, is_l), ("主题：深色 (Dark)", 103, !is_l && !is_m), ("主题：午夜蓝 (Midnight)", 104, is_m),
            ("---", 0, false),
            ("简体中文", 105, self.lang == tomoyo_core::Lang::Zh), ("English", 106, self.lang == tomoyo_core::Lang::En),
            ("---", 0, false),
            ("右侧实时预览 (Ctrl+P)", 107, self.preview_enabled), ("重置窗口位置与尺寸", 108, false),
            ("内存与插件监控 (:stats)", 109, false), ("热重载全部 (:reload)", 110, false),
            ("立即休眠整理内存 (:gc)", 111, false), ("打开 config.toml", 112, false), ("---", 0, false), ("退出 Tomoyo", 113, false),
        ];
        let cstrs: Vec<CString> = entries.iter().map(|(l, _, _)| CString::new(*l).unwrap()).collect();
        let ptrs: Vec<*const c_char> = cstrs.iter().map(|s| s.as_ptr()).collect();
        let cmds: Vec<u32> = entries.iter().map(|(_, id, _)| *id).collect();
        let chks: Vec<c_int> = entries.iter().map(|(_, _, c)| c_int::from(*c)).collect();
        unsafe { tomoyo_linux_show_popup(ptrs.as_ptr(), cmds.as_ptr(), chks.as_ptr(), entries.len(), on_menu_picked, std::ptr::null_mut()); }
    }
}
