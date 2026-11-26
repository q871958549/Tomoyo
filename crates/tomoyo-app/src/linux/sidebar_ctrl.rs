//! Linux left filter sidebar interaction: drag-to-reorder, scroll, right-click menu & tag modal.

use crate::linux::shell_state::{with_current_core, AppCore};
use crate::linux::{
    tomoyo_linux_schedule_idle, tomoyo_linux_show_popup, tomoyo_linux_show_tag_dialog,
    tomoyo_linux_ui_focus_entry, LinuxTrayMenuItem,
};
use crate::renderer::{
    compute_drag_drop_slot, compute_sidebar_layout, SidebarDragVisual, SidebarHit,
    FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP,
};
use std::ffi::{c_void, CStr, CString};
use std::time::Instant;
use tomoyo_core::{CustomFilterTab, Lang};
use tomoyo_protocol::SearchFilter;

const CMD_SB_ADD: u32 = 8501;
const CMD_SB_HIDE_BUILTIN: u32 = 8502;
const CMD_SB_EDIT_CUSTOM: u32 = 8503;
const CMD_SB_DEL_CUSTOM: u32 = 8504;
const CMD_SB_SETTINGS_FILTERS: u32 = 8505;
const CMD_SB_RESTORE_ALL: u32 = 8506;
const CMD_SB_RESET_ORDER: u32 = 8507;
const CMD_SB_RESTORE_BASE: u32 = 8600;

#[derive(Debug, Clone, Copy)]
pub(super) struct SidebarDragState {
    pub filter: SearchFilter,
    pub start_y_dip: f32,
    pub cur_y_dip: f32,
    pub grab_off_dip: f32,
    pub active: bool,
    pub insert_slot: usize,
}

#[derive(Default)]
pub(super) struct SidebarState {
    pub scroll: f32,
    pub hovered_add_tag: bool,
    pub drag: Option<SidebarDragState>,
    pub menu_target: Option<SearchFilter>,
    pub modal_open: bool,
}

pub(super) fn reorder_visible_keys(
    vis_keys: &[String],
    from_idx: usize,
    insert_slot: usize,
    prev_order: &[String],
) -> Option<Vec<String>> {
    if from_idx == 0 || from_idx >= vis_keys.len() || insert_slot == 0 || insert_slot > vis_keys.len() {
        return None;
    }
    if insert_slot == from_idx || insert_slot == from_idx + 1 {
        return None;
    }
    let mut moved: Vec<String> = vis_keys.iter().skip(1).cloned().collect();
    let src_sub = from_idx - 1;
    let dst_sub = if insert_slot > from_idx { insert_slot - 2 } else { insert_slot - 1 };
    let key = moved.remove(src_sub);
    moved.insert(dst_sub.min(moved.len()), key);
    for old in prev_order {
        if !old.eq_ignore_ascii_case("all") && !moved.iter().any(|k| k.eq_ignore_ascii_case(old)) {
            moved.push(old.clone());
        }
    }
    Some(moved)
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

unsafe extern "C" fn on_sidebar_menu_picked(_ctx: *mut c_void, id: u32) {
    with_current_core(|st| match id {
        0 => {
            st.menu_open = false;
            st.menu_closed_at = Some(Instant::now());
            if st.is_visible() && !st.ui.is_null() && !st.sidebar.modal_open {
                unsafe { tomoyo_linux_ui_focus_entry(st.ui); }
            }
        }
        0xFFFFFFFE => {
            st.menu_open = false;
            st.menu_closed_at = Some(Instant::now());
            if !st.sidebar.modal_open && st.menu_refocus && st.is_visible() && !st.ui.is_null() {
                st.menu_refocus = false;
                unsafe { tomoyo_linux_ui_focus_entry(st.ui); }
            }
        }
        _ => st.apply_sidebar_menu_command(id),
    });
}

unsafe extern "C" fn run_tag_dialog_idle(ud: *mut c_void) {
    let edit_idx = (ud as usize).checked_sub(1);
    let mut params = None;
    with_current_core(|st| {
        if st.shutting_down {
            st.sidebar.modal_open = false;
            return;
        }
        let is_en = st.lang == Lang::En;
        let tmode = if st.theme_name.eq_ignore_ascii_case("light") { 0 } else if st.theme_name.eq_ignore_ascii_case("midnight") { 2 } else { 1 };
        let existing = edit_idx.and_then(|i| st.config.read().ok()?.custom_filters.get(i).cloned());
        params = Some((st.ui, is_en, tmode, existing));
    });
    let Some((ui, is_en, tmode, existing)) = params else { return };
    let is_edit = i32::from(existing.is_some());
    let c_lbl = CString::new(existing.as_ref().map(|t| t.label.as_str()).unwrap_or("")).unwrap_or_default();
    let c_ext = CString::new(existing.as_ref().map(|t| t.extensions.join(", ")).unwrap_or_default()).unwrap_or_default();
    let c_exp = CString::new(existing.as_ref().map(|t| t.everything_expr.as_str()).unwrap_or("")).unwrap_or_default();
    let (mut ob_lbl, mut ob_ext, mut ob_exp) = ([0i8; 256], [0i8; 512], [0i8; 512]);
    let saved = unsafe {
        tomoyo_linux_show_tag_dialog(
            ui, is_edit, tmode, i32::from(is_en),
            c_lbl.as_ptr(), c_ext.as_ptr(), c_exp.as_ptr(),
            ob_lbl.as_mut_ptr(), ob_lbl.len(),
            ob_ext.as_mut_ptr(), ob_ext.len(),
            ob_exp.as_mut_ptr(), ob_exp.len(),
        )
    } != 0;
    let out_label = unsafe { CStr::from_ptr(ob_lbl.as_ptr()) }.to_string_lossy().trim().to_string();
    let out_exts = parse_extensions(&unsafe { CStr::from_ptr(ob_ext.as_ptr()) }.to_string_lossy());
    let out_expr = unsafe { CStr::from_ptr(ob_exp.as_ptr()) }.to_string_lossy().trim().to_string();

    with_current_core(|st| {
        st.sidebar.modal_open = false;
        st.menu_closed_at = Some(Instant::now());
        if saved && !out_label.is_empty() && (!out_exts.is_empty() || !out_expr.is_empty()) {
            let target_filter = st.config.write().ok().and_then(|mut cfg| {
                let idx = if let Some(ei) = edit_idx && let Some(tab) = cfg.custom_filters.get_mut(ei) {
                    tab.label = out_label.clone();
                    tab.extensions = out_exts;
                    tab.everything_expr = out_expr;
                    tab.enabled = true;
                    let _ = cfg.sanitize();
                    let _ = cfg.save();
                    ei
                } else if cfg.custom_filters.len() < 12 {
                    let i = cfg.custom_filters.len();
                    let slug = format!(
                        "custom_{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() % 100000)
                            .unwrap_or(i as u128)
                    );
                    let tag_hint = out_exts.first().cloned().unwrap_or_else(|| slug.clone());
                    cfg.custom_filters.push(CustomFilterTab {
                        id: slug,
                        label: out_label.clone(),
                        tag: format!("@{tag_hint}"),
                        extensions: out_exts,
                        everything_expr: out_expr,
                        enabled: true,
                    });
                    let _ = cfg.sanitize();
                    let _ = cfg.save();
                    cfg.custom_filters.len().saturating_sub(1)
                } else {
                    return None;
                };
                Some(SearchFilter::Custom(idx as u8))
            });
            if let Some(f) = target_filter {
                st.active_filter = f;
                st.show_filter_bar = true;
                st.selected = 0;
                st.scroll_offset = 0;
                st.dispatch_query();
                st.show_toast(&format!("{}「{out_label}」", st.lang.pick("已保存标签：", "Saved tag: ")));
            }
        }
        if st.is_visible() && !st.ui.is_null() {
            unsafe { tomoyo_linux_ui_focus_entry(st.ui); }
        }
    });
}

impl AppCore {
    pub(super) fn sidebar_drag_visual(&self) -> Option<SidebarDragVisual> {
        self.sidebar.drag.filter(|d| d.active).map(|d| SidebarDragVisual {
            dragged: d.filter,
            ghost_y0: d.cur_y_dip - d.grab_off_dip,
            insert_slot: d.insert_slot,
        })
    }

    pub(super) fn begin_sidebar_tab_press(&mut self, filter: SearchFilter, y_dip: f32) {
        if filter == SearchFilter::All {
            self.set_active_filter(SearchFilter::All);
            return;
        }
        let fy0 = (self.desired_height_dip() - FOOTER_HEIGHT_DIP) as f32;
        let tabs = self.visible_filter_tabs();
        let sl = compute_sidebar_layout(HEADER_HEIGHT_DIP as f32, fy0, &tabs, self.sidebar.scroll);
        let Some((idx, item)) = sl.items.iter().enumerate().find(|(_, it)| it.filter == filter) else {
            self.set_active_filter(filter);
            return;
        };
        let grab_off_dip = (y_dip - item.y0).clamp(4.0, 26.0);
        self.sidebar.drag = Some(SidebarDragState {
            filter,
            start_y_dip: y_dip,
            cur_y_dip: y_dip,
            grab_off_dip,
            active: false,
            insert_slot: idx,
        });
    }

    pub(super) fn update_sidebar_tab_drag(&mut self, y_dip: f32) -> bool {
        let Some(mut d) = self.sidebar.drag else { return false };
        d.cur_y_dip = y_dip;
        if !d.active && (y_dip - d.start_y_dip).abs() >= 6.0 {
            d.active = true;
            self.hovered = None;
            self.hovered_filter = None;
            self.sidebar.hovered_add_tag = false;
        }
        if d.active {
            let fy0 = (self.desired_height_dip() - FOOTER_HEIGHT_DIP) as f32;
            let tabs = self.visible_filter_tabs();
            let mut sl = compute_sidebar_layout(HEADER_HEIGHT_DIP as f32, fy0, &tabs, self.sidebar.scroll);
            if sl.max_scroll > 0.0 {
                let delta = if y_dip < sl.list_top + 16.0 { -7.0 } else if y_dip > sl.list_bot - 16.0 { 7.0 } else { 0.0 };
                if delta != 0.0 {
                    self.sidebar.scroll = (self.sidebar.scroll + delta).clamp(0.0, sl.max_scroll);
                    sl = compute_sidebar_layout(HEADER_HEIGHT_DIP as f32, fy0, &tabs, self.sidebar.scroll);
                }
            }
            d.insert_slot = compute_drag_drop_slot(y_dip, &sl);
            self.sidebar.drag = Some(d);
            self.invalidate();
        } else {
            self.sidebar.drag = Some(d);
        }
        true
    }

    pub(super) fn finish_sidebar_tab_drag(&mut self) -> bool {
        let Some(d) = self.sidebar.drag.take() else { return false };
        if d.active { self.commit_sidebar_reorder(d.filter, d.insert_slot); } else { self.set_active_filter(d.filter); }
        self.invalidate();
        true
    }

    pub(super) fn cancel_sidebar_tab_drag(&mut self) -> bool {
        self.sidebar.drag.take().is_some_and(|d| d.active)
    }

    pub(super) fn scroll_sidebar(&mut self, delta: i32) {
        let fy0 = (self.desired_height_dip() - FOOTER_HEIGHT_DIP) as f32;
        let tabs = self.visible_filter_tabs();
        let sl = compute_sidebar_layout(HEADER_HEIGHT_DIP as f32, fy0, &tabs, self.sidebar.scroll);
        self.sidebar.scroll = (self.sidebar.scroll - (delta as f32) * 32.0).clamp(0.0, sl.max_scroll);
        self.invalidate();
    }

    fn commit_sidebar_reorder(&mut self, filter: SearchFilter, insert_slot: usize) {
        let tabs = self.visible_filter_tabs();
        let Some(from_idx) = tabs.iter().position(|t| t.filter == filter) else { return };
        let label = tabs.get(from_idx).map(|t| t.label.clone()).unwrap_or_default();
        let moved = self.config.write().ok().and_then(|mut cfg| {
            let vis_keys: Vec<String> = tabs.iter().map(|t| t.filter.order_key(&cfg.custom_filters)).collect();
            let next_order = reorder_visible_keys(&vis_keys, from_idx, insert_slot, &cfg.filter_tab_order)?;
            cfg.filter_tab_order = next_order;
            let _ = cfg.sanitize();
            let _ = cfg.save();
            Some(())
        });
        if moved.is_some() {
            self.show_toast(&format!("{}「{label}」", self.lang.pick("已调整顺序：", "Reordered ")));
        }
    }

    pub(super) fn schedule_tag_dialog(&mut self, edit_idx: Option<usize>) {
        self.sidebar.modal_open = true;
        let encoded = edit_idx.map(|i| i + 1).unwrap_or(0) as *mut c_void;
        unsafe { tomoyo_linux_schedule_idle(run_tag_dialog_idle, encoded); }
    }

    pub(super) fn show_sidebar_context_menu(&mut self, hit: SidebarHit) {
        let l = self.lang;
        let tabs = self.visible_filter_tabs();
        let (hidden_list, has_custom_order) = self
            .config
            .read()
            .map(|c| (c.hidden_builtin_filters.clone(), !c.filter_tab_order.is_empty()))
            .unwrap_or_default();
        let target_tab = match hit { SidebarHit::Tab(f) => Some(f), SidebarHit::AddTag | SidebarHit::Empty => None };
        self.sidebar.menu_target = target_tab;
        let label_for = |f: SearchFilter| -> String {
            tabs.iter().find(|t| t.filter == f).map(|t| t.label.clone()).unwrap_or_else(|| f.label(l == Lang::Zh).into())
        };

        let mut owned: Vec<(CString, u32, i32, bool, bool)> = Vec::new();
        let mut push = |lbl: String, cmd: u32| {
            owned.push((CString::new(lbl).unwrap_or_default(), cmd, -1, false, false));
        };
        if let Some(f) = target_tab {
            let lbl = label_for(f);
            match f {
                SearchFilter::Custom(_) => {
                    push(format!("{}「{lbl}」…", l.pick("编辑标签", "Edit Tag")), CMD_SB_EDIT_CUSTOM);
                    push(format!("{}「{lbl}」", l.pick("删除标签", "Delete Tag")), CMD_SB_DEL_CUSTOM);
                    push("---".into(), 0);
                }
                SearchFilter::All => {}
                _ => {
                    push(format!("{}「{lbl}」", l.pick("隐藏内置标签", "Hide Built-in Tag")), CMD_SB_HIDE_BUILTIN);
                    push("---".into(), 0);
                }
            }
        }
        push(l.pick("新建自定义标签…", "New Custom Tag…").into(), CMD_SB_ADD);
        push(l.pick("打开设置管理全部标签…", "Manage Tags in Settings…").into(), CMD_SB_SETTINGS_FILTERS);
        if has_custom_order {
            push(l.pick("重置标签排序", "Reset Tab Order").into(), CMD_SB_RESET_ORDER);
        }
        if !hidden_list.is_empty() {
            push("---".into(), 0);
            for (i, hk) in hidden_list.iter().enumerate() {
                let b_lbl = SearchFilter::ALL
                    .iter()
                    .find(|f| f.id_key().eq_ignore_ascii_case(hk))
                    .map(|f| f.label(l == Lang::Zh).to_string())
                    .unwrap_or_else(|| hk.clone());
                push(format!("{}「{b_lbl}」", l.pick("恢复显示", "Restore")), CMD_SB_RESTORE_BASE + i as u32);
            }
            push(l.pick("恢复全部隐藏的内置标签", "Restore All Built-in Tags").into(), CMD_SB_RESTORE_ALL);
        }
        let items: Vec<LinuxTrayMenuItem> = owned
            .iter()
            .map(|(cs, cmd, p, ch, dis)| LinuxTrayMenuItem {
                label: cs.as_ptr(), cmd: *cmd, parent_idx: *p, checked: i32::from(*ch), disabled: i32::from(*dis),
            })
            .collect();
        self.menu_open = true;
        self.menu_closed_at = None;
        self.menu_refocus = true;
        unsafe { tomoyo_linux_show_popup(self.ui, items.as_ptr(), items.len(), on_sidebar_menu_picked, std::ptr::null_mut()); }
    }

    pub(super) fn apply_sidebar_menu_command(&mut self, cmd: u32) {
        let target_tab = self.sidebar.menu_target.take();
        let zh = self.lang == Lang::Zh;
        match cmd {
            CMD_SB_ADD => { self.menu_refocus = false; self.schedule_tag_dialog(None); }
            CMD_SB_EDIT_CUSTOM => {
                self.menu_refocus = false;
                if let Some(SearchFilter::Custom(idx)) = target_tab { self.schedule_tag_dialog(Some(idx as usize)); }
            }
            CMD_SB_DEL_CUSTOM => {
                if let Some(SearchFilter::Custom(idx)) = target_tab {
                    let i = idx as usize;
                    let removed = self.config.write().ok().and_then(|mut cfg| {
                        (i < cfg.custom_filters.len()).then(|| {
                            let lbl = cfg.custom_filters.remove(i).label;
                            let _ = cfg.sanitize();
                            let _ = cfg.save();
                            lbl
                        })
                    });
                    if let Some(lbl) = removed {
                        match self.active_filter {
                            SearchFilter::Custom(cur) if cur as usize == i => self.active_filter = SearchFilter::All,
                            SearchFilter::Custom(cur) if (cur as usize) > i => self.active_filter = SearchFilter::Custom(cur - 1),
                            _ => {}
                        }
                        self.dispatch_query();
                        self.show_toast(&format!("{}「{lbl}」", if zh { "已删除标签：" } else { "Deleted tag: " }));
                    }
                }
            }
            CMD_SB_HIDE_BUILTIN => {
                if let Some(f) = target_tab && f != SearchFilter::All {
                    if let Ok(mut cfg) = self.config.write() {
                        let key = f.id_key().to_string();
                        if !cfg.hidden_builtin_filters.iter().any(|k| k.eq_ignore_ascii_case(&key)) {
                            cfg.hidden_builtin_filters.push(key);
                            let _ = cfg.sanitize();
                            let _ = cfg.save();
                        }
                    }
                    if self.active_filter == f { self.active_filter = SearchFilter::All; }
                    self.dispatch_query();
                }
            }
            CMD_SB_SETTINGS_FILTERS => { self.menu_refocus = false; self.open_settings_center_at(Some("search:filters")); }
            CMD_SB_RESET_ORDER => {
                if let Ok(mut cfg) = self.config.write() { cfg.filter_tab_order.clear(); let _ = cfg.save(); }
                self.invalidate();
                self.show_toast(if zh { "已恢复默认标签顺序" } else { "Restored default tab order" });
            }
            CMD_SB_RESTORE_ALL => {
                if let Ok(mut cfg) = self.config.write() { cfg.hidden_builtin_filters.clear(); let _ = cfg.save(); }
                self.dispatch_query();
            }
            c if c >= CMD_SB_RESTORE_BASE => {
                let idx = (c - CMD_SB_RESTORE_BASE) as usize;
                let removed = if let Ok(mut cfg) = self.config.write() && idx < cfg.hidden_builtin_filters.len() {
                    cfg.hidden_builtin_filters.remove(idx);
                    let _ = cfg.save();
                    true
                } else {
                    false
                };
                if removed {
                    self.dispatch_query();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::reorder_visible_keys;

    #[test]
    fn reorders_visible_tabs_without_displacing_pinned_all() {
        let vis = vec!["all".into(), "app".into(), "file".into(), "custom:0".into()];
        let next = reorder_visible_keys(&vis, 3, 1, &["hidden_extra".into()]).unwrap();
        assert_eq!(next, vec!["custom:0", "app", "file", "hidden_extra"]);
        assert!(reorder_visible_keys(&vis, 0, 2, &[]).is_none());
        assert!(reorder_visible_keys(&vis, 1, 1, &[]).is_none());
        assert!(reorder_visible_keys(&vis, 1, 2, &[]).is_none());
    }
}
