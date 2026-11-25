//! AppShell orchestration bridge for Tomoyo Native Super Panel:
//! balanced hold arming, lossless background probing, lock-free inline translation,
//! and strictly non-blocking action dispatch (never stalls the Win32 `WH_MOUSE_LL` thread).

use super::AppShell;
use crate::actions::{execute_action, set_clipboard_text, shell_open};
use crate::super_panel::{
    self, build_actions_for_context, close_active_panel, inline_trans, open_super_panel,
    probe_foreground_selection, should_avoid_super_panel, sync_trigger_hook,
    uninstall_trigger_hook, SuperActionKind, SuperContext, SuperPanelExec, SuperPanelLaunch,
};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use tomoyo_core::{data_dir, is_safe_http_url, PLUGIN_TRANSLATE};
use tomoyo_protocol::{ActionKind, AttachedContext, OpenWebViewRequest};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, KillTimer, PostMessageW, SetTimer, WM_APP,
};

pub(super) const WM_APP_SP_ARM: u32 = WM_APP + 50;
pub(super) const WM_APP_SP_FIRE: u32 = WM_APP + 51;
pub(super) const WM_APP_SP_PROBED: u32 = WM_APP + 52;
pub(super) const WM_APP_SP_EXEC: u32 = WM_APP + 53;
pub(super) const WM_APP_SP_EXEC_DONE: u32 = WM_APP + 54;
pub(super) const TIMER_SP_HOLD: usize = 1008;

#[derive(Debug, Default)]
pub(crate) struct AsyncExecOutcome {
    pub fill_query: Option<String>,
    pub toast: Option<String>,
}

pub(crate) struct SuperPanelBridge {
    pub armed_seq: u32,
    pub armed_btn: u8,
    pub probing: bool,
    pub source_fg_hwnd: isize,
    pub probed_slot: Arc<Mutex<Option<(POINT, SuperContext)>>>,
    pub exec_slot: Arc<Mutex<Option<SuperPanelExec>>>,
    pub exec_done_slot: Arc<Mutex<Option<AsyncExecOutcome>>>,
}

impl Default for SuperPanelBridge {
    fn default() -> Self {
        Self {
            armed_seq: 0,
            armed_btn: 0,
            probing: false,
            source_fg_hwnd: 0,
            probed_slot: Arc::new(Mutex::new(None)),
            exec_slot: Arc::new(Mutex::new(None)),
            exec_done_slot: Arc::new(Mutex::new(None)),
        }
    }
}

impl AppShell {
    /// Syncs the zero-thread `WH_MOUSE_LL` trigger hook with current `SuperPanelConfig`.
    /// No-op when running in `--self-test` (`self.tray.is_none()`).
    pub(super) fn sync_super_panel_hook(&self) {
        if self.tray.is_none() {
            return;
        }
        if let Ok(cfg) = self.config.read() {
            sync_trigger_hook(self.hwnd, &cfg.super_panel, WM_APP_SP_ARM, WM_APP_SP_FIRE);
        }
    }

    pub(super) fn shutdown_super_panel(&self) {
        close_active_panel();
        uninstall_trigger_hook();
    }

    pub(super) fn on_super_panel_arm(&mut self, seq: u32, btn: u8) {
        self.super_panel.armed_seq = seq;
        self.super_panel.armed_btn = btn;
        let hold_ms = self
            .config
            .read()
            .map(|c| c.super_panel.hold_ms.clamp(180, 900))
            .unwrap_or(320);
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SP_HOLD);
            let _ = SetTimer(Some(self.hwnd), TIMER_SP_HOLD, hold_ms, None);
        }
    }

    pub(super) fn on_super_panel_hold_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SP_HOLD);
        }
        if !self.can_trigger_super_panel() {
            super_panel::trigger::abort_armed_hold(
                self.super_panel.armed_seq,
                self.super_panel.armed_btn,
            );
            return;
        }
        let Some(pt) = super_panel::consume_hold_if_valid(
            self.super_panel.armed_seq,
            self.super_panel.armed_btn,
        ) else {
            return;
        };
        self.begin_super_panel_probe(pt);
    }

    pub(super) fn on_super_panel_fire_click(&mut self) {
        if !self.can_trigger_super_panel() {
            return;
        }
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        self.begin_super_panel_probe(pt);
    }

    fn can_trigger_super_panel(&self) -> bool {
        if self.super_panel.probing || super_panel::is_panel_open() {
            return false;
        }
        let (enabled, extra_excl) = self
            .config
            .read()
            .map(|c| (c.super_panel.enabled, c.super_panel.excluded_apps.clone()))
            .unwrap_or((false, Vec::new()));
        enabled && !should_avoid_super_panel(self.hwnd, &extra_excl)
    }

    fn begin_super_panel_probe(&mut self, pt: POINT) {
        self.super_panel.probing = true;
        let fg = unsafe { GetForegroundWindow() };
        self.super_panel.source_fg_hwnd = if !fg.is_invalid() && fg != self.hwnd {
            fg.0 as isize
        } else {
            0
        };
        let suppress = Arc::clone(&self.suppress_clipboard_capture);
        let slot = Arc::clone(&self.super_panel.probed_slot);
        let hwnd_raw = self.hwnd.0 as usize;
        let _ = std::thread::Builder::new()
            .name("tomoyo-sp-probe".into())
            .spawn(move || {
                let ctx = probe_foreground_selection(&suppress);
                if let Ok(mut g) = slot.lock() {
                    *g = Some((pt, ctx));
                }
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut c_void)),
                        WM_APP_SP_PROBED,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            });
    }

    pub(super) fn on_super_panel_probed(&mut self) {
        self.super_panel.probing = false;
        let taken = self.super_panel.probed_slot.lock().ok().and_then(|mut g| g.take());
        let Some((anchor, ctx)) = taken else {
            return;
        };
        let Ok(cfg) = self.config.read().map(|c| c.clone()) else {
            return;
        };
        if !cfg.super_panel.enabled {
            return;
        }
        let actions = build_actions_for_context(&ctx, &cfg, &self.plugin_host, self.lang);
        let translate_cfg = inline_trans::effective_translate_config(&cfg);
        open_super_panel(SuperPanelLaunch {
            anchor,
            host_hwnd: self.hwnd,
            exec_msg: WM_APP_SP_EXEC,
            exec_slot: Arc::clone(&self.super_panel.exec_slot),
            translate_cfg,
            pal: self.theme,
            lang: self.lang,
            font_regular: self.font_regular,
            font_medium: self.font_medium,
            typography: self.typography.clone(),
            ctx,
            actions,
            auto_translate: cfg.super_panel.auto_translate_selection,
            translate_enabled: cfg.is_plugin_enabled(PLUGIN_TRANSLATE),
        });
    }

    pub(super) fn on_super_panel_exec(&mut self) {
        let taken = self.super_panel.exec_slot.lock().ok().and_then(|mut g| g.take());
        let Some(exec) = taken else {
            return;
        };
        let saved_fg = self.super_panel.source_fg_hwnd;
        match exec.kind {
            SuperActionKind::QuickTranslate => {}
            SuperActionKind::OpenTranslator(text) => {
                self.spawn_open_translator_async(text);
            }
            SuperActionKind::CopyText { text, toast } => {
                let ok = set_clipboard_text(&text);
                if !ok {
                    tracing::warn!("Super Panel set_clipboard_text failed");
                } else {
                    tracing::debug!(toast = %toast, "Super Panel copied text");
                }
            }
            SuperActionKind::OpenUrl(url) => {
                if is_safe_http_url(&url) && !shell_open(&url, None, None) {
                    tracing::warn!(url = %url, "Super Panel OpenUrl failed");
                }
            }
            SuperActionKind::WebSearch(query) => {
                let compact: String = query
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(300)
                    .collect();
                if compact.is_empty() {
                    return;
                }
                let tmpl = self
                    .config
                    .read()
                    .ok()
                    .and_then(|c| c.web_searches.first().map(|e| e.url.clone()))
                    .unwrap_or_else(|| "https://www.bing.com/search?q={q}".into());
                let url = tmpl.replace("{q}", &percent_encode_query(&compact));
                if is_safe_http_url(&url) && !shell_open(&url, None, None) {
                    tracing::warn!(url = %url, "Super Panel WebSearch failed");
                }
            }
            SuperActionKind::FillLauncher(q) => {
                self.show_launcher_with_query(Some(&q));
                if saved_fg != 0 {
                    self.inter.prev_foreground_hwnd = saved_fg;
                }
            }
            SuperActionKind::AttachFiles(paths) => {
                self.show_launcher_with_query(Some(""));
                if saved_fg != 0 {
                    self.inter.prev_foreground_hwnd = saved_fg;
                }
                self.attached = Some(AttachedContext::Files { paths });
                self.dispatch_query();
                self.invalidate();
            }
            SuperActionKind::CoreAction(kind) => {
                if matches!(kind, ActionKind::PluginExecute { .. } | ActionKind::OpenWebView { .. }) {
                    self.spawn_core_action_async(kind);
                } else {
                    let out = execute_action(&kind, "sp:action", &self.plugin_host, self.theme.is_dark);
                    self.apply_async_exec_outcome(AsyncExecOutcome {
                        fill_query: out.fill_query,
                        toast: out.toast,
                    });
                }
            }
        }
    }

    pub(super) fn on_super_panel_exec_done(&mut self) {
        let taken = self.super_panel.exec_done_slot.lock().ok().and_then(|mut g| g.take());
        if let Some(outcome) = taken {
            self.apply_async_exec_outcome(outcome);
        }
    }

    fn apply_async_exec_outcome(&mut self, outcome: AsyncExecOutcome) {
        let saved_fg = self.super_panel.source_fg_hwnd;
        if let Some(q) = outcome.fill_query {
            self.show_launcher_with_query(Some(&q));
            if saved_fg != 0 {
                self.inter.prev_foreground_hwnd = saved_fg;
            }
        }
        if let Some(t) = outcome.toast {
            self.show_toast(&t);
        }
    }

    /// Launches the Multi-Engine Translator WebView immediately off the UI thread without
    /// waiting on any in-flight STDIO RPC or stalling `WH_MOUSE_LL`.
    fn spawn_open_translator_async(&self, text: String) {
        let plugin_host = Arc::clone(&self.plugin_host);
        let is_dark = self.theme.is_dark;
        let done_slot = Arc::clone(&self.super_panel.exec_done_slot);
        let hwnd_raw = self.hwnd.0 as usize;
        let _ = std::thread::Builder::new()
            .name("tomoyo-sp-tr-open".into())
            .spawn(move || {
                let trimmed = text.trim();
                write_last_translate_request(trimmed, "");
                let initial_query = if trimmed.chars().count() <= 2000 {
                    serde_json::json!({ "text": trimmed, "target_lang": "" }).to_string()
                } else {
                    String::new()
                };
                let wv_req = OpenWebViewRequest {
                    entry: None,
                    title: Some("Tomoyo 多源翻译".into()),
                    initial_query,
                    width: Some(780),
                    height: Some(620),
                };
                let action = plugin_host
                    .resolve_webview_open(PLUGIN_TRANSLATE, &wv_req)
                    .unwrap_or_else(|| ActionKind::PluginExecute {
                        plugin_id: PLUGIN_TRANSLATE.into(),
                        action_id: "open_tr".into(),
                        payload: serde_json::json!({ "text": trimmed, "target_lang": "" }),
                    });
                let out = execute_action(&action, "sp:open_tr", &plugin_host, is_dark);
                if let Some(toast) = out.toast {
                    tracing::warn!(toast = %toast, "Super Panel OpenTranslator reported warning");
                    notify_exec_done(done_slot, hwnd_raw, AsyncExecOutcome { fill_query: out.fill_query, toast: Some(toast) });
                }
            });
    }

    fn spawn_core_action_async(&self, kind: ActionKind) {
        let plugin_host = Arc::clone(&self.plugin_host);
        let is_dark = self.theme.is_dark;
        let done_slot = Arc::clone(&self.super_panel.exec_done_slot);
        let hwnd_raw = self.hwnd.0 as usize;
        let _ = std::thread::Builder::new()
            .name("tomoyo-sp-exec".into())
            .spawn(move || {
                let out = execute_action(&kind, "sp:action", &plugin_host, is_dark);
                if out.fill_query.is_some() || out.toast.is_some() {
                    notify_exec_done(
                        done_slot,
                        hwnd_raw,
                        AsyncExecOutcome { fill_query: out.fill_query, toast: out.toast },
                    );
                }
            });
    }
}

fn notify_exec_done(
    slot: Arc<Mutex<Option<AsyncExecOutcome>>>,
    hwnd_raw: usize,
    outcome: AsyncExecOutcome,
) {
    if let Ok(mut g) = slot.lock() {
        *g = Some(outcome);
    }
    unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd_raw as *mut c_void)),
            WM_APP_SP_EXEC_DONE,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

fn write_last_translate_request(text: &str, target_lang: &str) {
    let dir = data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let body = serde_json::json!({ "text": text, "target_lang": target_lang }).to_string();
    let _ = std::fs::write(dir.join("last_translate_req.json"), body);
}

fn percent_encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}
