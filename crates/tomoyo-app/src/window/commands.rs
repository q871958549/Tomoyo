//! Action execution, internal commands, Super Match paste, async preview & async updater.

use super::{AppShell, EDIT_CTRL_ID};
use crate::actions::{
    compute_file_sha256_bounded, copy_file_contents_bounded, copy_image_as_base64_uri,
    execute_action, extract_zip_into_sibling_dir, kill_process_by_pid,
    paste_into_previous_window_async, read_clipboard_files, read_clipboard_image_png,
    spawn_capture_tool, spawn_pin_image_file,
};
use crate::tray::{
    MENU_ID_CAPTURE, MENU_ID_CLIPBOARD, MENU_ID_CONFIG, MENU_ID_GC, MENU_ID_QUIT, MENU_ID_RELOAD,
    MENU_ID_SETTINGS, MENU_ID_SHOW, MENU_ID_STATS, MENU_ID_TRANSLATE, MENU_ID_UPDATE,
};
use tomoyo_core::{is_autostart_enabled, set_autostart_enabled, Lang};
use tomoyo_protocol::{ActionKind, AttachedContext, InternalCommandKind};
use windows::Win32::Foundation::{HWND, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, IsWindowVisible, EN_CHANGE};

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
        MENU_ID_CLIPBOARD => app.open_clipboard_workbench(None),
        MENU_ID_TRANSLATE => app.open_translator_window("", None),
        MENU_ID_CAPTURE => app.trigger_capture("region"),
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
            InternalCommandKind::DeleteClipboardItem { id_key } => {
                if self.clipboard.delete_item(&id_key) {
                    self.show_toast(self.lang.pick("已删除该条剪贴板记录", "Deleted clipboard entry"));
                }
                self.dispatch_query();
            }
            InternalCommandKind::ClearUnpinnedClipboard => {
                let n = self.clipboard.clear_unpinned();
                self.show_toast(&match self.lang {
                    Lang::Zh => format!("已清理 {n} 条非置顶剪贴板记录（置顶已保留）"),
                    Lang::En => format!("Cleared {n} unpinned clipboard entries"),
                });
                self.dispatch_query();
            }
            InternalCommandKind::OpenClipboardManager { focus_key } => {
                self.open_clipboard_workbench(focus_key.as_deref());
            }
            InternalCommandKind::ToggleClipboardCapture => {
                let enabled = if let Ok(mut cfg) = self.config.write() {
                    cfg.clipboard.capture_enabled = !cfg.clipboard.capture_enabled;
                    let _ = cfg.save();
                    cfg.clipboard.capture_enabled
                } else {
                    true
                };
                self.show_toast(if enabled {
                    self.lang.pick("已恢复后台剪贴板采集", "Clipboard capture resumed")
                } else {
                    self.lang.pick("已暂停后台剪贴板采集", "Clipboard capture paused")
                });
                self.dispatch_query();
            }
            InternalCommandKind::OpenTranslator { text, target_lang } => {
                self.open_translator_window(&text, target_lang.as_deref());
            }
            InternalCommandKind::StartCapture { mode } => self.trigger_capture(&mode),
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
            InternalCommandKind::PinImageFile { path } => match spawn_pin_image_file(&path) {
                Ok(()) => self.hide_launcher(),
                Err(e) => self.show_toast(&e),
            },
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

    /// Hides the launcher if visible and launches the ephemeral `tomoyo-capture.exe` tool.
    pub fn trigger_capture(&mut self, mode: &str) {
        let was_visible = unsafe { IsWindowVisible(self.hwnd).as_bool() };
        if was_visible {
            self.hide_launcher();
        }
        let mode_owned = mode.to_string();
        if was_visible && mode_owned != "pin_clipboard" {
            // Allow DWM 110ms to finish hiding Tomoyo's launcher before BitBlt captures the screen.
            let _ = std::thread::Builder::new()
                .name("tomoyo-cap-delay".into())
                .spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(110));
                    if let Err(e) = spawn_capture_tool(&mode_owned) {
                        tracing::error!("Failed to launch tomoyo-capture.exe: {e}");
                    }
                });
        } else if let Err(e) = spawn_capture_tool(&mode_owned) {
            tracing::error!("Failed to launch tomoyo-capture.exe: {e}");
            self.show_toast(&format!("截图工具启动失败: {e}"));
        }
    }
}
