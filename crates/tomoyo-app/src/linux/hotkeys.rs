//! Linux global hotkey orchestration via KDE `org.kde.kglobalaccel` D-Bus + health tracking.

use crate::linux::ipc::schedule_on_ui;
use crate::linux::portal_hotkeys::{
    apply_portal_trigger_updates, build_portal_slots, kga_service_available,
    parse_portal_trigger_pairs, spec_to_qt_key, PortalHotkeyHandle,
};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::{Arc, Mutex};
use tomoyo_core::{
    load_or_init_flows, save_hotkey_health, HotkeyHealthReport, HotkeySlotStatus, TomoyoConfig,
};
use tomoyo_plugin_host::{ActivePluginHotkey, PluginHost};

unsafe extern "C" {
    fn tomoyo_kga_connect(cb: unsafe extern "C" fn(*mut c_void, *const c_char), ctx: *mut c_void) -> *mut c_void;
    fn tomoyo_kga_register(
        hk: *mut c_void,
        act_id: *const c_char,
        friendly: *const c_char,
        qt_key: c_int,
        allow_override: c_int,
        owner_out: *mut c_char,
        olen: usize,
    ) -> c_int;
    fn tomoyo_kga_unregister(hk: *mut c_void, act_id: *const c_char);
    fn tomoyo_kga_purge_all(hk: *mut c_void);
    fn tomoyo_kga_unblock_global(hk: *mut c_void);
    fn tomoyo_kga_destroy(hk: *mut c_void);
}

#[derive(Clone)]
pub enum HotkeyAction {
    ToggleMain,
    SuperPanel,
    Plugin(ActivePluginHotkey),
    Flow(String),
}

type FireHandler = Arc<Mutex<Option<Box<dyn Fn(HotkeyAction) + Send + Sync>>>>;
type HotkeyCtx = (Arc<Mutex<Vec<(String, HotkeyAction)>>>, FireHandler, Arc<Mutex<HotkeyHealthReport>>);

pub struct HotkeyManager {
    kga: *mut c_void,
    portal: Option<PortalHotkeyHandle>,
    registered_ids: Vec<String>,
    slots: Arc<Mutex<Vec<(String, HotkeyAction)>>>,
    on_fire: FireHandler,
    last_health: Arc<Mutex<HotkeyHealthReport>>,
    paused: bool,
    owns_kga_slots: bool,
    pub active_label: String,
    pub conflict_msg: Option<String>,
}

unsafe extern "C" fn on_kga_fired(ctx: *mut c_void, act_ptr: *const c_char) {
    if ctx.is_null() || act_ptr.is_null() {
        return;
    }
    let act_id = unsafe { CStr::from_ptr(act_ptr) }.to_string_lossy().into_owned();
    let state = unsafe { &*(ctx as *const HotkeyCtx) };
    let action_opt = state
        .0
        .lock()
        .ok()
        .and_then(|list| list.iter().find(|(id, _)| id == &act_id).map(|(_, a)| a.clone()));
    if let Some(action) = action_opt {
        tracing::debug!(action = %act_id, "KDE KGlobalAccel shortcut fired");
        let cb_arc = Arc::clone(&state.1);
        schedule_on_ui(move || {
            if let Ok(g) = cb_arc.lock()
                && let Some(cb) = g.as_ref()
            {
                cb(action);
            }
        });
    } else {
        tracing::warn!(action = %act_id, "Ignored unmatched KDE KGlobalAccel action");
    }
}

unsafe extern "C" fn on_portal_changed(ctx: *mut c_void, pairs_ptr: *const c_char) {
    if ctx.is_null() || pairs_ptr.is_null() {
        return;
    }
    let raw = unsafe { CStr::from_ptr(pairs_ptr) }.to_string_lossy().into_owned();
    let pairs = parse_portal_trigger_pairs(&raw);
    if pairs.is_empty() {
        return;
    }
    let state = unsafe { &*(ctx as *const HotkeyCtx) };
    if let Ok(mut health) = state.2.lock()
        && let Some(actual) = apply_portal_trigger_updates(&pairs, &mut health)
    {
        tracing::info!(main_trigger = %actual, "Wayland GlobalShortcuts Portal readback updated");
        save_hotkey_health(&health);
    }
}

impl HotkeyManager {
    pub fn new() -> Self {
        let slots = Arc::new(Mutex::new(Vec::new()));
        let on_fire: FireHandler = Arc::new(Mutex::new(None));
        let last_health = Arc::new(Mutex::new(HotkeyHealthReport::default()));
        let box_ctx = Box::new((Arc::clone(&slots), Arc::clone(&on_fire), Arc::clone(&last_health)));
        let ctx_ptr = Box::into_raw(box_ctx);
        let (kga, portal) = if kga_service_available() {
            (unsafe { tomoyo_kga_connect(on_kga_fired, ctx_ptr.cast()) }, None)
        } else {
            (
                std::ptr::null_mut(),
                PortalHotkeyHandle::connect(on_kga_fired, on_portal_changed, ctx_ptr.cast()),
            )
        };
        Self {
            kga,
            portal,
            registered_ids: Vec::new(),
            slots,
            on_fire,
            last_health,
            paused: false,
            owns_kga_slots: false,
            active_label: "Alt+Space".into(),
            conflict_msg: None,
        }
    }

    /// Creates an inert manager for headless `--self-test` / non-tray shells without touching D-Bus.
    pub fn new_inactive() -> Self {
        Self {
            kga: std::ptr::null_mut(),
            portal: None,
            registered_ids: Vec::new(),
            slots: Arc::new(Mutex::new(Vec::new())),
            on_fire: Arc::new(Mutex::new(None)),
            last_health: Arc::new(Mutex::new(HotkeyHealthReport::default())),
            paused: false,
            owns_kga_slots: false,
            active_label: "Alt+Space".into(),
            conflict_msg: None,
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool, cfg: &TomoyoConfig, plugin_host: &PluginHost) -> bool {
        self.paused = paused;
        if paused {
            self.unregister_all();
            self.active_label = cfg.lang().pick("快捷键已停用", "Hotkeys Paused").into();
            self.conflict_msg = None;
            save_hotkey_health(&HotkeyHealthReport {
                main: HotkeySlotStatus {
                    spec: cfg.hotkey.trim().into(),
                    disabled: true,
                    reason: cfg.lang().pick("已通过托盘一键停用全局快捷键", "Paused via system tray").into(),
                    ..Default::default()
                },
                clipboard: HotkeySlotStatus { disabled: true, ..Default::default() },
                translate: HotkeySlotStatus { disabled: true, ..Default::default() },
                capture: HotkeySlotStatus { disabled: true, ..Default::default() },
                pin: HotkeySlotStatus { disabled: true, ..Default::default() },
                ocr: HotkeySlotStatus { disabled: true, ..Default::default() },
                record: HotkeySlotStatus { disabled: true, ..Default::default() },
                updated_at_unix: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            });
            tracing::info!("Linux global hotkeys paused via tray toggle");
        } else {
            self.register_all(cfg, plugin_host);
            tracing::info!("Linux global hotkeys resumed via tray toggle");
        }
        self.paused
    }

    pub fn set_handler<F: Fn(HotkeyAction) + Send + Sync + 'static>(&self, f: F) {
        if let Ok(mut g) = self.on_fire.lock() {
            *g = Some(Box::new(f));
        }
    }

    pub fn unregister_all(&mut self) {
        if !self.kga.is_null() && self.owns_kga_slots {
            for id in self.registered_ids.drain(..) {
                if let Ok(cs) = CString::new(id) {
                    unsafe { tomoyo_kga_unregister(self.kga, cs.as_ptr()); }
                }
            }
            unsafe { tomoyo_kga_purge_all(self.kga); }
        } else {
            self.registered_ids.clear();
        }
        self.owns_kga_slots = false;
        if let Ok(mut s) = self.slots.lock() {
            s.clear();
        }
    }

    pub fn register_all(&mut self, cfg: &TomoyoConfig, plugin_host: &PluginHost) {
        if self.paused {
            self.unregister_all();
            self.active_label = cfg.lang().pick("快捷键已停用", "Hotkeys Paused").into();
            self.conflict_msg = None;
            return;
        }
        if !self.kga.is_null() {
            for id in self.registered_ids.drain(..) {
                if let Ok(cs) = CString::new(id) {
                    unsafe { tomoyo_kga_unregister(self.kga, cs.as_ptr()); }
                }
            }
            // Clear stale slots left by any previous instance before claiming KGlobalAccel ownership.
            unsafe {
                tomoyo_kga_purge_all(self.kga);
                tomoyo_kga_unblock_global(self.kga);
            }
            self.owns_kga_slots = true;
        } else {
            self.registered_ids.clear();
        }
        if let Ok(mut s) = self.slots.lock() {
            s.clear();
        }
        let mut health = HotkeyHealthReport {
            updated_at_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            ..Default::default()
        };

        if self.kga.is_null() && self.portal.is_some() {
            self.register_via_portal(cfg, plugin_host, &mut health);
            save_hotkey_health(&health);
            return;
        }

        let ov = c_int::from(cfg.override_conflict_hotkeys);
        let mut active_spec = cfg.hotkey.trim().to_string();
        health.main = self.try_slot("main", "唤出/隐藏 Tomoyo", &cfg.hotkey, ov, HotkeyAction::ToggleMain);
        if !health.main.ok {
            for fb in ["Ctrl+Space", "Ctrl+Shift+Space", "F2"] {
                let st = self.try_slot("main", "唤出/隐藏 Tomoyo", fb, ov, HotkeyAction::ToggleMain);
                if st.ok {
                    active_spec = fb.to_string();
                    health.main = st;
                    health.main.spec = cfg.hotkey.trim().to_string();
                    break;
                }
            }
        }
        self.active_label = active_spec;
        self.conflict_msg = (!health.main.ok).then(|| health.main.reason.clone()).filter(|s| !s.is_empty());

        health.clipboard = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.translate = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.capture = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.pin = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.ocr = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.record = HotkeySlotStatus { disabled: true, ..Default::default() };

        let mut summary = vec![format!("main={}({})", self.active_label, if health.main.ok { "ok" } else { "fail" })];
        if cfg.super_panel.enabled && !cfg.super_panel.hotkey.trim().is_empty() {
            let st = self.try_slot("super_panel", "Tomoyo 超级面板", &cfg.super_panel.hotkey, ov, HotkeyAction::SuperPanel);
            summary.push(format!("super_panel={}({})", cfg.super_panel.hotkey.trim(), if st.ok { "ok" } else { "fail" }));
        }
        for (i, hk) in plugin_host.active_plugin_hotkeys(cfg).into_iter().take(16).enumerate() {
            let id = format!("plugin_{i}");
            let label = format!("{} ({})", hk.plugin_id, hk.hotkey_id);
            let spec = hk.spec.clone();
            let st = self.try_slot(&id, &label, &spec, ov, HotkeyAction::Plugin(hk));
            summary.push(format!("{label}={spec}({})", if st.ok { "ok" } else { "fail" }));
            if !st.ok && !st.reason.is_empty() {
                tracing::warn!(slot = %id, hotkey = %spec, reason = %st.reason, "Plugin global hotkey failed");
            }
        }
        for (i, fl) in load_or_init_flows()
            .into_iter()
            .filter(|f| f.enabled && !f.hotkey.trim().is_empty())
            .take(16)
            .enumerate()
        {
            let id = format!("flow_{i}");
            let label = format!("工作流: {}", fl.name);
            let st = self.try_slot(&id, &label, &fl.hotkey, ov, HotkeyAction::Flow(fl.id));
            summary.push(format!("{id}={}({})", fl.hotkey, if st.ok { "ok" } else { "fail" }));
        }
        tracing::info!("Linux KGlobalAccel shortcuts synced: {}", summary.join(", "));
        save_hotkey_health(&health);
    }

    fn try_slot(
        &mut self,
        id: &str,
        friendly: &str,
        spec: &str,
        allow_override: c_int,
        act: HotkeyAction,
    ) -> HotkeySlotStatus {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return HotkeySlotStatus { disabled: true, ..Default::default() };
        }
        let Some(qt) = spec_to_qt_key(trimmed) else {
            return HotkeySlotStatus {
                spec: trimmed.into(),
                ok: false,
                reason: format!("暂不支持的热键格式: {trimmed}"),
                ..Default::default()
            };
        };
        if self.kga.is_null() {
            return HotkeySlotStatus {
                spec: trimmed.into(),
                ok: false,
                reason: "当前会话未提供 KDE KGlobalAccel 服务".into(),
                ..Default::default()
            };
        }
        let cid = CString::new(id).unwrap();
        let cfr = CString::new(friendly).unwrap();
        let mut owner = [0 as c_char; 160];
        let rc = unsafe {
            tomoyo_kga_register(self.kga, cid.as_ptr(), cfr.as_ptr(), qt, allow_override, owner.as_mut_ptr(), owner.len())
        };
        if rc == 1 || rc == 2 {
            self.registered_ids.push(id.into());
            if let Ok(mut list) = self.slots.lock() {
                list.push((id.into(), act));
            }
            let own_str = unsafe { CStr::from_ptr(owner.as_ptr()) }.to_string_lossy().into_owned();
            let reason = if rc == 2 && !own_str.is_empty() {
                format!("已高优接管原 `{own_str}` 快捷键")
            } else {
                String::new()
            };
            HotkeySlotStatus {
                spec: trimmed.into(),
                active: trimmed.into(),
                ok: true,
                overridden: rc == 2,
                reason,
                ..Default::default()
            }
        } else {
            let own_str = unsafe { CStr::from_ptr(owner.as_ptr()) }.to_string_lossy().into_owned();
            let reason = if !own_str.is_empty() {
                format!("热键 `{trimmed}` 已被系统快捷键 `{own_str}` 占用")
            } else {
                format!("KDE KGlobalAccel 注册 `{trimmed}` 失败")
            };
            HotkeySlotStatus { spec: trimmed.into(), ok: false, reason, ..Default::default() }
        }
    }
    fn register_via_portal(
        &mut self,
        cfg: &TomoyoConfig,
        plugin_host: &PluginHost,
        health: &mut HotkeyHealthReport,
    ) {
        let main_spec = cfg.hotkey.trim().to_string();
        let (portal_slots, action_pairs) = build_portal_slots(cfg, plugin_host);
        let bound = self.portal.as_ref().is_some_and(|p| p.bind(&portal_slots));
        if bound {
            if let Ok(mut list) = self.slots.lock() {
                for (id, act) in action_pairs {
                    self.registered_ids.push(id.clone());
                    list.push((id, act));
                }
            }
            if let Some(p) = self.portal.as_ref() {
                let _ = p.request_list();
            }
        }
        let summary: Vec<String> = portal_slots
            .iter()
            .map(|slot| format!("{}={}({})", slot.id, slot.trigger, if bound { "ok" } else { "fail" }))
            .collect();
        self.active_label = main_spec.clone();
        health.main = HotkeySlotStatus {
            spec: main_spec.clone(),
            active: if bound { main_spec } else { String::new() },
            ok: bound,
            reason: if bound {
                "已通过 XDG Wayland GlobalShortcuts Portal 绑定".into()
            } else {
                "XDG GlobalShortcuts Portal 绑定失败".into()
            },
            ..Default::default()
        };
        health.clipboard = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.translate = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.capture = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.pin = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.ocr = HotkeySlotStatus { disabled: true, ..Default::default() };
        health.record = HotkeySlotStatus { disabled: true, ..Default::default() };
        if let Ok(mut lh) = self.last_health.lock() {
            *lh = health.clone();
        }
        self.conflict_msg = (!bound).then(|| health.main.reason.clone());
        tracing::info!("Linux XDG GlobalShortcuts Portal synced: {}", summary.join(", "));
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.unregister_all();
        self.portal = None;
        if !self.kga.is_null() {
            unsafe { tomoyo_kga_destroy(self.kga); }
            self.kga = std::ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HotkeyManager;

    #[test]
    fn inactive_hotkey_manager_never_claims_kga_or_purges_on_drop() {
        let mut mgr = HotkeyManager::new_inactive();
        assert!(mgr.kga.is_null());
        assert!(mgr.portal.is_none());
        assert!(!mgr.owns_kga_slots);
        mgr.unregister_all();
        assert!(!mgr.owns_kga_slots);
        drop(mgr);
    }
}
