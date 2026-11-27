//! Linux event handlers: keyboard nav, mouse hit-tests, popup menus, clipboard capture & flows.

use crate::actions::{clipboard_has_rich_attachment, read_clipboard_files, read_clipboard_image_png};
use crate::linux::{
    tomoyo_linux_ui_begin_drag, tomoyo_linux_ui_get_entry_text, tomoyo_linux_ui_quit,
    tomoyo_linux_ui_set_cursor,
};
use crate::linux::ipc::schedule_on_ui;
use crate::linux::shell_state::{with_current_core, AppCore};
use crate::renderer::{hit_test_sidebar, SidebarHit, FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP, SIDEBAR_WIDTH_DIP};
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

pub unsafe extern "C" fn on_configure_cb(
    ctx: *mut c_void,
    x: c_int,
    y: c_int,
    w: c_int,
    h: c_int,
    flags: c_int,
) {
    if let Some(cell) = borrow_core(ctx) {
        if let Ok(mut st) = cell.try_borrow_mut() {
            st.handle_window_configure(x, y, w, h, flags);
        } else {
            schedule_on_ui(move || with_current_core(|st| st.handle_window_configure(x, y, w, h, flags)));
        }
    }
}

pub unsafe extern "C" fn on_kwin_pos_cb(
    _ctx: *mut c_void,
    x: c_int,
    y: c_int,
    w: c_int,
    h: c_int,
) {
    schedule_on_ui(move || {
        with_current_core(|st| st.handle_kwin_pos_report(x, y, w, h));
    });
}

pub unsafe extern "C" fn on_focus_out_cb(ctx: *mut c_void) {
    if let Some(cell) = borrow_core(ctx)
        && let Ok(mut st) = cell.try_borrow_mut()
        && !st.menu_open
        && !st.sidebar.modal_open
        && !st.dragging_divider
        && st.scrollbar.dragging.is_none()
        && !st.last_geom_change_at.is_some_and(|t| t.elapsed() < Duration::from_millis(250))
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
            3 => st.toggle_hotkeys_paused(),
            4 => st.handle_tray_update_click(),
            5 => st.open_super_panel(),
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
            if st.cancel_sidebar_tab_drag() { st.invalidate(); }
            else if st.action_menu_open { st.action_menu_open = false; st.invalidate(); }
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
    let (xd, yd) = (x as f32, y as f32);
    let header_h = HEADER_HEIGHT_DIP as f32;
    let fy0 = (st.desired_height_dip() - FOOTER_HEIGHT_DIP) as f32;
    let list_top = header_h + 4.0;
    let (lx0, lx1) = st.list_bounds_dip(st.desired_width_dip() as f32);
    let row_h = st.typography.row_height_dip() as f32;
    let vis_cap = st.visible_row_capacity();
    let row_from_y = ((lx0..lx1).contains(&xd) && yd >= list_top && yd < fy0)
        .then(|| ((yd - list_top) / row_h) as usize)
        .filter(|&slot| slot < vis_cap)
        .map(|slot| st.scroll_offset + slot)
        .filter(|&i| i < st.results.len());
    let sidebar_hit = if st.is_filter_bar_visible() && xd < SIDEBAR_WIDTH_DIP && yd >= header_h && yd < fy0 {
        let tabs = st.visible_filter_tabs();
        hit_test_sidebar(xd, yd, header_h, fy0, &tabs, st.sidebar.scroll)
    } else {
        None
    };

    match kind {
        4 => {
            if st.handle_scrollbar_release(xd, yd) {
                return;
            }
            if std::mem::replace(&mut st.dragging_divider, false) {
                st.persist_window_geometry();
            }
            let _ = st.finish_sidebar_tab_drag();
        }
        6 => {
            if st.is_filter_bar_visible() && xd < SIDEBAR_WIDTH_DIP && yd >= header_h && yd < fy0 {
                st.scroll_sidebar(delta);
            } else if !st.results.is_empty() {
                if let Some(sb) = st.current_scrollbar_metrics() {
                    let step = 2usize;
                    let next = if delta > 0 {
                        st.scroll_offset.saturating_sub(step)
                    } else {
                        (st.scroll_offset + step).min(sb.max_scroll)
                    };
                    if next != st.scroll_offset {
                        st.scroll_offset = next;
                        st.hovered = ((lx0..lx1).contains(&xd) && yd >= list_top && yd < fy0)
                            .then(|| ((yd - list_top) / row_h) as usize)
                            .filter(|&slot| slot < vis_cap)
                            .map(|slot| st.scroll_offset + slot)
                            .filter(|&i| i < st.results.len());
                        st.invalidate();
                    }
                } else {
                    if delta < 0 && st.selected + 1 < st.results.len() {
                        st.selected += 1;
                    } else if delta > 0 && st.selected > 0 {
                        st.selected -= 1;
                    }
                    st.ensure_selected_visible();
                    st.refresh_selected_preview();
                    st.invalidate();
                }
            }
        }
        5 => {
            if st.handle_scrollbar_motion(xd, yd) {
                return;
            }
            if st.dragging_divider {
                let w_dip = st.desired_width_dip() as f32;
                let (dx0, _) = st.list_bounds_dip(w_dip);
                let max_pw = ((w_dip - dx0) - 320.0).clamp(220.0, 620.0);
                let clamped = (w_dip - xd).round().clamp(220.0, max_pw) as i32;
                if clamped != st.preview_width_dip {
                    st.preview_width_dip = clamped;
                    st.invalidate();
                }
                return;
            }
            if st.update_sidebar_tab_drag(yd) {
                return;
            }
            let sb_hover = st.current_scrollbar_metrics().and_then(|sb| sb.hit_test(xd, yd));
            if !st.ui.is_null() {
                let cur = if sb_hover.is_none() && st.is_over_preview_divider(xd, yd) { 9 } else { 0 };
                unsafe { tomoyo_linux_ui_set_cursor(st.ui, cur); }
            }
            let new_logo_hover = (10.0..=54.0).contains(&xd) && (9.0..=53.0).contains(&yd);
            let new_hovered = if sb_hover.is_some() { None } else { row_from_y };
            let new_filter_hover = match sidebar_hit { Some(SidebarHit::Tab(f)) => Some(f), _ => None };
            let new_add_hover = matches!(sidebar_hit, Some(SidebarHit::AddTag));
            if new_logo_hover != st.hovered_logo
                || new_hovered != st.hovered
                || new_filter_hover != st.hovered_filter
                || new_add_hover != st.sidebar.hovered_add_tag
                || sb_hover != st.scrollbar.hover
            {
                st.hovered_logo = new_logo_hover;
                st.hovered = new_hovered;
                st.hovered_filter = new_filter_hover;
                st.sidebar.hovered_add_tag = new_add_hover;
                st.scrollbar.hover = sb_hover;
                st.invalidate();
            }
        }
        1 | 2 => {
            if st.current_scrollbar_metrics().and_then(|sb| sb.hit_test(xd, yd)).is_some() {
                if kind == 1 {
                    let _ = st.handle_scrollbar_press(xd, yd);
                }
                return;
            }
            if kind == 1 && st.is_over_preview_divider(xd, yd) {
                st.dragging_divider = true;
                return;
            }
            if yd < header_h && xd < 56.0 {
                st.show_launcher_context_menu(false);
                return;
            }
            if kind == 1 && yd < header_h {
                unsafe { tomoyo_linux_ui_begin_drag(st.ui); }
                return;
            }
            if let Some(hit) = sidebar_hit {
                match hit {
                    SidebarHit::Tab(f) if kind == 1 => st.begin_sidebar_tab_press(f, yd),
                    SidebarHit::Tab(f) => st.set_active_filter(f),
                    SidebarHit::AddTag => st.schedule_tag_dialog(None),
                    SidebarHit::Empty => {}
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
            else if let Some(hit) = sidebar_hit { st.show_sidebar_context_menu(hit); }
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
        8 => st.handle_mouse_leave(),
        _ => {}
    }
}

impl AppCore {
    fn ensure_selected_visible(&mut self) {
        let cap = self.visible_row_capacity();
        if self.selected < self.scroll_offset { self.scroll_offset = self.selected; }
        else if self.selected >= self.scroll_offset + cap {
            self.scroll_offset = self.selected + 1 - cap;
        }
    }
}
