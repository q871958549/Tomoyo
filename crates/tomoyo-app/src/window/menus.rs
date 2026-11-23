//! Theme-matched cascading (Level-1 / Level-2) context menus for search input, brand logo, and result rows.
//!
//! Search input & header menus stay open after tryable selections (themes, languages, view toggles,
//! search mode prefixes, edit actions, diagnostics) and dismiss only on outside click, `Esc`, or exit actions.

use super::{AppShell, EM_GETSEL, EM_SETSEL};
use crate::actions::{clipboard_has_rich_attachment, set_clipboard_text};
use crate::popup_menu::{run_interactive_popup_menu, show_themed_popup_menu, MenuStepOutcome, PopupEntry};
use tomoyo_core::Lang;
use tomoyo_protocol::SearchFilter;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetForegroundWindow, GetWindowThreadProcessId, SendMessageW,
    SetForegroundWindow, WM_COPY, WM_CUT, WM_PASTE,
};

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
const CMD_FILTER_BAR: u32 = 115;
const CMD_RESET_WIN: u32 = 108;
const CMD_STATS: u32 = 109;
const CMD_RELOAD: u32 = 110;
const CMD_GC: u32 = 111;
const CMD_CONFIG: u32 = 112;
const CMD_QUIT: u32 = 113;
const CMD_UPDATE: u32 = 114;

impl AppShell {
    pub(super) fn show_logo_context_menu(&mut self) {
        self.show_launcher_context_menu(false);
    }

    pub(super) fn show_launcher_context_menu(&mut self, from_edit: bool) {
        let initial = self.build_launcher_menu_entries(from_edit);
        let pal = self.theme;
        let dpi_scale = self.dpi_scale;
        let font_regular = self.font_regular;
        let font_medium = self.font_medium;
        let mut close_action: Option<u32> = None;

        self.inter.in_modal_overlay = true;
        let app_ptr = self as *mut AppShell;
        run_interactive_popup_menu(
            pal,
            dpi_scale,
            font_regular,
            font_medium,
            initial,
            false,
            |cmd| {
                let shell = unsafe { &mut *app_ptr };
                if matches!(cmd, CMD_SETTINGS | CMD_CONFIG | CMD_QUIT) {
                    close_action = Some(cmd);
                    return MenuStepOutcome::Close;
                }
                shell.execute_tryable_menu_command(cmd);
                unsafe {
                    let _ = UpdateWindow(shell.hwnd);
                }
                MenuStepOutcome::StayOpen {
                    entries: shell.build_launcher_menu_entries(from_edit),
                    pal: shell.theme,
                }
            },
        );
        self.inter.in_modal_overlay = false;

        if let Some(cmd) = close_action {
            self.execute_terminal_menu_command(cmd);
            return;
        }
        unsafe {
            let fg = GetForegroundWindow();
            let mut fg_pid = 0u32;
            if !fg.is_invalid() {
                GetWindowThreadProcessId(fg, Some(&mut fg_pid));
            }
            if fg.is_invalid() || fg == self.hwnd || fg_pid == std::process::id() {
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.edit_hwnd));
            } else {
                self.hide_launcher();
            }
        }
    }

    fn build_launcher_menu_entries(&self, _from_edit: bool) -> Vec<PopupEntry> {
        let l = self.lang;
        let low = self.current_query.trim().to_lowercase();
        let has_query = !self.current_query.is_empty();
        let mut sel_start = 0u32;
        let mut sel_end = 0u32;
        unsafe {
            SendMessageW(
                self.edit_hwnd,
                EM_GETSEL,
                Some(WPARAM(&mut sel_start as *mut _ as usize)),
                Some(LPARAM(&mut sel_end as *mut _ as isize)),
            );
        }
        let has_sel = sel_end > sel_start;

        let edit_sub = PopupEntry::submenu(
            l.pick("编辑与输入", "Edit Input"),
            vec![
                PopupEntry::item(CMD_EDIT_PASTE, l.pick("粘贴", "Paste"), Some("Ctrl+V"), false),
                PopupEntry::item_disabled(CMD_EDIT_CUT, l.pick("剪切选中", "Cut"), Some("Ctrl+X"), !has_sel),
                PopupEntry::item_disabled(CMD_EDIT_COPY, l.pick("复制文本", "Copy"), Some("Ctrl+C"), !has_query && !has_sel),
                PopupEntry::item_disabled(CMD_EDIT_SELALL, l.pick("全选搜索框", "Select All"), Some("Ctrl+A"), !has_query),
                PopupEntry::Separator,
                PopupEntry::item_disabled(CMD_EDIT_CLEAR, l.pick("清空当前输入", "Clear Input"), Some("Esc"), !has_query && self.attached.is_none()),
            ],
        );

        let mode_sub = PopupEntry::submenu(
            l.pick("切换搜索模式", "Search Mode"),
            vec![
                PopupEntry::item(CMD_MODE_ALL, l.pick("全部混合搜索", "All Unified Search"), None, !has_query && self.active_filter == SearchFilter::All),
                PopupEntry::Separator,
                PopupEntry::item(CMD_MODE_EV, l.pick("Everything 文件检索", "Everything Files"), Some("f "), low.starts_with("f ") || low.starts_with("ev ")),
                PopupEntry::item(CMD_MODE_CB, l.pick("剪贴板历史", "Clipboard History"), Some("cb"), low == "cb" || low.starts_with("cb ")),
                PopupEntry::item(CMD_MODE_WIN, l.pick("窗口快速切换", "Window Switcher"), Some("w"), low == "w" || low.starts_with("w ")),
                PopupEntry::item(CMD_MODE_KILL, l.pick("进程管理与结束", "Process Manager"), Some("kill"), low == "kill" || low.starts_with("kill ")),
                PopupEntry::item(CMD_MODE_SNIP, l.pick("代码片段模板", "Text Snippets"), Some("snip"), low == "snip" || low.starts_with("snip ")),
                PopupEntry::item(CMD_MODE_CALC, l.pick("科学计算与换算", "Calculator"), Some("= "), low.starts_with("= ")),
            ],
        );

        let is_light = self.theme_name.eq_ignore_ascii_case("light");
        let is_midnight = self.theme_name.eq_ignore_ascii_case("midnight");
        let is_dark = !is_light && !is_midnight;
        let theme_sub = PopupEntry::submenu(
            l.pick("外观主题", "Appearance Theme"),
            vec![
                PopupEntry::item(CMD_THEME_LIGHT, l.pick("浅色 (Light)", "Light"), None, is_light),
                PopupEntry::item(CMD_THEME_DARK, l.pick("深色 (Dark)", "Dark"), None, is_dark),
                PopupEntry::item(CMD_THEME_MIDNIGHT, l.pick("午夜蓝 (Midnight)", "Midnight"), None, is_midnight),
            ],
        );

        let lang_sub = PopupEntry::submenu(
            l.pick("界面语言", "Display Language"),
            vec![
                PopupEntry::item(CMD_LANG_ZH, "简体中文", None, l == Lang::Zh),
                PopupEntry::item(CMD_LANG_EN, "English", None, l == Lang::En),
            ],
        );

        let view_sub = PopupEntry::submenu(
            l.pick("视图与窗口", "View & Layout"),
            vec![
                PopupEntry::item(CMD_PREVIEW, l.pick("右侧实时预览", "Live Preview Pane"), Some("Ctrl+P"), self.preview_enabled),
                PopupEntry::item(CMD_FILTER_BAR, l.pick("分类筛选标签栏", "Category Filter Bar"), None, self.show_filter_bar),
                PopupEntry::Separator,
                PopupEntry::item(CMD_RESET_WIN, l.pick("重置窗口位置与尺寸", "Reset Window Geometry"), None, false),
            ],
        );

        let tools_sub = PopupEntry::submenu(
            l.pick("诊断与维护", "Diagnostics & Tools"),
            vec![
                PopupEntry::item(CMD_UPDATE, l.pick("检查更新", "Check Updates"), Some(":update"), low == ":update"),
                PopupEntry::item(CMD_STATS, l.pick("内存与插件监控", "Memory & Plugins"), Some(":stats"), low == ":stats"),
                PopupEntry::item(CMD_RELOAD, l.pick("热重载全部配置", "Hot-Reload All"), Some(":reload"), false),
                PopupEntry::item(CMD_GC, l.pick("立即休眠整理内存", "Hibernate & Trim RAM"), Some(":gc"), false),
                PopupEntry::Separator,
                PopupEntry::item(CMD_CONFIG, l.pick("打开 config.toml", "Open config.toml"), None, false),
            ],
        );

        vec![
            edit_sub,
            mode_sub,
            PopupEntry::Separator,
            theme_sub,
            lang_sub,
            view_sub,
            PopupEntry::Separator,
            tools_sub,
            PopupEntry::item(CMD_SETTINGS, l.pick("设置中心...", "Settings..."), Some("Ctrl+,"), false),
            PopupEntry::Separator,
            PopupEntry::danger_item(CMD_QUIT, l.pick("退出 Tomoyo", "Exit Tomoyo"), None),
        ]
    }

    fn execute_tryable_menu_command(&mut self, cmd: u32) {
        match cmd {
            CMD_EDIT_PASTE => {
                if clipboard_has_rich_attachment() {
                    self.on_paste_intercept();
                } else {
                    unsafe { let _ = SendMessageW(self.edit_hwnd, WM_PASTE, None, None); }
                }
            }
            CMD_EDIT_CUT => unsafe { let _ = SendMessageW(self.edit_hwnd, WM_CUT, None, None); },
            CMD_EDIT_COPY => {
                let (mut s, mut e) = (0u32, 0u32);
                unsafe {
                    SendMessageW(
                        self.edit_hwnd,
                        EM_GETSEL,
                        Some(WPARAM(&mut s as *mut _ as usize)),
                        Some(LPARAM(&mut e as *mut _ as isize)),
                    );
                }
                if e > s {
                    unsafe { let _ = SendMessageW(self.edit_hwnd, WM_COPY, None, None); }
                } else if !self.current_query.is_empty() && set_clipboard_text(&self.current_query) {
                    self.show_toast(self.lang.pick("✓ 已复制搜索框文本", "✓ Copied query text"));
                }
            }
            CMD_EDIT_SELALL => unsafe {
                let _ = SendMessageW(self.edit_hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            },
            CMD_EDIT_CLEAR => {
                self.attached = None;
                self.set_query_text("");
                self.invalidate();
            }
            CMD_MODE_ALL => {
                self.active_filter = SearchFilter::All;
                self.attached = None;
                self.set_query_text("");
            }
            CMD_MODE_EV => {
                self.active_filter = SearchFilter::All;
                self.set_query_text("f ");
            }
            CMD_MODE_CB => {
                self.active_filter = SearchFilter::All;
                self.set_query_text("cb ");
            }
            CMD_MODE_WIN => {
                self.active_filter = SearchFilter::All;
                self.set_query_text("w ");
            }
            CMD_MODE_KILL => {
                self.active_filter = SearchFilter::All;
                self.set_query_text("kill ");
            }
            CMD_MODE_SNIP => {
                self.active_filter = SearchFilter::All;
                self.set_query_text("snip ");
            }
            CMD_MODE_CALC => {
                self.active_filter = SearchFilter::All;
                self.set_query_text("= ");
            }
            CMD_THEME_LIGHT => self.apply_theme("light", true),
            CMD_THEME_DARK => self.apply_theme("dark", true),
            CMD_THEME_MIDNIGHT => self.apply_theme("midnight", true),
            CMD_LANG_ZH => self.apply_language("zh", true),
            CMD_LANG_EN => self.apply_language("en", true),
            CMD_PREVIEW => {
                self.preview_enabled = !self.preview_enabled;
                if let Ok(mut c) = self.config.write() {
                    c.auto_preview = self.preview_enabled;
                    let _ = c.save();
                }
                self.cached_preview_id = None;
                self.refresh_selected_preview();
                self.resize_to_fit_results();
                self.invalidate();
            }
            CMD_FILTER_BAR => {
                self.show_filter_bar = !self.show_filter_bar;
                if let Ok(mut c) = self.config.write() {
                    c.show_filter_bar = self.show_filter_bar;
                    let _ = c.save();
                }
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
                self.show_toast(self.lang.pick("✓ 已热重载配置、应用与插件", "✓ Hot-reloaded all"));
            }
            CMD_GC => {
                let c = self.run_hibernate();
                self.show_toast(&format!("✓ 内存整理完成 (#{c})"));
            }
            _ => {}
        }
    }

    fn execute_terminal_menu_command(&mut self, cmd: u32) {
        unsafe { let _ = SetForegroundWindow(self.hwnd); }
        match cmd {
            CMD_SETTINGS => self.open_settings_center(),
            CMD_CONFIG => {
                self.open_config_file_in_editor();
                self.hide_launcher();
            }
            CMD_QUIT => {
                self.shutdown_clean();
                unsafe { let _ = DestroyWindow(self.hwnd); }
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
            entries.push(PopupEntry::item((ai + 1) as u32, act.label.clone(), act.shortcut.as_deref(), false));
        }
        if !item.actions.is_empty() {
            entries.push(PopupEntry::Separator);
        }
        entries.push(PopupEntry::item(CMD_COPY_TITLE, l.pick("复制文本内容", "Copy Text"), Some("Ctrl+C"), false));
        entries.push(PopupEntry::item(CMD_TOGGLE_PREVIEW, l.pick("右侧实时预览", "Live Preview Pane"), Some("Ctrl+P"), self.preview_enabled));

        self.inter.in_modal_overlay = true;
        let chosen = show_themed_popup_menu(self.theme, self.dpi_scale, self.font_regular, self.font_medium, &entries, false);
        self.inter.in_modal_overlay = false;

        let Some(cmd) = chosen else { return };
        unsafe { let _ = SetForegroundWindow(self.hwnd); }
        if cmd == CMD_COPY_TITLE {
            let text = item.copy_text.as_deref().unwrap_or(&item.title);
            if set_clipboard_text(text) {
                self.show_toast(self.lang.pick("已复制到剪贴板", "Copied to clipboard"));
            }
            return;
        }
        if cmd == CMD_TOGGLE_PREVIEW {
            self.execute_tryable_menu_command(CMD_PREVIEW);
            return;
        }
        let action_idx = (cmd - 1) as usize;
        if action_idx < item.actions.len() {
            self.selected = idx;
            self.execute_selected(action_idx);
        }
    }
}
