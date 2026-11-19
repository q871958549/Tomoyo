//! Native Win32 right-click context menus for the Modern White Logo and search result items.

use super::AppShell;
use crate::actions::set_clipboard_text;
use tomoyo_core::Lang;
use windows::core::PCWSTR;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, DestroyWindow, GetCursorPos, SetForegroundWindow,
    TrackPopupMenu, HMENU, MF_CHECKED, MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TPM_TOPALIGN,
};

impl AppShell {
    pub(super) fn show_logo_context_menu(&mut self) {
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
        const CMD_UPDATE: u32 = 114;

        let l = self.lang;
        self.inter.suppress_next_deactivate = true;
        let chosen = unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                self.inter.suppress_next_deactivate = false;
                return;
            };
            append_menu_item(menu, CMD_SETTINGS, l.pick("设置中心...\tCtrl+,", "Settings...\tCtrl+,"), false);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

            let is_light = self.theme_name.eq_ignore_ascii_case("light");
            let is_midnight = self.theme_name.eq_ignore_ascii_case("midnight");
            let is_dark = !is_light && !is_midnight;

            append_menu_item(menu, CMD_THEME_LIGHT, l.pick("主题：现代白瓷光", "Theme: Modern White"), is_light);
            append_menu_item(menu, CMD_THEME_DARK, l.pick("主题：深空黑曜石", "Theme: Obsidian Dark"), is_dark);
            append_menu_item(menu, CMD_THEME_MIDNIGHT, l.pick("主题：深邃午夜蓝", "Theme: Deep Midnight"), is_midnight);

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_menu_item(menu, CMD_LANG_ZH, "语言：简体中文", l == Lang::Zh);
            append_menu_item(menu, CMD_LANG_EN, "Language: English", l == Lang::En);

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_menu_item(menu, CMD_PREVIEW, l.pick("右侧实时预览\tCtrl+P", "Live Preview Pane\tCtrl+P"), self.preview_enabled);
            append_menu_item(menu, CMD_RESET_WIN, l.pick("重置窗口位置与尺寸", "Reset Window Position & Size"), false);

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_menu_item(menu, CMD_UPDATE, l.pick("检查更新 (:update)", "Check for Updates (:update)"), false);
            append_menu_item(menu, CMD_STATS, l.pick("内存与插件监控 (:stats)", "Memory & Plugin Stats (:stats)"), false);
            append_menu_item(menu, CMD_RELOAD, l.pick("热重载全部 (:reload)", "Hot-Reload All (:reload)"), false);
            append_menu_item(menu, CMD_GC, l.pick("立即休眠整理内存 (:gc)", "Hibernate & Trim RAM (:gc)"), false);
            append_menu_item(menu, CMD_CONFIG, l.pick("打开 config.toml", "Open config.toml"), false);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_menu_item(menu, CMD_QUIT, l.pick("退出 Tomoyo", "Exit Tomoyo"), false);

            track_and_destroy(self.hwnd, menu)
        };
        self.inter.suppress_next_deactivate = false;

        match chosen {
            CMD_SETTINGS => self.open_settings_center(),
            CMD_THEME_LIGHT => self.apply_theme("light", true),
            CMD_THEME_DARK => self.apply_theme("dark", true),
            CMD_THEME_MIDNIGHT => self.apply_theme("midnight", true),
            CMD_LANG_ZH => self.apply_language("zh", true),
            CMD_LANG_EN => self.apply_language("en", true),
            CMD_PREVIEW => {
                self.preview_enabled = !self.preview_enabled;
                self.cached_preview_id = None;
                self.refresh_selected_preview();
                self.resize_to_fit_results();
                self.invalidate();
            }
            CMD_RESET_WIN => self.reset_window_geometry(),
            CMD_UPDATE => {
                self.set_query_text(":update");
                self.trigger_update_check_async();
            }
            CMD_STATS => self.set_query_text(":stats"),
            CMD_RELOAD => {
                self.reload_config_and_all();
                self.show_toast(self.lang.pick("已热重载配置、应用与插件", "Hot-reloaded all"));
            }
            CMD_GC => {
                let c = self.run_hibernate();
                self.show_toast(&format!("内存整理完成 (#{c})"));
            }
            CMD_CONFIG => {
                self.open_config_file_in_editor();
                self.hide_launcher();
            }
            CMD_QUIT => {
                self.plugin_host.shutdown_all();
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            _ => {}
        }
    }

    pub(super) fn show_result_context_menu(&mut self, idx: usize) {
        let Some(item) = self.results.get(idx).cloned() else {
            return;
        };
        const CMD_COPY_TITLE: u32 = 9001;
        const CMD_TOGGLE_PREVIEW: u32 = 9002;
        let l = self.lang;

        self.inter.suppress_next_deactivate = true;
        let chosen = unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                self.inter.suppress_next_deactivate = false;
                return;
            };
            for (ai, act) in item.actions.iter().enumerate() {
                let label = match &act.shortcut {
                    Some(sc) => format!("{}\t{sc}", act.label),
                    None => act.label.clone(),
                };
                append_menu_item(menu, (ai + 1) as u32, &label, false);
            }
            if !item.actions.is_empty() {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            append_menu_item(menu, CMD_COPY_TITLE, l.pick("复制文本内容", "Copy Text"), false);
            append_menu_item(
                menu,
                CMD_TOGGLE_PREVIEW,
                l.pick("右侧实时预览\tCtrl+P", "Live Preview Pane\tCtrl+P"),
                self.preview_enabled,
            );
            track_and_destroy(self.hwnd, menu)
        };
        self.inter.suppress_next_deactivate = false;

        if chosen == 0 {
            return;
        }
        if chosen == CMD_COPY_TITLE {
            let text = item.copy_text.as_deref().unwrap_or(&item.title);
            if set_clipboard_text(text) {
                self.show_toast(self.lang.pick("已复制到剪贴板", "Copied to clipboard"));
            }
            return;
        }
        if chosen == CMD_TOGGLE_PREVIEW {
            self.preview_enabled = !self.preview_enabled;
            self.cached_preview_id = None;
            self.refresh_selected_preview();
            self.resize_to_fit_results();
            self.invalidate();
            return;
        }
        let action_idx = (chosen - 1) as usize;
        if action_idx < item.actions.len() {
            self.selected = idx;
            self.execute_selected(action_idx);
        }
    }
}

unsafe fn track_and_destroy(hwnd: windows::Win32::Foundation::HWND, menu: HMENU) -> u32 {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            pt.x,
            pt.y,
            Some(0),
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        cmd.0 as u32
    }
}

unsafe fn append_menu_item(menu: HMENU, id: u32, text: &str, checked: bool) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let flags = MF_STRING | if checked { MF_CHECKED } else { Default::default() };
    let _ = unsafe { AppendMenuW(menu, flags, id as usize, PCWSTR(wide.as_ptr())) };
}
