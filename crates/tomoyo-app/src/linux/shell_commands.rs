//! Linux action execution, internal commands, clipboard sync, workflows & IPC hot-reload.

use crate::actions::{
    clipboard_source_app_name, execute_action, is_clipboard_confidential, kill_process_by_pid,
    paste_into_previous_window_async, read_clipboard_files, read_clipboard_image_png,
    read_clipboard_text, send_ctrl_c_cleanly, set_clipboard_text, shell_open, spawn_capture_tool,
    spawn_clipboard_manager_window, spawn_settings_window, spawn_translator_window,
};
use crate::linux::tomoyo_linux_ui_quit;
use crate::linux::ipc::schedule_on_ui;
use crate::linux::shell_state::{with_current_core, AppCore};
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
use tomoyo_protocol::{InternalCommandKind, PluginEventKind, PluginEventPayload};

impl AppCore {
    pub(super) fn execute_selected(&mut self, action_idx: usize) {
        let Some(item) = self.results.get(self.selected).cloned() else { return; };
        let Some(act) = item.actions.get(action_idx).or_else(|| item.actions.first()) else { return; };
        self.router.record_execution_for_query(&self.current_query, &item.id);
        let outcome = execute_action(&act.kind, &item.id, &self.plugin_host, self.theme.is_dark);
        if let Some(cmd) = outcome.internal { self.handle_internal_command(cmd); return; }
        if let Some(q) = outcome.fill_query { self.set_query_text(&q); return; }
        if let Some(t) = outcome.toast { self.show_toast(&t); }
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
            InternalCommandKind::OpenSettingsWindow => self.open_settings_center(),
            InternalCommandKind::ResetWindowGeometry => { self.custom_pos = None; self.resize_to_fit(); }
            InternalCommandKind::SetTheme { theme } => self.apply_theme(&theme, true),
            InternalCommandKind::SetLanguage { lang } => self.apply_language(&lang, true),
            InternalCommandKind::ToggleClipboardPin { id_key } => { self.clipboard.toggle_pin(&id_key); self.dispatch_query(); }
            InternalCommandKind::DeleteClipboardItem { id_key } => { self.clipboard.delete_item(&id_key); self.dispatch_query(); }
            InternalCommandKind::ClearUnpinnedClipboard => { let n = self.clipboard.clear_unpinned(); self.show_toast(&format!("已清空 {n} 条未固定剪贴板")); self.dispatch_query(); }
            InternalCommandKind::OpenClipboardManager { focus_key } => self.open_clipboard_workbench(focus_key.as_deref()),
            InternalCommandKind::ToggleClipboardCapture => {
                if let Ok(mut cfg) = self.config.write() { cfg.clipboard.capture_enabled = !cfg.clipboard.capture_enabled; let _ = cfg.save(); }
                self.dispatch_query();
            }
            InternalCommandKind::OpenTranslator { text, target_lang } => self.open_translator_window(&text, target_lang.as_deref()),
            InternalCommandKind::StartCapture { mode } => self.trigger_capture(&mode),
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
            InternalCommandKind::PinImageFile { path } => { if let Err(e) = crate::actions::spawn_pin_image_file(&path) { self.show_toast(&e); } else { self.hide_launcher(); } }
            InternalCommandKind::ExtractZipHere { path } => { match crate::actions::extract_zip_into_sibling_dir(&path) { Ok(d) => self.show_toast(&format!("已解压至 {d}")), Err(e) => self.show_toast(&e) } }
            InternalCommandKind::CopyFileContents { path } => { match crate::actions::copy_file_contents_bounded(&path) { Ok(n) => self.show_toast(&format!("已复制文件文本 ({n} B)")), Err(e) => self.show_toast(&e) } }
            InternalCommandKind::ComputeFileSha256 { path } => { match crate::actions::compute_file_sha256_bounded(&path) { Ok(h) => { let _ = set_clipboard_text(&h); self.show_toast(&format!("SHA-256 已复制: {h}")); } Err(e) => self.show_toast(&e) } }
            InternalCommandKind::RunFlow { flow_id, input } => self.execute_workflow(&flow_id, &input),
            InternalCommandKind::InstallQuietMd => {
                self.show_toast("正在后台下载并校验 Quiet.md 官方包…");
                let _ = std::thread::Builder::new().name("tomoyo-quietmd-setup".into()).spawn(|| {
                    match tomoyo_core::install_quiet_md_verified() {
                        Ok(m) => tracing::info!("{m}"),
                        Err(e) => tracing::warn!("Quiet.md install failed: {e}"),
                    }
                });
            }
        }
    }

    pub(super) fn open_settings_center(&mut self) {
        if let Err(e) = spawn_settings_window(std::process::id() as isize, &self.theme_name) {
            self.show_toast(&format!("设置中心启动失败: {e}"));
        } else { self.hide_launcher(); }
    }

    pub(super) fn open_clipboard_workbench(&mut self, focus: Option<&str>) {
        if let Err(e) = spawn_clipboard_manager_window(std::process::id() as isize, &self.theme_name, focus) {
            self.show_toast(&format!("剪贴板工作台启动失败: {e}"));
        } else { self.hide_launcher(); }
    }

    pub(super) fn open_translator_window(&mut self, txt: &str, tgt: Option<&str>) {
        if let Err(e) = spawn_translator_window(std::process::id() as isize, &self.theme_name, txt, tgt) {
            self.show_toast(&format!("翻译窗口启动失败: {e}"));
        } else { self.hide_launcher(); }
    }

    pub(super) fn trigger_capture(&mut self, mode: &str) {
        self.trigger_capture_ex(mode, true);
    }

    pub(super) fn trigger_capture_ex(&mut self, mode: &str, hide_launcher_first: bool) {
        let was_visible = self.is_visible();
        if hide_launcher_first {
            self.hide_launcher();
        }
        if hide_launcher_first && was_visible {
            let m = mode.to_string();
            let _ = std::thread::Builder::new().name("tomoyo-cap-delay".into()).spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(75));
                let _ = spawn_capture_tool(&m);
            });
        } else if let Err(e) = spawn_capture_tool(mode) {
            self.show_toast(&format!("截图工具启动失败: {e}"));
        }
    }

    pub(super) fn on_clipboard_update(&mut self) {
        if self.suppress_clipboard.load(Ordering::Acquire) { return; }
        let (on, imgs, excl) = self.config.read().map(|c| (c.clipboard.capture_enabled, c.clipboard.capture_images, c.clipboard.excluded_apps.clone())).unwrap_or((true, true, vec![]));
        if !on || is_clipboard_confidential() { return; }
        let src = clipboard_source_app_name();
        if !src.is_empty() && excl.iter().any(|e| e.eq_ignore_ascii_case(&src)) { return; }
        if let Some(files) = read_clipboard_files() { self.clipboard.push_files_with_source(files, &src); }
        else if let Some(txt) = read_clipboard_text() {
            if self.plugin_host.has_event_subscriber(PluginEventKind::ClipboardText) {
                let host = Arc::clone(&self.plugin_host);
                let payload = PluginEventPayload { event: PluginEventKind::ClipboardText, data: serde_json::json!({ "text": txt, "source_app": src }) };
                let _ = std::thread::Builder::new().name("tomoyo-clip-ev".into()).spawn(move || { let _ = host.dispatch_event(None, &payload); });
            }
            self.clipboard.push_text_with_source(txt, &src);
        } else if imgs && let Some((path, w, h, digest)) = read_clipboard_image_png() {
            self.clipboard.push_image_with_source(path, w, h, digest, &src);
        }
    }

    pub(super) fn trigger_selection_translate(&mut self) {
        let theme = self.theme_name.clone();
        let sup = Arc::clone(&self.suppress_clipboard);
        let pid = std::process::id() as isize;
        let _ = std::thread::Builder::new().name("tomoyo-sel-tr".into()).spawn(move || {
            sup.store(true, Ordering::Release);
            let prev = read_clipboard_text();
            send_ctrl_c_cleanly();
            std::thread::sleep(Duration::from_millis(120));
            let cap = read_clipboard_text().unwrap_or_default();
            if let Some(old) = prev.as_deref().filter(|o| *o != cap) { let _ = set_clipboard_text(old); }
            sup.store(false, Ordering::Release);
            let _ = spawn_translator_window(pid, &theme, cap.trim(), None);
        });
    }

    pub(super) fn handle_plugin_hotkey(&mut self, hk: ActivePluginHotkey) {
        if !hk.trigger_query.trim().is_empty() { self.show_launcher_with_query(Some(&hk.trigger_query)); return; }
        let payload = PluginEventPayload { event: PluginEventKind::HotkeyTriggered, data: serde_json::json!({ "hotkey_id": hk.hotkey_id, "spec": hk.spec }) };
        if let Some(r) = self.plugin_host.dispatch_event(Some(&hk.plugin_id), &payload).into_iter().next() {
            if let Some(q) = r.fill_query { self.show_launcher_with_query(Some(&q)); }
            if let Some(t) = r.toast { self.show_toast(&t); }
        }
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
        if let Some(m) = plan.capture_mode.as_deref() { self.trigger_capture(m); return; }
        if let Some(t) = plan.launch_translate.as_deref() { self.open_translator_window(t, None); return; }
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
        self.show_toast(&format!("准备下载 v{} 更新包...", rel.version));
        let _ = std::thread::Builder::new().name("tomoyo-upd-dl".into()).spawn(move || {
            let res = download_and_verify_release_asset_with_progress(&rel, |_| {}).and_then(|p| launch_verified_update_installer(&p));
            schedule_on_ui(move || with_current_core(|st| match res {
                Ok(_) => st.show_toast("✓ 更新包已启动安装，即将重启 Tomoyo"),
                Err(e) => st.show_toast(&format!("⚠ 更新失败: {e}")),
            }));
        });
    }

    pub(super) fn on_update_ready(&mut self) {
        if let UpdateStatus::Available { release, .. } = self.updater.snapshot() {
            self.show_toast(&format!("发现新版本 v{}！输入 :update 升级", release.version));
        }
        self.invalidate();
    }

    pub(super) fn handle_ipc_line(&mut self, line: &str) {
        match line {
            "show" => self.show_launcher_with_query(None),
            "reload:0" => self.reload_config_and_all(false),
            "reload:1" => self.reload_config_and_all(true),
            "reload:2" => { self.clipboard.reload_from_db(); if self.is_visible() { self.dispatch_query(); } }
            "reload:3" => self.hotkeys.unregister_all(),
            "reload:4" => {
                let cfg = self.config.read().map(|c| c.clone()).unwrap_or_default();
                self.hotkeys.register_all(&cfg, &self.plugin_host);
            }
            "capture:region" => self.trigger_capture("region"),
            "capture:ocr" => self.trigger_capture("ocr"),
            "capture:record" => self.trigger_capture("record"),
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
        self.list_width_dip = fresh.window.width_dip.clamp(520, 1400);
        self.max_visible_rows = fresh.window.max_visible_rows.clamp(4, 18);
        self.preview_width_dip = fresh.window.preview_width_dip.clamp(220, 620);
        self.apply_theme(&fresh.theme, false);
        let cfg_clone = fresh.clone();
        if let Ok(mut w) = self.config.write() { *w = fresh; }
        if force_providers { self.router.reload_all(); }
        self.hotkeys.register_all(&cfg_clone, &self.plugin_host);
        if self.is_visible() { self.dispatch_query(); }
    }
}
