//! Action execution, internal commands, Super Match paste, async preview & async updater.

use super::{AppShell, EDIT_CTRL_ID};
use crate::actions::{
    compute_file_sha256_bounded, copy_file_contents_bounded, copy_image_as_base64_uri,
    execute_action, extract_zip_into_sibling_dir, kill_process_by_pid,
    paste_into_previous_window_async, read_clipboard_files, read_clipboard_image_png,
};
use crate::tray::{
    MENU_ID_CONFIG, MENU_ID_GC, MENU_ID_PLUGIN_TRAY_BASE, MENU_ID_QUIT, MENU_ID_RELOAD,
    MENU_ID_SETTINGS, MENU_ID_SHOW, MENU_ID_STATS, MENU_ID_UPDATE,
};
use tomoyo_core::{is_autostart_enabled, set_autostart_enabled, Lang};
use tomoyo_protocol::{ActionKind, AttachedContext, InternalCommandKind, PluginEventKind};
use windows::Win32::Foundation::{HWND, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, EN_CHANGE};

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
            app.show_launcher();
            app.set_query_text(":update");
            app.trigger_update_check_async();
        }
        MENU_ID_STATS => {
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
        id if id >= MENU_ID_PLUGIN_TRAY_BASE => {
            super::plugin_hotkeys::execute_plugin_tray_action(
                app,
                (id - MENU_ID_PLUGIN_TRAY_BASE) as usize,
            );
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
            super::plugin_hotkeys::dispatch_clipboard_event(
                self,
                PluginEventKind::ClipboardImage,
                serde_json::json!({ "path": path, "width": w, "height": h, "digest": digest, "source_app": "" }),
            );
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

        self.router
            .record_execution_for_query(&self.current_query, &item.id);
        if item.provider_id == "builtin.everything"
            && !item.id.starts_with("everything.")
            && !item.id.starts_with("ev:")
        {
            tomoyo_providers::record_everything_query(&self.current_query);
        }
        if matches!(action.kind, ActionKind::CopyImage { .. }) {
            self.inter.suppress_next_clip_img = true;
        }
        if let Some((hide_win, toast)) = self.try_execute_webview_action(&action.kind, &action.id) {
            if let Some(t) = toast { self.show_toast(&t); }
            if hide_win {
                self.hide_launcher();
            } else {
                self.action_menu_open = false;
                self.invalidate();
            }
            return;
        }
        if self.try_start_async_launch(&action.kind, &item.id, &item.title) {
            return;
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
        if outcome.refresh_query {
            self.action_menu_open = false;
            self.dispatch_query();
            self.invalidate();
        } else if outcome.hide_window {
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
            InternalCommandKind::CheckForUpdates => self.trigger_update_check_async(),
            InternalCommandKind::InstallUpdate => self.trigger_update_install_async(),
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
            InternalCommandKind::ExtractZipHere { path } => match extract_zip_into_sibling_dir(&path) {
                Ok(dest) => {
                    self.show_toast(&format!("已安全解压到 {dest}"));
                    self.hide_launcher();
                }
                Err(e) => self.show_toast(&e),
            },
            InternalCommandKind::CopyFileContents { path } => match copy_file_contents_bounded(&path) {
                Ok(chars) => {
                    self.show_toast(&format!("已复制文件内容（{chars} 字符）"));
                    self.hide_launcher();
                }
                Err(e) => self.show_toast(&e),
            },
            InternalCommandKind::ComputeFileSha256 { path } => match compute_file_sha256_bounded(&path) {
                Ok(hex) => {
                    self.show_toast(&format!("SHA-256 已复制：{}…", &hex[..16]));
                    self.hide_launcher();
                }
                Err(e) => self.show_toast(&e),
            },
            InternalCommandKind::RunFlow { flow_id, input } => self.execute_workflow(&flow_id, &input),
        }
    }
}
