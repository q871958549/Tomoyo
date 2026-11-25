//! Linux global hotkey orchestration via KDE `org.kde.kglobalaccel` D-Bus + health tracking.

use crate::linux::ipc::schedule_on_ui;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::{Arc, Mutex};
use tomoyo_core::{
    load_or_init_flows, parse_hotkey, save_hotkey_health, HotkeyHealthReport, HotkeySlotStatus,
    TomoyoConfig,
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
    Plugin(ActivePluginHotkey),
    Flow(String),
}

type FireHandler = Arc<Mutex<Option<Box<dyn Fn(HotkeyAction) + Send + Sync>>>>;

pub struct HotkeyManager {
    kga: *mut c_void,
    registered_ids: Vec<String>,
    slots: Arc<Mutex<Vec<(String, HotkeyAction)>>>,
    on_fire: FireHandler,
    paused: bool,
    pub active_label: String,
    pub conflict_msg: Option<String>,
}

unsafe extern "C" fn on_kga_fired(ctx: *mut c_void, act_ptr: *const c_char) {
    if ctx.is_null() || act_ptr.is_null() {
        return;
    }
    let act_id = unsafe { CStr::from_ptr(act_ptr) }.to_string_lossy().into_owned();
    let state = unsafe { &*(ctx as *const (Arc<Mutex<Vec<(String, HotkeyAction)>>>, FireHandler)) };
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

impl HotkeyManager {
    pub fn new() -> Self {
        let slots = Arc::new(Mutex::new(Vec::new()));
        let on_fire: FireHandler = Arc::new(Mutex::new(None));
        let box_ctx = Box::new((Arc::clone(&slots), Arc::clone(&on_fire)));
        let ctx_ptr = Box::into_raw(box_ctx);
        let kga = unsafe { tomoyo_kga_connect(on_kga_fired, ctx_ptr.cast()) };
        Self {
            kga,
            registered_ids: Vec::new(),
            slots,
            on_fire,
            paused: false,
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
        if !self.kga.is_null() {
            for id in self.registered_ids.drain(..) {
                if let Ok(cs) = CString::new(id) {
                    unsafe { tomoyo_kga_unregister(self.kga, cs.as_ptr()); }
                }
            }
            unsafe { tomoyo_kga_purge_all(self.kga); }
        } else {
            self.registered_ids.clear();
        }
        if let Ok(mut s) = self.slots.lock() {
            s.clear();
        }
    }

    pub fn register_all(&mut self, cfg: &TomoyoConfig, plugin_host: &PluginHost) {
        self.unregister_all();
        if self.paused {
            self.active_label = cfg.lang().pick("快捷键已停用", "Hotkeys Paused").into();
            self.conflict_msg = None;
            return;
        }
        if !self.kga.is_null() {
            unsafe { tomoyo_kga_unblock_global(self.kga); }
        }
        let mut health = HotkeyHealthReport {
            updated_at_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            ..Default::default()
        };

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
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.unregister_all();
        if !self.kga.is_null() {
            unsafe { tomoyo_kga_destroy(self.kga); }
            self.kga = std::ptr::null_mut();
        }
    }
}

fn spec_to_qt_key(spec: &str) -> Option<i32> {
    let p = parse_hotkey(spec)?;
    let mut qt: i32 = 0;
    if (p.modifiers & 0x0004) != 0 { qt |= 0x0200_0000; }
    if (p.modifiers & 0x0002) != 0 { qt |= 0x0400_0000; }
    if (p.modifiers & 0x0001) != 0 { qt |= 0x0800_0000; }
    if (p.modifiers & 0x0008) != 0 { qt |= 0x1000_0000; }
    let key: i32 = match p.vk {
        0x20 => 0x20,
        0x30..=0x39 | 0x41..=0x5A => p.vk as i32,
        0x09 => 0x0100_0001,
        0x0D => 0x0100_0004,
        0x2D => 0x0100_0006,
        0x2E => 0x0100_0007,
        0x2C => 0x0100_0009,
        0x24 => 0x0100_0010,
        0x23 => 0x0100_0011,
        0x25 => 0x0100_0012,
        0x26 => 0x0100_0013,
        0x27 => 0x0100_0014,
        0x28 => 0x0100_0015,
        0x21 => 0x0100_0016,
        0x22 => 0x0100_0017,
        0x70..=0x7B => 0x0100_0030 + (p.vk as i32 - 0x70),
        0xC0 => 0x60,
        0xBD => 0x2D,
        0xBB => 0x3D,
        0xDB => 0x5B,
        0xDD => 0x5D,
        0xDC => 0x5C,
        0xBA => 0x3B,
        0xDE => 0x27,
        0xBC => 0x2C,
        0xBE => 0x2E,
        0xBF => 0x2F,
        _ => return None,
    };
    Some(qt | key)
}
