//! Action execution, internal commands, Super Match paste, async preview & async updater.

use super::input::register_configured_hotkey;
use super::layout::apply_cue_banner;
use super::{AppShell, EDIT_CTRL_ID, HOTKEY_ID, WM_APP_UPDATE_READY};
use crate::actions::{
    copy_image_as_base64_uri, execute_action, kill_process_by_pid,
    paste_into_previous_window_async, read_clipboard_files, read_clipboard_image_png,
};
use crate::renderer::MIN_WINDOW_WIDTH_DIP;
use crate::tray::{
    MENU_ID_CONFIG, MENU_ID_GC, MENU_ID_QUIT, MENU_ID_RELOAD, MENU_ID_SETTINGS, MENU_ID_SHOW,
    MENU_ID_STATS, MENU_ID_UPDATE,
};
use std::ffi::c_void;
use std::sync::Arc;
use tomoyo_core::{
    is_autostart_enabled, set_autostart_enabled, Lang, TomoyoConfig, UpdateStatus,
};
use tomoyo_protocol::{ActionKind, AttachedContext, InternalCommandKind};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, IsWindowVisible, PostMessageW, EN_CHANGE,
};

pub(super) fn dispatch_wm_command(hwnd: HWND, app: &mut AppShell, wparam: WPARAM) -> LRESULT {
    let id = (wparam.0 & 0xFFFF) as u32;
    let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
    if id as usize == EDIT_CTRL_ID && code == EN_CHANGE {
        app.on_edit_changed();
        return LRESULT(0);
    }
    dispatch_menu_id(hwnd, app, id);
    LRESULT(0)
}

pub(super) fn dispatch_menu_id(hwnd: HWND, app: &mut AppShell, id: u32) {
    match id {
        MENU_ID_SHOW => app.show_launcher(),
        MENU_ID_SETTINGS => app.open_settings_center(),
        MENU_ID_UPDATE => {
            app.inter.suppress_next_deactivate = true;
            app.show_launcher();
            app.set_query_text(":update");
            app.trigger_update_check_async();
        }
        MENU_ID_STATS => {
            app.inter.suppress_next_deactivate = true;
            app.show_launcher();
            app.set_query_text(":stats");
        }
        MENU_ID_CONFIG => app.open_config_file_in_editor(),
        MENU_ID_RELOAD => {
            app.reload_config_and_all();
            app.show_launcher();
            app.show_toast(app.lang.pick("已热重载配置、应用与插件", "Hot-reloaded all"));
        }
        MENU_ID_GC => {
            let _ = app.run_hibernate();
        }
        MENU_ID_QUIT => {
            app.shutdown_clean();
            unsafe { let _ = DestroyWindow(hwnd); }
        }
        _ => {}
    }
}

impl AppShell {
    pub(super) fn refresh_selected_preview(&mut self) {
        if !self.preview_enabled {
            self.preview_loader.cancel_all();
            self.cached_preview_id = None;
            self.cached_preview = None;
            return;
        }
        let Some(item) = self.results.get(self.selected) else {
            self.preview_loader.cancel_all();
            self.cached_preview_id = None;
            self.cached_preview = None;
            return;
        };
        if self.cached_preview_id.as_deref() == Some(&item.id) {
            return;
        }
        let Some(spec) = item.preview.as_ref() else {
            self.preview_loader.cancel_all();
            self.cached_preview_id = Some(item.id.clone());
            self.cached_preview = None;
            return;
        };
        if let Some(instant) = self.preview_loader.resolve_or_enqueue(&item.id, spec, self.lang) {
            self.cached_preview_id = Some(item.id.clone());
            self.cached_preview = Some(instant);
        } else {
            // Disk / image preview resolves asynchronously off the UI thread.
            self.cached_preview_id = None;
            self.cached_preview = None;
        }
    }

    pub(super) fn trigger_update_check_async(&mut self) {
        let repo = self
            .config
            .read()
            .map(|c| c.update.repo.clone())
            .unwrap_or_else(|_| "q871958549/Tomoyo".into());
        self.show_toast(self.lang.pick(
            "正在连接 GitHub Releases 检查最新版本...",
            "Checking GitHub Releases for updates...",
        ));
        let updater = Arc::clone(&self.updater);
        let cfg = Arc::clone(&self.config);
        let hwnd_raw = self.hwnd.0 as usize;
        let _ = std::thread::Builder::new()
            .name("tomoyo-manual-update".into())
            .spawn(move || {
                let res = updater.check_now_sync(&repo);
                if matches!(res, UpdateStatus::UpToDate { .. } | UpdateStatus::Available { .. }) {
                    if let Ok(mut w) = cfg.write() {
                        w.update.last_checked_unix = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let _ = w.save();
                    }
                }
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut c_void)),
                        WM_APP_UPDATE_READY,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            });
    }

    pub(super) fn on_update_ready(&mut self) {
        let msg = match self.updater.snapshot() {
            UpdateStatus::Available { release, .. } => match self.lang {
                Lang::Zh => format!("发现新版本 v{}！输入 :update 查看更新日志并下载", release.version),
                Lang::En => format!("New version v{} available! Type :update to download", release.version),
            },
            UpdateStatus::UpToDate { current, note, .. } => {
                if note.is_empty() {
                    match self.lang {
                        Lang::Zh => format!("当前已是最新版本 v{current}"),
                        Lang::En => format!("Already on latest version v{current}"),
                    }
                } else {
                    note
                }
            }
            UpdateStatus::Error { message, .. } => match self.lang {
                Lang::Zh => format!("检查更新失败：{message}"),
                Lang::En => format!("Update check failed: {message}"),
            },
            _ => return,
        };
        self.show_toast(&msg);
        if self.current_query.trim().starts_with(':') {
            self.dispatch_query();
        }
    }

    pub(super) fn on_paste_intercept(&mut self) {
        if let Some(files) = read_clipboard_files() {
            let n = files.len();
            self.attached = Some(AttachedContext::Files { paths: files });
            self.show_toast(&match self.lang {
                Lang::Zh => format!("已附加 {n} 个文件胶囊（Super Match）· 按 Backspace 清除"),
                Lang::En => format!("Attached {n} file(s) capsule · Press Backspace to clear"),
            });
            self.dispatch_query();
            self.invalidate();
            return;
        }
        if let Some((path, w, h, digest)) = read_clipboard_image_png() {
            self.clipboard.push_image(path.clone(), w, h, digest);
            self.attached = Some(AttachedContext::Image {
                path,
                width: w,
                height: h,
            });
            self.show_toast(&match self.lang {
                Lang::Zh => format!("已附加 {w}×{h} 图片胶囊（Super Match）· 按 Backspace 清除"),
                Lang::En => format!("Attached {w}×{h} image capsule · Press Backspace to clear"),
            });
            self.dispatch_query();
            self.invalidate();
        }
    }

    pub(super) fn execute_selected(&mut self, action_idx: usize) {
        let Some(item) = self.results.get(self.selected).cloned() else {
            return;
        };
        let Some(action) = item.actions.get(action_idx).or_else(|| item.actions.first()) else {
            return;
        };

        self.router.record_execution(&item.id);
        if matches!(action.kind, ActionKind::CopyImage { .. }) {
            self.inter.suppress_next_clip_img = true;
        }
        let outcome = execute_action(&action.kind, &item.id, &self.plugin_host, self.theme.is_dark);

        if let Some(internal) = outcome.internal {
            self.handle_internal_command(internal);
            return;
        }
        if let Some(fill) = outcome.fill_query {
            self.action_menu_open = false;
            self.set_query_text(&fill);
            return;
        }
        if let Some(toast) = outcome.toast {
            self.show_toast(&toast);
        }
        if outcome.hide_window {
            let prev = self.inter.prev_foreground_hwnd;
            self.hide_launcher();
            if outcome.paste_after_hide {
                paste_into_previous_window_async(prev);
            }
        } else {
            self.action_menu_open = false;
            self.invalidate();
        }
    }

    fn handle_internal_command(&mut self, cmd: InternalCommandKind) {
        match cmd {
            InternalCommandKind::TriggerHibernateGc => {
                let cycles = self.run_hibernate();
                self.show_toast(&format!("已完成休眠内存整理 (#{cycles})"));
                self.set_query_text(":stats");
            }
            InternalCommandKind::ReloadAll => {
                self.reload_config_and_all();
                self.show_toast(self.lang.pick("已热重载配置、应用与插件", "Hot-reloaded all"));
                self.dispatch_query();
            }
            InternalCommandKind::QuitApp => {
                self.shutdown_clean();
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            InternalCommandKind::RefreshQuery => self.dispatch_query(),
            InternalCommandKind::OpenConfigFile => {
                self.open_config_file_in_editor();
                self.hide_launcher();
            }
            InternalCommandKind::OpenSettingsWindow => self.open_settings_center(),
            InternalCommandKind::ResetWindowGeometry => self.reset_window_geometry(),
            InternalCommandKind::SetTheme { theme } => {
                self.apply_theme(&theme, true);
                self.show_toast(self.lang.pick("已实时切换主题并保存", "Theme updated & saved"));
                self.dispatch_query();
            }
            InternalCommandKind::SetLanguage { lang } => {
                self.apply_language(&lang, true);
                self.show_toast(self.lang.pick("已切换界面语言", "Display language updated"));
            }
            InternalCommandKind::ToggleClipboardPin { id_key } => {
                let pinned = self.clipboard.toggle_pin(&id_key);
                self.show_toast(if pinned {
                    self.lang.pick("已置顶该剪贴板记录", "Clipboard entry pinned")
                } else {
                    self.lang.pick("已取消置顶", "Clipboard entry unpinned")
                });
                self.dispatch_query();
            }
            InternalCommandKind::CheckForUpdates => self.trigger_update_check_async(),
            InternalCommandKind::ClearAttachedContext => {
                self.attached = None;
                self.dispatch_query();
                self.invalidate();
            }
            InternalCommandKind::CopyImageAsBase64 { path } => match copy_image_as_base64_uri(&path) {
                Ok(chars) => {
                    self.show_toast(&match self.lang {
                        Lang::Zh => format!("已复制 Base64 Data URI（{chars} 字符）"),
                        Lang::En => format!("Copied Base64 Data URI ({chars} chars)"),
                    });
                    self.hide_launcher();
                }
                Err(e) => self.show_toast(&e),
            },
            InternalCommandKind::KillProcess { pid, name } => {
                match kill_process_by_pid(pid) {
                    Ok(()) => self.show_toast(&match self.lang {
                        Lang::Zh => format!("已结束进程 {name} (PID {pid})"),
                        Lang::En => format!("Terminated {name} (PID {pid})"),
                    }),
                    Err(e) => self.show_toast(&e),
                }
                self.dispatch_query();
            }
            InternalCommandKind::ToggleAutostart => {
                let target = !is_autostart_enabled();
                match set_autostart_enabled(target) {
                    Ok(()) => self.show_toast(if target {
                        self.lang.pick("已开启开机静默自启", "Launch at Windows login ENABLED")
                    } else {
                        self.lang.pick("已关闭开机自启", "Launch at Windows login DISABLED")
                    }),
                    Err(e) => self.show_toast(&format!("Autostart error: {e}")),
                }
                self.dispatch_query();
            }
            InternalCommandKind::TogglePreviewPane => {
                self.preview_enabled = !self.preview_enabled;
                self.cached_preview_id = None;
                self.refresh_selected_preview();
                self.resize_to_fit_results();
                self.invalidate();
            }
        }
    }

    pub fn is_renderer_hibernated(&self) -> bool {
        self.renderer.is_hibernated()
    }

    pub fn wait_background_idle_for_test(&mut self) {
        self.renderer.wait_icons_idle_for_test();
        self.preview_loader.wait_idle_and_drain();
    }

    pub fn self_test_paint_and_hibernate(&mut self, query: &str) -> usize {
        self.current_query = query.to_string();
        let generation = self.router.next_generation();
        self.results = self.router.search_with_filter(
            query,
            generation,
            20,
            self.clipboard.latest_snapshot(),
            self.attached.clone(),
            self.active_filter,
        );
        self.selected = 0;
        self.inter.hovered = if self.results.len() > 1 { Some(1) } else { None };
        self.scroll_offset = 0;
        self.cached_preview_id = None;
        self.refresh_selected_preview();
        let count = self.results.len();

        let hdc = unsafe { windows::Win32::Graphics::Gdi::GetDC(Some(self.hwnd)) };
        let w = (self.desired_width_dip() as f32 * self.dpi_scale) as i32;
        let h = (self.desired_height_dip() as f32 * self.dpi_scale) as i32;
        self.render_into_hdc(hdc, w, h);
        unsafe {
            windows::Win32::Graphics::Gdi::ReleaseDC(Some(self.hwnd), hdc);
        }
        count
    }

    pub fn self_test_attached_paint(&mut self, ctx: AttachedContext) -> usize {
        self.attached = Some(ctx);
        let hits = self.self_test_paint_and_hibernate("");
        self.attached = None;
        hits
    }

    pub(super) fn reload_config_and_all(&mut self) {
        let fresh = TomoyoConfig::load_or_create();
        let new_lang = fresh.lang();
        let new_theme = fresh.theme.clone();
        let new_hotkey = fresh.hotkey.clone();
        self.preview_enabled = fresh.auto_preview;
        self.show_filter_bar = fresh.show_filter_bar;
        self.geom.list_width_dip = fresh.window.width_dip.clamp(MIN_WINDOW_WIDTH_DIP, 1400);
        self.geom.max_visible_rows = fresh.window.max_visible_rows.clamp(4, 18);
        self.geom.preview_width_dip = fresh.window.preview_width_dip.clamp(220, 620);
        self.geom.custom_pos = fresh.window.custom_x.zip(fresh.window.custom_y);
        if let Ok(mut cfg) = self.config.write() {
            *cfg = fresh;
        }
        self.lang = new_lang;
        unsafe {
            apply_cue_banner(self.edit_hwnd, new_lang);
        }
        self.apply_theme(&new_theme, false);
        if self.tray.is_some() {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ID);
            }
            self.hotkey_label = register_configured_hotkey(self.hwnd, &new_hotkey);
            if let Some(tray) = &mut self.tray {
                tray.update_tooltip(&self.hotkey_label);
            }
        }
        self.router.reload_all();
        if unsafe { IsWindowVisible(self.hwnd).as_bool() } {
            self.position_on_active_monitor();
            self.dispatch_query();
            self.invalidate();
        }
    }
}
