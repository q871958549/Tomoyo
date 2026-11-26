//! Linux action execution, internal commands, clipboard sync, workflows & IPC hot-reload.

use crate::actions::{
    clipboard_source_app_name, execute_action, is_clipboard_confidential, kill_process_by_pid,
    paste_into_previous_window_async, read_clipboard_files, read_clipboard_image_png,
    read_clipboard_text, send_ctrl_c_cleanly, set_clipboard_text, shell_open,
    spawn_settings_window_at,
};
use crate::linux::{
    tomoyo_linux_tray_set_menu, tomoyo_linux_tray_set_title, tomoyo_linux_ui_quit,
    LinuxTrayMenuItem,
};
use crate::linux::ipc::schedule_on_ui;
use crate::linux::shell_state::{with_current_core, AppCore};
use std::ffi::CString;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tomoyo_core::{
    download_and_verify_release_asset_with_progress, is_autostart_enabled, is_safe_http_url,
    launch_verified_update_installer, load_or_init_flows, run_flow_llm_transform,
    run_flow_pipeline, run_flow_pipeline_with_llm, set_autostart_enabled, TomoyoConfig,
    UpdateStatus,
};
use tomoyo_plugin_host::ActivePluginHotkey;
use tomoyo_protocol::{
    ActionKind, ClipboardSnapshot, ExecuteResponse, InternalCommandKind, OpenWebViewRequest,
    Permission, PluginEventKind, PluginEventPayload,
};

impl AppCore {
    pub(super) fn execute_selected(&mut self, action_idx: usize) {
        let Some(item) = self.results.get(self.selected).cloned() else { return; };
        let Some(act) = item.actions.get(action_idx).or_else(|| item.actions.first()) else { return; };
        self.router.record_execution_for_query(&self.current_query, &item.id);
        let outcome = execute_action(&act.kind, &item.id, &self.plugin_host, self.theme.is_dark);
        if let Some(cmd) = outcome.internal { self.handle_internal_command(cmd); return; }
        if let Some(q) = outcome.fill_query { self.set_query_text(&q); return; }
        if let Some(t) = outcome.toast { self.show_toast(&t); }
        if outcome.refresh_query { self.dispatch_query(); }
        if outcome.hide_window { self.hide_launcher(); }
        if outcome.paste_after_hide { paste_into_previous_window_async(0); }
    }

    fn handle_internal_command(&mut self, cmd: InternalCommandKind) {
        match cmd {
            InternalCommandKind::TriggerHibernateGc => { let c = self.run_hibernate(); self.show_toast(&format!("已完成第 {c} 次内存整理")); }
            InternalCommandKind::ReloadAll => self.reload_config_and_all(true),
            InternalCommandKind::QuitApp => { self.shutdown_clean(); unsafe { tomoyo_linux_ui_quit(); } }
            InternalCommandKind::RefreshQuery => self.dispatch_query(),
            InternalCommandKind::OpenConfigFile => { let _ = shell_open(&TomoyoConfig::config_path().to_string_lossy(), None, None); self.hide_launcher(); }
            InternalCommandKind::OpenSettingsWindow => self.open_settings_center_at(None),
            InternalCommandKind::OpenSettingsRoute { route } => {
                self.open_settings_center_at(Some(&route));
            }
            InternalCommandKind::ResetWindowGeometry => self.reset_window_geometry(),
            InternalCommandKind::SetTheme { theme } => self.apply_theme(&theme, true),
            InternalCommandKind::SetLanguage { lang } => self.apply_language(&lang, true),
            InternalCommandKind::CheckForUpdates => self.trigger_update_check_async(),
            InternalCommandKind::InstallUpdate => self.trigger_update_install_async(),
            InternalCommandKind::ClearAttachedContext => { self.attached = None; self.dispatch_query(); }
            InternalCommandKind::CopyImageAsBase64 { path } => {
                match crate::actions::copy_image_as_base64_uri(&path) {
                    Ok(n) => { self.hide_launcher(); self.show_toast(&format!("已复制 Base64 Data URI ({n} B)")); }
                    Err(e) => self.show_toast(&e),
                }
            }
            InternalCommandKind::KillProcess { pid, name } => match kill_process_by_pid(pid) {
                Ok(()) => { self.show_toast(&format!("已结束进程 {name} (PID {pid})")); self.dispatch_query(); }
                Err(e) => self.show_toast(&e),
            },
            InternalCommandKind::ToggleAutostart => { let next = !is_autostart_enabled(); let _ = set_autostart_enabled(next); self.dispatch_query(); }
            InternalCommandKind::TogglePreviewPane => { self.preview_enabled = !self.preview_enabled; self.resize_to_fit(); }
            InternalCommandKind::ExtractZipHere { path } => { match crate::actions::extract_zip_into_sibling_dir(&path) { Ok(d) => self.show_toast(&format!("已解压至 {d}")), Err(e) => self.show_toast(&e) } }
            InternalCommandKind::CopyFileContents { path } => { match crate::actions::copy_file_contents_bounded(&path) { Ok(n) => self.show_toast(&format!("已复制文件文本 ({n} B)")), Err(e) => self.show_toast(&e) } }
            InternalCommandKind::ComputeFileSha256 { path } => { match crate::actions::compute_file_sha256_bounded(&path) { Ok(h) => { let _ = set_clipboard_text(&h); self.show_toast(&format!("SHA-256 已复制: {h}")); } Err(e) => self.show_toast(&e) } }
            InternalCommandKind::RunFlow { flow_id, input } => self.execute_workflow(&flow_id, &input),
        }
    }

    pub(super) fn open_settings_center(&mut self) {
        self.open_settings_center_at(None);
    }

    pub(super) fn open_settings_center_at(&mut self, route: Option<&str>) {
        if let Err(e) = spawn_settings_window_at(std::process::id() as isize, &self.theme_name, route) {
            self.show_toast(&format!("设置中心启动失败: {e}"));
        } else { self.hide_launcher(); }
    }

    pub(super) fn on_clipboard_update(&mut self) {
        if self.suppress_clipboard.load(Ordering::Acquire) || is_clipboard_confidential() {
            return;
        }
        let sub_text = self.plugin_host.has_event_subscriber(PluginEventKind::ClipboardText);
        let sub_img = self.plugin_host.has_event_subscriber(PluginEventKind::ClipboardImage);
        let src = if sub_text || sub_img { clipboard_source_app_name() } else { String::new() };
        if sub_text && let Some(files) = read_clipboard_files() {
            self.dispatch_clipboard_event(
                PluginEventKind::ClipboardText,
                serde_json::json!({ "kind": "files", "paths": files, "source_app": src }),
            );
        } else if let Some(txt) = read_clipboard_text() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            self.last_clip_snapshot = Some(ClipboardSnapshot {
                text: txt.chars().take(2048).collect(),
                captured_at_unix: now,
            });
            if sub_text {
                self.dispatch_clipboard_event(
                    PluginEventKind::ClipboardText,
                    serde_json::json!({ "kind": "text", "text": txt, "source_app": src }),
                );
            }
        } else if sub_img && let Some((path, w, h, digest)) = read_clipboard_image_png() {
            self.dispatch_clipboard_event(
                PluginEventKind::ClipboardImage,
                serde_json::json!({
                    "path": path, "width": w, "height": h, "digest": digest, "source_app": src
                }),
            );
        }
    }

    fn dispatch_clipboard_event(&self, kind: PluginEventKind, data: serde_json::Value) {
        let host = Arc::clone(&self.plugin_host);
        let payload = PluginEventPayload::new(kind, data);
        let _ = std::thread::Builder::new()
            .name("tomoyo-clip-ev".into())
            .spawn(move || {
                let _ = host.dispatch_event(None, &payload);
            });
    }

    pub(super) fn apply_plugin_exec_response(&mut self, plugin_id: &str, r: ExecuteResponse) {
        if let Some(txt) = r.copy_text.as_deref() {
            let _ = set_clipboard_text(txt);
        }
        if let Some(url) = r.open_url.as_deref() && is_safe_http_url(url) {
            let _ = shell_open(url, None, None);
        }
        if let Some(wv_req) = &r.open_webview
            && let Some(wv_act) = self.plugin_host.resolve_webview_open(plugin_id, wv_req)
        {
            let out = execute_action(&wv_act, plugin_id, &self.plugin_host, self.theme.is_dark);
            if let Some(t) = out.toast { self.show_toast(&t); }
            if out.hide_window { self.hide_launcher(); }
            return;
        }
        if let Some(q) = r.fill_query { self.show_launcher_with_query(Some(&q)); }
        if let Some(t) = r.toast { self.show_toast(&t); }
    }

    pub(super) fn handle_plugin_hotkey(&mut self, hk: ActivePluginHotkey) {
        if !hk.trigger_query.trim().is_empty() { self.show_launcher_with_query(Some(&hk.trigger_query)); return; }
        if hk.capture_selection {
            if !self.plugin_host.plugin_has_permission(&hk.plugin_id, Permission::SelectionRead) {
                self.show_toast(&format!("插件 {} 未获授「划词读取 (SelectionRead)」权限", hk.plugin_id));
                return;
            }
            let sup = Arc::clone(&self.suppress_clipboard);
            let host = Arc::clone(&self.plugin_host);
            let _ = std::thread::Builder::new().name("tomoyo-sel-plug".into()).spawn(move || {
                sup.store(true, Ordering::Release);
                let prev = read_clipboard_text();
                send_ctrl_c_cleanly();
                std::thread::sleep(Duration::from_millis(120));
                let cap = read_clipboard_text().unwrap_or_default();
                if let Some(old) = prev.as_deref().filter(|o| *o != cap) { let _ = set_clipboard_text(old); }
                sup.store(false, Ordering::Release);
                let payload = PluginEventPayload::new(
                    PluginEventKind::HotkeyTriggered,
                    serde_json::json!({ "hotkey_id": hk.hotkey_id, "spec": hk.spec, "selected_text": cap.trim() }),
                );
                if let Some(r) = host.dispatch_event(Some(&hk.plugin_id), &payload).into_iter().next() {
                    let pid = hk.plugin_id.clone();
                    schedule_on_ui(move || with_current_core(|st| st.apply_plugin_exec_response(&pid, r)));
                }
            });
            return;
        }
        let payload = PluginEventPayload::new(
            PluginEventKind::HotkeyTriggered,
            serde_json::json!({ "hotkey_id": hk.hotkey_id, "spec": hk.spec }),
        );
        if let Some(r) = self.plugin_host.dispatch_event(Some(&hk.plugin_id), &payload).into_iter().next() {
            self.apply_plugin_exec_response(&hk.plugin_id, r);
        }
    }

    pub(super) fn toggle_hotkeys_paused(&mut self) {
        let cfg = self.config.read().map(|c| c.clone()).unwrap_or_default();
        let paused = self.hotkeys.set_paused(!self.hotkeys.is_paused(), &cfg, &self.plugin_host);
        if !self.ui.is_null()
            && let Ok(t) = CString::new(format!("Tomoyo ({})", self.hotkeys.active_label))
        {
            unsafe { tomoyo_linux_tray_set_title(self.ui, t.as_ptr()); }
        }
        self.rebuild_tray_menu();
        let msg = if paused {
            self.lang.pick("已停用全部全局快捷键（托盘右键可随时恢复）", "All global hotkeys paused")
        } else {
            self.lang.pick("已恢复全部全局快捷键", "All global hotkeys resumed")
        };
        if self.is_visible() { self.show_toast(msg); }
    }

    pub(super) fn handle_tray_update_click(&mut self) {
        if let Some(rel) = self.updater.available_release() && rel.download_url.is_some() {
            self.trigger_update_install_async();
            return;
        }
        self.show_launcher_with_query(Some(":update"));
        self.trigger_update_check_async();
    }

    pub(super) fn rebuild_tray_menu(&mut self) {
        if self.ui.is_null() { return; }
        self.tray_plugin_actions = self.plugin_host.active_tray_actions();
        let l = self.lang;
        let paused = self.hotkeys.is_paused();
        let hk_label = if paused {
            l.pick("恢复全局快捷键（已停用）", "Resume Global Hotkeys (Paused)")
        } else {
            l.pick("停用全局快捷键", "Pause Global Hotkeys")
        };
        let upd_label = match self.updater.available_release() {
            Some(r) if r.download_url.is_some() => match l {
                tomoyo_core::Lang::Zh => format!("立即自动更新 v{} 并重启", r.version),
                tomoyo_core::Lang::En => format!("Auto-Update to v{} & Restart", r.version),
            },
            Some(r) => match l {
                tomoyo_core::Lang::Zh => format!("发现新版本 v{} (:update)", r.version),
                tomoyo_core::Lang::En => format!("New Version v{} (:update)", r.version),
            },
            None => l.pick("检查更新 (:update)", "Check for Updates").into(),
        };
        let mut raw: Vec<(String, u32, bool)> = vec![
            (l.pick("显示 Tomoyo", "Show Tomoyo").into(), 1, false),
            (l.pick("设置中心...", "Settings...").into(), 2, false),
            (hk_label.into(), 3, paused),
        ];
        if !self.tray_plugin_actions.is_empty() {
            raw.push(("---".into(), 0, false));
            for (idx, act) in self.tray_plugin_actions.iter().enumerate() {
                raw.push((act.label.clone(), 100 + idx as u32, false));
            }
        }
        raw.push(("---".into(), 0, false));
        raw.push((upd_label, 4, false));
        raw.push(("---".into(), 0, false));
        raw.push((l.pick("退出 Tomoyo", "Exit Tomoyo").into(), 9, false));
        let cstrs: Vec<CString> = raw.iter().map(|(s, _, _)| CString::new(s.as_str()).unwrap_or_default()).collect();
        let items: Vec<LinuxTrayMenuItem> = cstrs.iter().zip(raw.iter()).map(|(cs, (_, cmd, chk))| LinuxTrayMenuItem {
            label: cs.as_ptr(), cmd: *cmd, parent_idx: -1, checked: i32::from(*chk), disabled: i32::from(*cmd == 0),
        }).collect();
        unsafe { tomoyo_linux_tray_set_menu(self.ui, items.as_ptr(), items.len()); }
    }

    pub(super) fn execute_plugin_tray_action(&mut self, idx: usize) {
        let Some(act) = self.tray_plugin_actions.get(idx).cloned() else { return; };
        if !act.trigger_query.trim().is_empty() {
            self.show_launcher_with_query(Some(&act.trigger_query));
            return;
        }
        let kind = if let Some(ui_act) = act.ui_action {
            ui_act
        } else {
            ActionKind::PluginExecute {
                plugin_id: act.plugin_id.clone(),
                action_id: act.action_id,
                payload: serde_json::json!({ "source": "tray", "tray_action_id": act.id }),
            }
        };
        let out = execute_action(&kind, &format!("tray:{}", act.id), &self.plugin_host, self.theme.is_dark);
        if let Some(q) = out.fill_query { self.show_launcher_with_query(Some(&q)); }
        if let Some(t) = out.toast { self.show_toast(&t); }
    }

    pub(super) fn execute_workflow(&mut self, flow_id: &str, input: &str) {
        let Some(flow) = load_or_init_flows().into_iter().find(|f| f.id.eq_ignore_ascii_case(flow_id)) else {
            self.show_toast(&format!("未找到工作流：{flow_id}")); return;
        };
        let clip = read_clipboard_text().unwrap_or_default();
        if flow.has_llm_step() {
            let tr = self.config.read().map(|c| c.translate.clone()).unwrap_or_default();
            let inp = input.to_string();
            self.show_toast(&format!("正在执行「{}」（本地/AI 处理中）…", flow.name));
            let _ = std::thread::Builder::new().name("tomoyo-flow-llm".into()).spawn(move || {
                let res = run_flow_pipeline_with_llm(&flow, &inp, &clip, |p, t| run_flow_llm_transform(&tr, p, t));
                schedule_on_ui(move || with_current_core(|st| match res {
                    Ok(plan) => st.apply_flow_plan(plan),
                    Err(e) => st.show_toast(&format!("⚠ {e}")),
                }));
            });
        } else {
            self.apply_flow_plan(run_flow_pipeline(&flow, input, &clip));
        }
    }

    fn apply_flow_plan(&mut self, plan: tomoyo_core::FlowRunPlan) {
        if let Some(t) = plan.copy_text.as_deref() { let _ = set_clipboard_text(t); }
        if let Some(u) = plan.open_url.as_deref() && is_safe_http_url(u) { let _ = shell_open(u, None, None); }
        if let Some(p) = plan.open_path.as_deref() && std::path::Path::new(p).exists() { let _ = shell_open(p, None, None); }
        if let Some(m) = plan.capture_mode.as_deref() {
            let act = ActionKind::PluginExecute {
                plugin_id: tomoyo_core::PLUGIN_CAPTURE.into(),
                action_id: format!("run_{m}"),
                payload: serde_json::json!({ "mode": m, "from_launcher": true }),
            };
            let out = execute_action(&act, "flow:capture", &self.plugin_host, self.theme.is_dark);
            if let Some(msg) = out.toast { self.show_toast(&msg); }
            if out.hide_window { self.hide_launcher(); }
            return;
        }
        if let Some(t) = plan.launch_translate.as_deref() {
            let req = OpenWebViewRequest { initial_query: t.to_string(), ..Default::default() };
            if let Some(act) = self.plugin_host.resolve_webview_open(tomoyo_core::PLUGIN_TRANSLATE, &req) {
                let out = execute_action(&act, "flow:translate", &self.plugin_host, self.theme.is_dark);
                if let Some(msg) = out.toast { self.show_toast(&msg); }
                if out.hide_window { self.hide_launcher(); }
            } else {
                self.show_toast("请先在设置中心→插件市场安装「多源对照翻译」插件");
            }
            return;
        }
        if let Some(f) = plan.fill_query.as_deref() { self.show_launcher_with_query(Some(f)); return; }
        if let Some(t) = plan.toast { self.show_toast(&t); }
        if plan.hide_window { self.hide_launcher(); }
        if plan.paste_after_hide { paste_into_previous_window_async(0); }
    }

    fn trigger_update_check_async(&mut self) {
        let repo = self.config.read().map(|c| c.update.repo.clone()).unwrap_or_else(|_| "q871958549/Tomoyo".into());
        let updater = Arc::clone(&self.updater);
        self.show_toast("正在检查更新...");
        let _ = std::thread::Builder::new().name("tomoyo-upd-chk".into()).spawn(move || {
            let _ = updater.check_now_sync(&repo);
            schedule_on_ui(|| with_current_core(|st| st.on_update_ready()));
        });
    }

    fn trigger_update_install_async(&mut self) {
        let Some(rel) = self.updater.available_release() else { self.trigger_update_check_async(); return; };
        self.show_toast(&format!("准备下载 v{} 更新包并校验 SHA-256...", rel.version));
        let _ = std::thread::Builder::new().name("tomoyo-upd-dl".into()).spawn(move || {
            let res = download_and_verify_release_asset_with_progress(&rel, |_| {}).and_then(|p| launch_verified_update_installer(&p));
            schedule_on_ui(move || with_current_core(|st| match res {
                Ok(true) => {
                    st.shutdown_clean();
                    unsafe { tomoyo_linux_ui_quit(); }
                }
                Ok(false) => st.show_toast("✓ 更新包校验通过，已在文件管理器定位"),
                Err(e) => st.show_toast(&format!("⚠ 更新失败: {e}")),
            }));
        });
    }

    pub(super) fn on_update_ready(&mut self) {
        self.rebuild_tray_menu();
        if let UpdateStatus::Available { release, .. } = self.updater.snapshot() {
            let auto_inst = self.config.read().map(|c| c.update.auto_install).unwrap_or(false);
            if auto_inst && release.download_url.is_some() {
                self.trigger_update_install_async();
                return;
            }
            self.show_toast(&format!("发现新版本 v{}！输入 :update 升级", release.version));
        }
        self.invalidate();
    }

    pub(super) fn handle_ipc_line(&mut self, line: &str) {
        match line {
            "show" => self.show_launcher_with_query(None),
            "reload:0" => self.reload_config_and_all(false),
            "reload:1" => self.reload_config_and_all(true),
            "reload:2" => { if self.is_visible() { self.dispatch_query(); } }
            "reload:3" => self.hotkeys.unregister_all(),
            "reload:4" => {
                let cfg = self.config.read().map(|c| c.clone()).unwrap_or_default();
                self.hotkeys.register_all(&cfg, &self.plugin_host);
            }
            "reload:5" => {
                tracing::info!("Updater requested graceful shutdown; exiting Tomoyo cleanly");
                self.shutdown_clean();
                unsafe { tomoyo_linux_ui_quit(); }
            }
            "capture:region" | "capture:ocr" | "capture:record" => {
                let mode = line.strip_prefix("capture:").unwrap_or("region");
                let act = ActionKind::PluginExecute {
                    plugin_id: tomoyo_core::PLUGIN_CAPTURE.into(),
                    action_id: format!("run_{mode}"),
                    payload: serde_json::json!({ "mode": mode, "from_launcher": false }),
                };
                let _ = execute_action(&act, "ipc:capture", &self.plugin_host, self.theme.is_dark);
            }
            _ => {
                if let Some(png_path) = line.strip_prefix("clip_img:") {
                    crate::actions::set_clipboard_image_from_png(png_path);
                    let _ = std::fs::remove_file(png_path);
                    self.on_clipboard_update();
                } else if let Some(file_path) = line.strip_prefix("clip_file:") {
                    crate::actions::set_clipboard_file_uri(file_path);
                    self.on_clipboard_update();
                } else if let Some(raw_json) = line.strip_prefix("clip_txt:")
                    && let Ok(txt) = serde_json::from_str::<String>(raw_json)
                {
                    let _ = set_clipboard_text(&txt);
                    self.on_clipboard_update();
                }
            }
        }
    }

    fn reload_config_and_all(&mut self, force_providers: bool) {
        let fresh = TomoyoConfig::load_or_create();
        self.lang = fresh.lang();
        self.preview_enabled = fresh.auto_preview;
        self.show_filter_bar = fresh.show_filter_bar;
        self.sync_window_geom_from_config(&fresh);
        self.apply_theme(&fresh.theme, false);
        let cfg_clone = fresh.clone();
        if let Ok(mut w) = self.config.write() { *w = fresh; }
        if force_providers { self.router.reload_all(); }
        self.sync_clipboard_watcher();
        self.hotkeys.register_all(&cfg_clone, &self.plugin_host);
        if !self.ui.is_null()
            && let Ok(t) = CString::new(format!("Tomoyo ({})", self.hotkeys.active_label))
        {
            unsafe { tomoyo_linux_tray_set_title(self.ui, t.as_ptr()); }
        }
        self.rebuild_tray_menu();
        if self.is_visible() { self.resize_to_fit(); self.dispatch_query(); }
    }
}
