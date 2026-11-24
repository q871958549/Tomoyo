//! Linux event handlers: keyboard nav, mouse hit-tests, popup menus, clipboard capture & flows.

use crate::actions::{clipboard_has_rich_attachment, read_clipboard_files, read_clipboard_image_png};
use crate::linux::{
    tomoyo_linux_ui_begin_drag, tomoyo_linux_ui_get_entry_text, tomoyo_linux_ui_quit,
};
use crate::linux::ipc::schedule_on_ui;
use crate::linux::shell_state::{with_current_core, AppCore};
use crate::renderer::{compute_filter_bar_layout, FilterHit, HEADER_HEIGHT_DIP};
use std::cell::RefCell;
use std::ffi::{c_int, c_void, CStr};
use std::time::Duration;
use tomoyo_protocol::{AttachedContext, SearchFilter};

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
        && !st.menu_closed_at.is_some_and(|t| t.elapsed() < Duration::from_millis(220))
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
            9 => {
                st.shutdown_clean();
                unsafe { tomoyo_linux_ui_quit(); }
            }
            id if id >= 100 => st.execute_plugin_tray_action((id - 100) as usize),
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
    let filter_h = st.filter_bar_height_dip() as f32;
    let list_top = header_h + filter_h + 4.0;
    let row_h = st.typography.row_height_dip() as f32;
    let row_from_y = (yd >= list_top).then(|| st.scroll_offset + ((yd - list_top) / row_h) as usize).filter(|&i| i < st.results.len());

    match kind {
        6 => {
            let max_off = st.results.len().saturating_sub(st.max_visible_rows);
            st.scroll_offset = if delta > 0 { st.scroll_offset.saturating_sub(1) } else { (st.scroll_offset + 1).min(max_off) };
            st.invalidate();
        }
        5 => {
            st.hovered = row_from_y;
            let hit = if st.is_filter_bar_visible() && yd >= header_h && yd < header_h + filter_h {
                let tabs = st.visible_filter_tabs();
                compute_filter_bar_layout(
                    st.list_width_dip as f32,
                    header_h,
                    &tabs,
                    st.active_filter,
                    st.filter_counts.as_ref(),
                    st.filter_bar_wrap,
                )
                .hit_test(xd, yd)
            } else {
                None
            };
            st.hovered_filter = match hit {
                Some(FilterHit::Tab(f)) => Some(f),
                _ => None,
            };
            st.hovered_overflow = matches!(hit, Some(FilterHit::Overflow));
            st.invalidate();
        }
        1 | 2 => {
            if yd < header_h && xd < 56.0 {
                st.show_launcher_context_menu(false);
                return;
            }
            if kind == 1 && yd < header_h {
                unsafe { tomoyo_linux_ui_begin_drag(st.ui); }
                return;
            }
            if st.is_filter_bar_visible() && yd >= header_h && yd < header_h + filter_h {
                let tabs = st.visible_filter_tabs();
                let fl = compute_filter_bar_layout(
                    st.list_width_dip as f32,
                    header_h,
                    &tabs,
                    st.active_filter,
                    st.filter_counts.as_ref(),
                    st.filter_bar_wrap,
                );
                match fl.hit_test(xd, yd) {
                    Some(FilterHit::Tab(f)) => st.set_active_filter(f),
                    Some(FilterHit::Overflow) => {
                        if let Some(first) = fl.overflow_tabs.first() {
                            st.set_active_filter(first.filter);
                        }
                    }
                    None => {}
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
            if yd < header_h && xd < 56.0 { st.show_launcher_context_menu(false); }
            else if yd < header_h { st.show_launcher_context_menu(true); }
            else if let Some(i) = row_from_y {
                st.selected = i;
                st.action_menu_open = true;
                st.selected_action = 0;
                st.invalidate();
            } else {
                st.show_launcher_context_menu(false);
            }
        }
        7 => st.show_launcher_context_menu(true),
        _ => {}
    }
}

impl AppCore {
    fn ensure_selected_visible(&mut self) {
        if self.selected < self.scroll_offset { self.scroll_offset = self.selected; }
        else if self.selected >= self.scroll_offset + self.max_visible_rows {
            self.scroll_offset = self.selected + 1 - self.max_visible_rows;
        }
    }
}
