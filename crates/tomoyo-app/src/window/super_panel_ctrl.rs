//! AppShell orchestration bridge for Tomoyo Native Super Panel:
//! mouse hold arming, lossless background probing, panel launch, and unified action dispatch.

use super::AppShell;
use crate::actions::{execute_action, set_clipboard_text, shell_open};
use crate::super_panel::{
    self, build_actions_for_context, close_active_panel, open_super_panel,
    probe_foreground_selection, should_avoid_super_panel, sync_trigger_hook,
    uninstall_trigger_hook, SuperActionKind, SuperContext, SuperPanelExec, SuperPanelLaunch,
};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use tomoyo_core::{is_safe_http_url, PLUGIN_TRANSLATE};
use tomoyo_protocol::{ActionKind, AttachedContext};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, KillTimer, PostMessageW, SetTimer, WM_APP,
};

pub(super) const WM_APP_SP_ARM: u32 = WM_APP + 50;
pub(super) const WM_APP_SP_FIRE: u32 = WM_APP + 51;
pub(super) const WM_APP_SP_PROBED: u32 = WM_APP + 52;
pub(super) const WM_APP_SP_EXEC: u32 = WM_APP + 53;
pub(super) const TIMER_SP_HOLD: usize = 1008;

pub(crate) struct SuperPanelBridge {
    pub armed_seq: u32,
    pub armed_btn: u8,
    pub probing: bool,
    pub probed_slot: Arc<Mutex<Option<(POINT, SuperContext)>>>,
    pub exec_slot: Arc<Mutex<Option<SuperPanelExec>>>,
}

impl Default for SuperPanelBridge {
    fn default() -> Self {
        Self {
            armed_seq: 0,
            armed_btn: 0,
            probing: false,
            probed_slot: Arc::new(Mutex::new(None)),
            exec_slot: Arc::new(Mutex::new(None)),
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
        open_super_panel(SuperPanelLaunch {
            anchor,
            host_hwnd: self.hwnd,
            exec_msg: WM_APP_SP_EXEC,
            exec_slot: Arc::clone(&self.super_panel.exec_slot),
            plugin_host: Arc::clone(&self.plugin_host),
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
        match exec.kind {
            SuperActionKind::QuickTranslate => {}
            SuperActionKind::OpenTranslator(text) => {
                let kind = ActionKind::PluginExecute {
                    plugin_id: PLUGIN_TRANSLATE.into(),
                    action_id: "open_tr".into(),
                    payload: serde_json::json!({ "text": text, "target_lang": "" }),
                };
                let _ = execute_action(&kind, "sp:open_tr", &self.plugin_host, self.theme.is_dark);
            }
            SuperActionKind::CopyText { text, toast } => {
                let _ = set_clipboard_text(&text);
                tracing::debug!(toast = %toast, "Super Panel copied text");
            }
            SuperActionKind::OpenUrl(url) => {
                if is_safe_http_url(&url) {
                    let _ = shell_open(&url, None, None);
                }
            }
            SuperActionKind::WebSearch(query) => {
                let tmpl = self
                    .config
                    .read()
                    .ok()
                    .and_then(|c| c.web_searches.first().map(|e| e.url.clone()))
                    .unwrap_or_else(|| "https://www.bing.com/search?q={q}".into());
                let url = tmpl.replace("{q}", &percent_encode_query(&query));
                if is_safe_http_url(&url) {
                    let _ = shell_open(&url, None, None);
                }
            }
            SuperActionKind::FillLauncher(q) => {
                self.show_launcher_with_query(Some(&q));
            }
            SuperActionKind::AttachFiles(paths) => {
                self.show_launcher_with_query(Some(""));
                self.attached = Some(AttachedContext::Files { paths });
                self.dispatch_query();
                self.invalidate();
            }
            SuperActionKind::CoreAction(kind) => {
                let out = execute_action(&kind, "sp:action", &self.plugin_host, self.theme.is_dark);
                if let Some(q) = out.fill_query {
                    self.show_launcher_with_query(Some(&q));
                }
                if let Some(t) = out.toast {
                    self.show_toast(&t);
                }
            }
        }
    }
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
