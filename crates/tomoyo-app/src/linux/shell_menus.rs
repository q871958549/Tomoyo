//! Native GTK3 search-box & logo cascading context menu + focus-safe popup lifecycle.

use crate::actions::{
    clipboard_has_rich_attachment, read_clipboard_files, read_clipboard_image_png, set_clipboard_text,
    shell_open,
};
use crate::linux::shell_state::{with_current_core, AppCore};
use crate::linux::{
    tomoyo_linux_add_timeout, tomoyo_linux_entry_edit_cmd, tomoyo_linux_entry_has_selection,
    tomoyo_linux_schedule_idle, tomoyo_linux_ui_focus_entry, tomoyo_linux_ui_is_active,
    tomoyo_linux_ui_quit,
};
use std::ffi::{c_char, c_int, c_void, CString};
use std::time::Instant;
use tomoyo_core::{Lang, TomoyoConfig};
use tomoyo_protocol::{AttachedContext, SearchFilter};

const CMD_MENU_CLOSED_AFTER_PICK: u32 = 0xFFFF_FFFE;
const CMD_EDIT_PASTE: u32 = 10;
const CMD_EDIT_CUT: u32 = 11;
const CMD_EDIT_COPY: u32 = 12;
const CMD_EDIT_SELALL: u32 = 13;
const CMD_EDIT_CLEAR: u32 = 14;
const CMD_MODE_ALL: u32 = 20;
const CMD_MODE_EV: u32 = 21;
const CMD_MODE_CB: u32 = 22;
const CMD_MODE_WIN: u32 = 23;
const CMD_MODE_KILL: u32 = 24;
const CMD_MODE_SNIP: u32 = 25;
const CMD_MODE_CALC: u32 = 26;
const CMD_SETTINGS: u32 = 101;
const CMD_THEME_LIGHT: u32 = 102;
const CMD_THEME_DARK: u32 = 103;
const CMD_THEME_MIDNIGHT: u32 = 104;
const CMD_LANG_ZH: u32 = 105;
const CMD_LANG_EN: u32 = 106;
const CMD_PREVIEW: u32 = 107;
const CMD_RESET_WIN: u32 = 108;
const CMD_STATS: u32 = 109;
const CMD_RELOAD: u32 = 110;
const CMD_GC: u32 = 111;
const CMD_CONFIG: u32 = 112;
const CMD_QUIT: u32 = 113;
const CMD_FILTER_BAR: u32 = 115;

#[repr(C)]
struct CMenuItem {
    label: *const c_char,
    cmd: u32,
    parent_idx: i32,
    checked: i32,
    disabled: i32,
}

unsafe extern "C" {
    fn tomoyo_linux_show_popup(
        a: *mut c_void,
        items: *const CMenuItem,
        n: usize,
        cb: unsafe extern "C" fn(*mut c_void, u32),
        ctx: *mut c_void,
    );
}

struct OwnedMenu {
    _strs: Vec<CString>,
    items: Vec<CMenuItem>,
}

impl OwnedMenu {
    fn new() -> Self {
        Self { _strs: Vec::new(), items: Vec::new() }
    }

    fn push(&mut self, label: &str, cmd: u32, parent_idx: i32, checked: bool, disabled: bool) -> i32 {
        let cs = CString::new(label).unwrap_or_else(|_| CString::new("").unwrap());
        let ptr = cs.as_ptr();
        self._strs.push(cs);
        let idx = self.items.len() as i32;
        self.items.push(CMenuItem {
            label: ptr,
            cmd,
            parent_idx,
            checked: i32::from(checked),
            disabled: i32::from(disabled),
        });
        idx
    }

    fn sep(&mut self, parent_idx: i32) {
        self.push("---", 0, parent_idx, false, true);
    }

    fn submenu(&mut self, label: &str, parent_idx: i32) -> i32 {
        self.push(label, 0, parent_idx, false, false)
    }
}

unsafe extern "C" fn refocus_entry_idle(_data: *mut c_void) {
    with_current_core(|st| {
        if st.is_visible() && !st.menu_open && !st.ui.is_null() {
            unsafe { tomoyo_linux_ui_focus_entry(st.ui); }
        }
    });
}

unsafe extern "C" fn dismissed_focus_check_timer(_data: *mut c_void) -> c_int {
    with_current_core(|st| {
        if st.is_visible() && !st.menu_open && !st.ui.is_null() {
            if unsafe { tomoyo_linux_ui_is_active(st.ui) } != 0 {
                unsafe { tomoyo_linux_ui_focus_entry(st.ui); }
            } else {
                st.hide_launcher();
            }
        }
    });
    0
}

unsafe extern "C" fn on_menu_picked(_ctx: *mut c_void, id: u32) {
    with_current_core(|st| match id {
        0 => {
            st.menu_open = false;
            st.menu_closed_at = Some(Instant::now());
            st.menu_refocus = false;
            unsafe { tomoyo_linux_add_timeout(90, dismissed_focus_check_timer, std::ptr::null_mut()); }
        }
        CMD_MENU_CLOSED_AFTER_PICK => {
            st.menu_open = false;
            st.menu_closed_at = Some(Instant::now());
            let refocus = st.menu_refocus;
            st.menu_refocus = false;
            if refocus && st.is_visible() && !st.ui.is_null() {
                unsafe { tomoyo_linux_schedule_idle(refocus_entry_idle, std::ptr::null_mut()); }
            }
        }
        _ => st.apply_menu_command(id),
    });
}

impl AppCore {
    pub(super) fn show_launcher_context_menu(&mut self, from_edit: bool) {
        self.menu_open = true;
        self.menu_closed_at = None;
        self.menu_refocus = false;
        let l = self.lang;
        let has_query = !self.current_query.is_empty();
        let has_sel = !self.ui.is_null() && unsafe { tomoyo_linux_entry_has_selection(self.ui) != 0 };
        let clear_disabled = !has_query && self.attached.is_none();

        let mut m = OwnedMenu::new();
        let edit_parent = if from_edit { -1 } else { m.submenu(l.pick("编辑与输入", "Edit Input"), -1) };
        m.push(l.pick("粘贴 (Ctrl+V)", "Paste (Ctrl+V)"), CMD_EDIT_PASTE, edit_parent, false, false);
        m.push(l.pick("剪切选中 (Ctrl+X)", "Cut (Ctrl+X)"), CMD_EDIT_CUT, edit_parent, false, !has_sel);
        m.push(l.pick("复制文本 (Ctrl+C)", "Copy (Ctrl+C)"), CMD_EDIT_COPY, edit_parent, false, !has_query && !has_sel);
        m.push(l.pick("全选搜索框 (Ctrl+A)", "Select All (Ctrl+A)"), CMD_EDIT_SELALL, edit_parent, false, !has_query);
        m.sep(edit_parent);
        m.push(l.pick("清空当前输入 (Esc)", "Clear Input (Esc)"), CMD_EDIT_CLEAR, edit_parent, false, clear_disabled);

        if from_edit {
            m.sep(-1);
        }
        let low = self.current_query.trim().to_lowercase();
        let mode_p = m.submenu(l.pick("切换搜索模式", "Search Mode"), -1);
        m.push(l.pick("全部混合搜索", "All Unified Search"), CMD_MODE_ALL, mode_p, !has_query && self.active_filter == SearchFilter::All, false);
        m.sep(mode_p);
        m.push(l.pick("文件检索 (f )", "Files (f )"), CMD_MODE_EV, mode_p, low.starts_with("f ") || low.starts_with("ev "), false);
        m.push(l.pick("剪贴板历史 (cb)", "Clipboard (cb)"), CMD_MODE_CB, mode_p, low == "cb" || low.starts_with("cb "), false);
        m.push(l.pick("窗口快速切换 (w)", "Windows (w)"), CMD_MODE_WIN, mode_p, low == "w" || low.starts_with("w "), false);
        m.push(l.pick("进程管理 (kill)", "Processes (kill)"), CMD_MODE_KILL, mode_p, low == "kill" || low.starts_with("kill "), false);
        m.push(l.pick("代码片段 (snip)", "Snippets (snip)"), CMD_MODE_SNIP, mode_p, low == "snip" || low.starts_with("snip "), false);
        m.push(l.pick("科学计算 (= )", "Calculator (= )"), CMD_MODE_CALC, mode_p, low.starts_with("= "), false);

        m.sep(-1);
        let is_l = self.theme_name.eq_ignore_ascii_case("light");
        let is_m = self.theme_name.eq_ignore_ascii_case("midnight");
        let theme_p = m.submenu(l.pick("外观主题", "Appearance Theme"), -1);
        m.push(l.pick("浅色 (Light)", "Light"), CMD_THEME_LIGHT, theme_p, is_l, false);
        m.push(l.pick("深色 (Dark)", "Dark"), CMD_THEME_DARK, theme_p, !is_l && !is_m, false);
        m.push(l.pick("午夜蓝 (Midnight)", "Midnight"), CMD_THEME_MIDNIGHT, theme_p, is_m, false);

        let lang_p = m.submenu(l.pick("界面语言", "Language"), -1);
        m.push("简体中文", CMD_LANG_ZH, lang_p, l == Lang::Zh, false);
        m.push("English", CMD_LANG_EN, lang_p, l == Lang::En, false);

        let view_p = m.submenu(l.pick("视图与窗口", "View & Layout"), -1);
        m.push(l.pick("右侧实时预览 (Ctrl+P)", "Live Preview (Ctrl+P)"), CMD_PREVIEW, view_p, self.preview_enabled, false);
        m.push(l.pick("分类筛选标签栏", "Category Filter Bar"), CMD_FILTER_BAR, view_p, self.show_filter_bar, false);
        m.sep(view_p);
        m.push(l.pick("重置窗口位置与尺寸", "Reset Geometry"), CMD_RESET_WIN, view_p, false, false);

        m.sep(-1);
        let tools_p = m.submenu(l.pick("诊断与维护", "Diagnostics & Tools"), -1);
        m.push(l.pick("内存与插件监控 (:stats)", "Memory & Plugins (:stats)"), CMD_STATS, tools_p, false, false);
        m.push(l.pick("热重载全部 (:reload)", "Hot-Reload All (:reload)"), CMD_RELOAD, tools_p, false, false);
        m.push(l.pick("立即休眠整理内存 (:gc)", "Hibernate & Trim (:gc)"), CMD_GC, tools_p, false, false);
        m.sep(tools_p);
        m.push(l.pick("打开 config.toml", "Open config.toml"), CMD_CONFIG, tools_p, false, false);

        m.push(l.pick("设置中心... (Ctrl+,)", "Settings... (Ctrl+,)"), CMD_SETTINGS, -1, false, false);
        m.sep(-1);
        m.push(l.pick("退出 Tomoyo", "Exit Tomoyo"), CMD_QUIT, -1, false, false);

        unsafe {
            tomoyo_linux_show_popup(
                self.ui,
                m.items.as_ptr(),
                m.items.len(),
                on_menu_picked,
                std::ptr::null_mut(),
            );
        }
    }

    fn apply_menu_command(&mut self, id: u32) {
        self.menu_refocus = true;
        match id {
            CMD_EDIT_PASTE => {
                if clipboard_has_rich_attachment() {
                    if let Some(files) = read_clipboard_files() {
                        self.attached = Some(AttachedContext::Files { paths: files });
                        self.dispatch_query();
                    } else if let Some((path, width, height, _)) = read_clipboard_image_png() {
                        self.attached = Some(AttachedContext::Image { path, width, height });
                        self.dispatch_query();
                    }
                } else if !self.ui.is_null() {
                    unsafe { tomoyo_linux_entry_edit_cmd(self.ui, 3); }
                }
            }
            CMD_EDIT_CUT if !self.ui.is_null() => unsafe { tomoyo_linux_entry_edit_cmd(self.ui, 1); },
            CMD_EDIT_COPY => {
                let has_sel = !self.ui.is_null() && unsafe { tomoyo_linux_entry_has_selection(self.ui) != 0 };
                if has_sel {
                    unsafe { tomoyo_linux_entry_edit_cmd(self.ui, 2); }
                } else if !self.current_query.is_empty() && set_clipboard_text(&self.current_query) {
                    self.show_toast(self.lang.pick("✓ 已复制搜索框文本", "✓ Copied query text"));
                }
            }
            CMD_EDIT_SELALL if !self.ui.is_null() => unsafe { tomoyo_linux_entry_edit_cmd(self.ui, 4); },
            CMD_EDIT_CLEAR => {
                self.attached = None;
                self.set_query_text("");
                self.invalidate();
            }
            CMD_MODE_ALL => { self.active_filter = SearchFilter::All; self.attached = None; self.set_query_text(""); }
            CMD_MODE_EV => { self.active_filter = SearchFilter::All; self.set_query_text("f "); }
            CMD_MODE_CB => { self.active_filter = SearchFilter::All; self.set_query_text("cb "); }
            CMD_MODE_WIN => { self.active_filter = SearchFilter::All; self.set_query_text("w "); }
            CMD_MODE_KILL => { self.active_filter = SearchFilter::All; self.set_query_text("kill "); }
            CMD_MODE_SNIP => { self.active_filter = SearchFilter::All; self.set_query_text("snip "); }
            CMD_MODE_CALC => { self.active_filter = SearchFilter::All; self.set_query_text("= "); }
            CMD_THEME_LIGHT => self.apply_theme("light", true),
            CMD_THEME_DARK => self.apply_theme("dark", true),
            CMD_THEME_MIDNIGHT => self.apply_theme("midnight", true),
            CMD_LANG_ZH => self.apply_language("zh", true),
            CMD_LANG_EN => self.apply_language("en", true),
            CMD_PREVIEW => {
                self.preview_enabled = !self.preview_enabled;
                if let Ok(mut c) = self.config.write() { c.auto_preview = self.preview_enabled; let _ = c.save(); }
                self.resize_to_fit();
                self.invalidate();
            }
            CMD_FILTER_BAR => {
                self.show_filter_bar = !self.show_filter_bar;
                if let Ok(mut c) = self.config.write() { c.show_filter_bar = self.show_filter_bar; let _ = c.save(); }
                self.resize_to_fit();
                self.invalidate();
            }
            CMD_RESET_WIN => self.reset_window_geometry(),
            CMD_STATS => self.set_query_text(":stats"),
            CMD_RELOAD => { self.router.reload_all(); self.dispatch_query(); self.show_toast("已热重载全部插件与索引"); }
            CMD_GC => { let _ = self.run_hibernate(); self.show_toast("已完成内存休眠整理"); }
            CMD_CONFIG => {
                self.menu_refocus = false;
                let _ = shell_open(&TomoyoConfig::config_path().to_string_lossy(), None, None);
                self.hide_launcher();
            }
            CMD_SETTINGS => {
                self.menu_refocus = false;
                self.open_settings_center();
            }
            CMD_QUIT => {
                self.menu_refocus = false;
                self.shutdown_clean();
                unsafe { tomoyo_linux_ui_quit(); }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_menu_preserves_submenu_indices_and_flags() {
        let mut m = OwnedMenu::new();
        let paste = m.push("Paste", CMD_EDIT_PASTE, -1, false, false);
        let cut = m.push("Cut", CMD_EDIT_CUT, -1, false, true);
        let sub = m.submenu("Theme", -1);
        let dark = m.push("Dark", CMD_THEME_DARK, sub, true, false);
        m.sep(sub);
        assert_eq!((paste, cut, sub, dark), (0, 1, 2, 3));
        assert_eq!(m.items.len(), 5);
        assert_eq!(m.items[1].disabled, 1);
        assert_eq!(m.items[2].cmd, 0);
        assert_eq!(m.items[3].parent_idx, sub);
        assert_eq!(m.items[3].checked, 1);
        assert_eq!(m.items[4].parent_idx, sub);
    }
}
