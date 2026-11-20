//! Theme-matched right-click context menus for the Brand Logo and search result items.

use super::AppShell;
use crate::actions::set_clipboard_text;
use crate::popup_menu::{show_themed_popup_menu, PopupEntry};
use tomoyo_core::Lang;
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, SetForegroundWindow};

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
        let is_light = self.theme_name.eq_ignore_ascii_case("light");
        let is_midnight = self.theme_name.eq_ignore_ascii_case("midnight");
        let is_dark = !is_light && !is_midnight;

        let entries = vec![
            PopupEntry::item(
                CMD_SETTINGS,
                l.pick("设置中心...", "Settings..."),
                Some("Ctrl+,"),
                false,
            ),
            PopupEntry::Separator,
            PopupEntry::item(
                CMD_THEME_LIGHT,
                l.pick("主题：浅色 (Light)", "Theme: Light"),
                None,
                is_light,
            ),
            PopupEntry::item(
                CMD_THEME_DARK,
                l.pick("主题：深色 (Dark)", "Theme: Dark"),
                None,
                is_dark,
            ),
            PopupEntry::item(
                CMD_THEME_MIDNIGHT,
                l.pick("主题：午夜蓝 (Midnight)", "Theme: Midnight"),
                None,
                is_midnight,
            ),
            PopupEntry::Separator,
            PopupEntry::item(CMD_LANG_ZH, "语言：简体中文", None, l == Lang::Zh),
            PopupEntry::item(CMD_LANG_EN, "Language: English", None, l == Lang::En),
            PopupEntry::Separator,
            PopupEntry::item(
                CMD_PREVIEW,
                l.pick("右侧实时预览", "Live Preview Pane"),
                Some("Ctrl+P"),
                self.preview_enabled,
            ),
            PopupEntry::item(
                CMD_RESET_WIN,
                l.pick("重置窗口位置与尺寸", "Reset Window Position & Size"),
                None,
                false,
            ),
            PopupEntry::Separator,
            PopupEntry::item(
                CMD_UPDATE,
                l.pick("检查更新 (:update)", "Check for Updates"),
                None,
                false,
            ),
            PopupEntry::item(
                CMD_STATS,
                l.pick("内存与插件监控 (:stats)", "Memory & Plugin Stats"),
                None,
                false,
            ),
            PopupEntry::item(
                CMD_RELOAD,
                l.pick("热重载全部 (:reload)", "Hot-Reload All"),
                None,
                false,
            ),
            PopupEntry::item(
                CMD_GC,
                l.pick("立即休眠整理内存 (:gc)", "Hibernate & Trim RAM"),
                None,
                false,
            ),
            PopupEntry::item(
                CMD_CONFIG,
                l.pick("打开 config.toml", "Open config.toml"),
                None,
                false,
            ),
            PopupEntry::Separator,
            PopupEntry::item(CMD_QUIT, l.pick("退出 Tomoyo", "Exit Tomoyo"), None, false),
        ];

        self.inter.suppress_next_deactivate = true;
        let chosen = show_themed_popup_menu(
            self.theme,
            self.dpi_scale,
            self.font_regular,
            self.font_medium,
            &entries,
            false,
        );
        self.inter.suppress_next_deactivate = false;

        let Some(cmd) = chosen else {
            return;
        };
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
        match cmd {
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
                self.shutdown_clean();
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

        let mut entries = Vec::with_capacity(item.actions.len() + 3);
        for (ai, act) in item.actions.iter().enumerate() {
            entries.push(PopupEntry::item(
                (ai + 1) as u32,
                act.label.clone(),
                act.shortcut.as_deref(),
                false,
            ));
        }
        if !item.actions.is_empty() {
            entries.push(PopupEntry::Separator);
        }
        entries.push(PopupEntry::item(
            CMD_COPY_TITLE,
            l.pick("复制文本内容", "Copy Text"),
            Some("Ctrl+C"),
            false,
        ));
        entries.push(PopupEntry::item(
            CMD_TOGGLE_PREVIEW,
            l.pick("右侧实时预览", "Live Preview Pane"),
            Some("Ctrl+P"),
            self.preview_enabled,
        ));

        self.inter.suppress_next_deactivate = true;
        let chosen = show_themed_popup_menu(
            self.theme,
            self.dpi_scale,
            self.font_regular,
            self.font_medium,
            &entries,
            false,
        );
        self.inter.suppress_next_deactivate = false;

        let Some(cmd) = chosen else {
            return;
        };
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
        if cmd == CMD_COPY_TITLE {
            let text = item.copy_text.as_deref().unwrap_or(&item.title);
            if set_clipboard_text(text) {
                self.show_toast(self.lang.pick("已复制到剪贴板", "Copied to clipboard"));
            }
            return;
        }
        if cmd == CMD_TOGGLE_PREVIEW {
            self.preview_enabled = !self.preview_enabled;
            self.cached_preview_id = None;
            self.refresh_selected_preview();
            self.resize_to_fit_results();
            self.invalidate();
            return;
        }
        let action_idx = (cmd - 1) as usize;
        if action_idx < item.actions.len() {
            self.selected = idx;
            self.execute_selected(action_idx);
        }
    }
}
