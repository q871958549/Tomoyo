//! Context menu actions for the left filter sidebar (hide built-in, edit/delete custom, quick add).

use super::AppShell;
use crate::popup_menu::PopupEntry;
use crate::renderer::SidebarHit;
use tomoyo_core::Lang;
use tomoyo_protocol::SearchFilter;

impl AppShell {
    pub(super) fn show_sidebar_context_menu(&mut self, hit: SidebarHit) {
        let l = self.lang;
        let tabs = self.visible_filter_tabs();
        let hidden_list = self
            .config
            .read()
            .map(|c| c.hidden_builtin_filters.clone())
            .unwrap_or_default();

        const CMD_ADD: u32 = 8501;
        const CMD_HIDE_BUILTIN: u32 = 8502;
        const CMD_EDIT_CUSTOM: u32 = 8503;
        const CMD_DEL_CUSTOM: u32 = 8504;
        const CMD_SETTINGS_FILTERS: u32 = 8505;
        const CMD_RESTORE_ALL: u32 = 8506;
        const CMD_RESTORE_BASE: u32 = 8600;

        let target_tab = match hit {
            SidebarHit::Tab(f) => Some(f),
            SidebarHit::AddTag | SidebarHit::Empty => None,
        };
        let label_for = |f: SearchFilter| -> String {
            tabs.iter()
                .find(|t| t.filter == f)
                .map(|t| t.label.clone())
                .unwrap_or_else(|| f.label(l == Lang::Zh).into())
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
                let b_lbl = SearchFilter::ALL
                    .iter()
                    .find(|f| f.id_key().eq_ignore_ascii_case(hk))
                    .map(|f| f.label(l == Lang::Zh).to_string())
                    .unwrap_or_else(|| hk.clone());
                sub.push(PopupEntry::item(CMD_RESTORE_BASE + i as u32, b_lbl, None, false));
            }
            if hidden_list.len() > 1 {
                sub.push(PopupEntry::Separator);
                sub.push(PopupEntry::item(CMD_RESTORE_ALL, l.pick("恢复全部隐藏标签", "Restore All Hidden"), None, false));
            }
            entries.push(PopupEntry::submenu(l.pick("恢复隐藏的内置标签", "Restore Hidden Built-in"), sub));
        }

        entries.push(PopupEntry::Separator);
        entries.push(PopupEntry::item(CMD_SETTINGS_FILTERS, l.pick("打开完整标签设置…", "Full Tag Settings…"), None, false));

        let Some(cmd) = self.run_tracked_popup(&entries) else { return };
        match cmd {
            CMD_ADD => self.open_custom_filter_dialog(None),
            CMD_EDIT_CUSTOM => {
                if let Some(SearchFilter::Custom(idx)) = target_tab {
                    self.open_custom_filter_dialog(Some(idx as usize));
                }
            }
            CMD_DEL_CUSTOM => {
                if let Some(SearchFilter::Custom(idx)) = target_tab {
                    self.delete_custom_filter_at(idx as usize);
                }
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
                    if self.active_filter == f {
                        self.active_filter = SearchFilter::All;
                    }
                    self.dispatch_query();
                    self.invalidate();
                }
            }
            CMD_RESTORE_ALL => {
                if let Ok(mut cfg) = self.config.write() {
                    cfg.hidden_builtin_filters.clear();
                    let _ = cfg.save();
                }
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
            } else {
                None
            }
        });
        if let Some(lbl) = removed_name {
            match self.active_filter {
                SearchFilter::Custom(cur) if cur as usize == idx => self.active_filter = SearchFilter::All,
                SearchFilter::Custom(cur) if (cur as usize) > idx => {
                    self.active_filter = SearchFilter::Custom(cur - 1);
                }
                _ => {}
            }
            self.dispatch_query();
            self.invalidate();
            self.show_toast(&format!("{}: {lbl}", self.lang.pick("已删除标签", "Deleted tag")));
        }
    }
}
