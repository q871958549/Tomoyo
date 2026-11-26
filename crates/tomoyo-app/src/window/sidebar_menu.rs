//! Left filter sidebar drag-to-reorder state machine and right-click management menu.

use super::AppShell;
use crate::popup_menu::PopupEntry;
use crate::renderer::{
    compute_drag_drop_slot, compute_sidebar_layout, SidebarDragVisual, SidebarHit,
    FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP,
};
use tomoyo_core::Lang;
use tomoyo_protocol::SearchFilter;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SidebarDragState {
    pub filter: SearchFilter,
    pub start_y_dip: f32,
    pub cur_y_dip: f32,
    pub grab_off_dip: f32,
    pub active: bool,
    pub insert_slot: usize,
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

impl AppShell {
    pub(super) fn sidebar_drag_visual(&self) -> Option<SidebarDragVisual> {
        self.inter.sidebar_drag.filter(|d| d.active).map(|d| SidebarDragVisual {
            dragged: d.filter,
            ghost_y0: d.cur_y_dip - d.grab_off_dip,
            insert_slot: d.insert_slot,
        })
    }

    pub(super) fn begin_sidebar_tab_press(&mut self, hwnd: HWND, filter: SearchFilter, y_dip: f32) {
        if filter == SearchFilter::All {
            self.set_active_filter(SearchFilter::All);
            unsafe { let _ = SetFocus(Some(self.edit_hwnd)); }
            return;
        }
        let (_, h_dip) = self.current_client_dip();
        let tabs = self.visible_filter_tabs();
        let sl = compute_sidebar_layout(HEADER_HEIGHT_DIP as f32, h_dip - FOOTER_HEIGHT_DIP as f32, &tabs, self.inter.sidebar_scroll);
        let Some((idx, item)) = sl.items.iter().enumerate().find(|(_, it)| it.filter == filter) else {
            self.set_active_filter(filter);
            return;
        };
        let grab_off_dip = (y_dip - item.y0).clamp(4.0, 26.0);
        self.inter.sidebar_drag = Some(SidebarDragState {
            filter,
            start_y_dip: y_dip,
            cur_y_dip: y_dip,
            grab_off_dip,
            active: false,
            insert_slot: idx,
        });
        unsafe { SetCapture(hwnd); }
    }

    pub(super) fn update_sidebar_tab_drag(&mut self, y_dip: f32) -> bool {
        let Some(mut d) = self.inter.sidebar_drag else { return false };
        d.cur_y_dip = y_dip;
        if !d.active && (y_dip - d.start_y_dip).abs() >= 6.0 {
            d.active = true;
            self.inter.hovered = None;
            self.inter.hovered_filter = None;
            self.inter.hovered_add_tag = false;
        }
        if d.active {
            let (_, h_dip) = self.current_client_dip();
            let tabs = self.visible_filter_tabs();
            let fy0 = h_dip - FOOTER_HEIGHT_DIP as f32;
            let mut sl = compute_sidebar_layout(HEADER_HEIGHT_DIP as f32, fy0, &tabs, self.inter.sidebar_scroll);
            if sl.max_scroll > 0.0 {
                let delta = if y_dip < sl.list_top + 16.0 {
                    -7.0
                } else if y_dip > sl.list_bot - 16.0 {
                    7.0
                } else {
                    0.0
                };
                if delta != 0.0 {
                    self.inter.sidebar_scroll = (self.inter.sidebar_scroll + delta).clamp(0.0, sl.max_scroll);
                    sl = compute_sidebar_layout(HEADER_HEIGHT_DIP as f32, fy0, &tabs, self.inter.sidebar_scroll);
                }
            }
            d.insert_slot = compute_drag_drop_slot(y_dip, &sl);
            self.inter.sidebar_drag = Some(d);
            self.invalidate();
        } else {
            self.inter.sidebar_drag = Some(d);
        }
        true
    }

    pub(super) fn finish_sidebar_tab_drag(&mut self) -> bool {
        let Some(d) = self.inter.sidebar_drag.take() else { return false };
        unsafe {
            let _ = ReleaseCapture();
            let _ = SetFocus(Some(self.edit_hwnd));
        }
        if d.active {
            self.commit_sidebar_reorder(d.filter, d.insert_slot);
        } else {
            self.set_active_filter(d.filter);
        }
        self.invalidate();
        true
    }

    pub(super) fn cancel_sidebar_tab_drag(&mut self) -> bool {
        self.inter.sidebar_drag.take().is_some_and(|d| d.active)
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

    pub(super) fn show_sidebar_context_menu(&mut self, hit: SidebarHit) {
        let l = self.lang;
        let tabs = self.visible_filter_tabs();
        let (hidden_list, has_custom_order) = self
            .config
            .read()
            .map(|c| (c.hidden_builtin_filters.clone(), !c.filter_tab_order.is_empty()))
            .unwrap_or_default();

        const CMD_ADD: u32 = 8501;
        const CMD_HIDE_BUILTIN: u32 = 8502;
        const CMD_EDIT_CUSTOM: u32 = 8503;
        const CMD_DEL_CUSTOM: u32 = 8504;
        const CMD_SETTINGS_FILTERS: u32 = 8505;
        const CMD_RESTORE_ALL: u32 = 8506;
        const CMD_RESET_ORDER: u32 = 8507;
        const CMD_RESTORE_BASE: u32 = 8600;

        let target_tab = match hit {
            SidebarHit::Tab(f) => Some(f),
            SidebarHit::AddTag | SidebarHit::Empty => None,
        };
        let label_for = |f: SearchFilter| -> String {
            tabs.iter().find(|t| t.filter == f).map(|t| t.label.clone()).unwrap_or_else(|| f.label(l == Lang::Zh).into())
        };

        let mut entries: Vec<PopupEntry> = Vec::new();
        if let Some(f) = target_tab {
            let lbl = label_for(f);
            match f {
                SearchFilter::Custom(_) => {
                    entries.push(PopupEntry::item(CMD_EDIT_CUSTOM, format!("{}「{lbl}」…", l.pick("编辑标签", "Edit Tag")), None, false));
                    entries.push(PopupEntry::danger_item(CMD_DEL_CUSTOM, format!("{}「{lbl}」", l.pick("删除标签", "Delete Tag")), None));
                    entries.push(PopupEntry::Separator);
                }
                SearchFilter::All => {}
                _ => {
                    entries.push(PopupEntry::item(CMD_HIDE_BUILTIN, format!("{}「{lbl}」", l.pick("隐藏内置标签", "Hide Built-in Tag")), None, false));
                    entries.push(PopupEntry::Separator);
                }
            }
        }
        entries.push(PopupEntry::item(CMD_ADD, l.pick("+ 添加自定义标签…", "+ Add Custom Tag…"), None, false));

        if !hidden_list.is_empty() {
            let mut sub: Vec<PopupEntry> = Vec::new();
            for (i, hk) in hidden_list.iter().enumerate() {
                let b_lbl = SearchFilter::ALL.iter().find(|f| f.id_key().eq_ignore_ascii_case(hk))
                    .map(|f| f.label(l == Lang::Zh).to_string()).unwrap_or_else(|| hk.clone());
                sub.push(PopupEntry::item(CMD_RESTORE_BASE + i as u32, b_lbl, None, false));
            }
            if hidden_list.len() > 1 {
                sub.push(PopupEntry::Separator);
                sub.push(PopupEntry::item(CMD_RESTORE_ALL, l.pick("恢复全部隐藏标签", "Restore All Hidden"), None, false));
            }
            entries.push(PopupEntry::submenu(l.pick("恢复隐藏的内置标签", "Restore Hidden Built-in"), sub));
        }

        if has_custom_order {
            entries.push(PopupEntry::item(CMD_RESET_ORDER, l.pick("重置标签排序", "Reset Tab Order"), None, false));
        }
        entries.push(PopupEntry::Separator);
        entries.push(PopupEntry::item(CMD_SETTINGS_FILTERS, l.pick("打开完整标签设置…", "Full Tag Settings…"), None, false));

        let Some(cmd) = self.run_tracked_popup(&entries) else { return };
        match cmd {
            CMD_ADD => self.open_custom_filter_dialog(None),
            CMD_EDIT_CUSTOM => {
                if let Some(SearchFilter::Custom(idx)) = target_tab { self.open_custom_filter_dialog(Some(idx as usize)); }
            }
            CMD_DEL_CUSTOM => {
                if let Some(SearchFilter::Custom(idx)) = target_tab { self.delete_custom_filter_at(idx as usize); }
            }
            CMD_HIDE_BUILTIN => {
                if let Some(f) = target_tab && f != SearchFilter::All {
                    if let Ok(mut cfg) = self.config.write() {
                        let key = f.id_key().to_string();
                        if !cfg.hidden_builtin_filters.iter().any(|h| h.eq_ignore_ascii_case(&key)) {
                            cfg.hidden_builtin_filters.push(key);
                        }
                        let _ = cfg.sanitize();
                        let _ = cfg.save();
                    }
                    if self.active_filter == f { self.active_filter = SearchFilter::All; }
                    self.dispatch_query();
                    self.invalidate();
                }
            }
            CMD_RESTORE_ALL => {
                if let Ok(mut cfg) = self.config.write() { cfg.hidden_builtin_filters.clear(); let _ = cfg.save(); }
                self.invalidate();
            }
            CMD_RESET_ORDER => {
                if let Ok(mut cfg) = self.config.write() { cfg.filter_tab_order.clear(); let _ = cfg.save(); }
                self.show_toast(l.pick("已恢复默认标签顺序", "Reset default tab order"));
                self.invalidate();
            }
            CMD_SETTINGS_FILTERS => self.open_settings_center_at(Some("search:filters")),
            c if c >= CMD_RESTORE_BASE => {
                let i = (c - CMD_RESTORE_BASE) as usize;
                if let Ok(mut cfg) = self.config.write() && i < cfg.hidden_builtin_filters.len() {
                    cfg.hidden_builtin_filters.remove(i);
                    let _ = cfg.save();
                }
                self.invalidate();
            }
            _ => {}
        }
    }

    fn delete_custom_filter_at(&mut self, idx: usize) {
        let removed_name = self.config.write().ok().and_then(|mut cfg| {
            if idx < cfg.custom_filters.len() {
                let rem = cfg.custom_filters.remove(idx);
                let _ = cfg.sanitize();
                let _ = cfg.save();
                Some(rem.label)
            } else { None }
        });
        if let Some(lbl) = removed_name {
            match self.active_filter {
                SearchFilter::Custom(cur) if cur as usize == idx => self.active_filter = SearchFilter::All,
                SearchFilter::Custom(cur) if (cur as usize) > idx => self.active_filter = SearchFilter::Custom(cur - 1),
                _ => {}
            }
            self.dispatch_query();
            self.invalidate();
            self.show_toast(&format!("{}: {lbl}", self.lang.pick("已删除标签", "Deleted tag")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::reorder_visible_keys;

    #[test]
    fn reorders_tabs_while_keeping_all_pinned_and_preserving_hidden_tail() {
        let vis: Vec<String> = ["all", "app", "pdf", "custom:code"].into_iter().map(Into::into).collect();
        let prev = vec!["ppt".into()];
        // Drag `custom:code` (idx 3) before `app` (slot 1)
        let res = reorder_visible_keys(&vis, 3, 1, &prev).unwrap();
        assert_eq!(res, vec!["custom:code", "app", "pdf", "ppt"]);
        // Drag `app` (idx 1) to end (slot 4)
        let down = reorder_visible_keys(&vis, 1, 4, &[]).unwrap();
        assert_eq!(down, vec!["pdf", "custom:code", "app"]);
        // No-op slots return None
        assert!(reorder_visible_keys(&vis, 1, 1, &[]).is_none());
        assert!(reorder_visible_keys(&vis, 1, 2, &[]).is_none());
    }
}
