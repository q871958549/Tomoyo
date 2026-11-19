//! Action execution, internal commands, Super Match paste, preview resolution & async updater.

use super::{AppShell, WM_APP_UPDATE_READY};
use crate::actions::{
    copy_image_as_base64_uri, execute_action, kill_process_by_pid,
    paste_into_previous_window_async, read_clipboard_files, read_clipboard_image_png,
};
use std::ffi::c_void;
use std::sync::Arc;
use tomoyo_core::{
    is_autostart_enabled, read_file_preview, read_image_thumbnail, set_autostart_enabled, Lang,
    ResolvedPreview, UpdateStatus,
};
use tomoyo_protocol::{ActionKind, AttachedContext, InternalCommandKind, PreviewSpec};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, PostMessageW};

impl AppShell {
    pub(super) fn refresh_selected_preview(&mut self) {
        if !self.preview_enabled {
            self.cached_preview_id = None;
            self.cached_preview = None;
            return;
        }
        let Some(item) = self.results.get(self.selected) else {
            self.cached_preview_id = None;
            self.cached_preview = None;
            return;
        };
        if self.cached_preview_id.as_deref() == Some(&item.id) {
            return;
        }
        self.cached_preview_id = Some(item.id.clone());
        self.cached_preview = item.preview.as_ref().map(|spec| match spec {
            PreviewSpec::Text { title, body } => ResolvedPreview {
                title: title.clone(),
                meta_line: match self.lang {
                    Lang::Zh => format!("{} 字符 · {} 行", body.chars().count(), body.lines().count().max(1)),
                    Lang::En => format!("{} chars · {} lines", body.chars().count(), body.lines().count().max(1)),
                },
                body: body.clone(),
                is_monospace: body.contains('{') || body.contains('\n'),
                image_rgba: None,
            },
            PreviewSpec::KeyValue { title, entries } => {
                let body = entries
                    .iter()
                    .map(|(k, v)| format!("{k:<14} {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                ResolvedPreview {
                    title: title.clone(),
                    meta_line: match self.lang {
                        Lang::Zh => format!("{} 项属性", entries.len()),
                        Lang::En => format!("{} properties", entries.len()),
                    },
                    body,
                    is_monospace: true,
                    image_rgba: None,
                }
            }
            PreviewSpec::FilePreview { path } => read_file_preview(path),
            PreviewSpec::ImagePreview { path, .. } => read_image_thumbnail(path, 280, 200),
        });
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
                self.plugin_host.shutdown_all();
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
}
