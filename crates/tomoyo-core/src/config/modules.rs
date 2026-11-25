//! Modular official plugin identifiers & sanitization for extensible search tabs and disabled plugins.

use super::TomoyoConfig;
use std::collections::{BTreeMap, HashSet};
use tomoyo_protocol::CustomFilterTab;

pub const PLUGIN_CLIPBOARD: &str = "dev.tomoyo.clipboard";
pub const PLUGIN_TRANSLATE: &str = "dev.tomoyo.translate";
pub const PLUGIN_CAPTURE: &str = "dev.tomoyo.capture";
pub const PLUGIN_OCR: &str = "dev.tomoyo.ocr";
pub const PLUGIN_NOTES: &str = "dev.tomoyo.notes";
pub const PLUGIN_MD_EDITOR: &str = "dev.tomoyo.markdown-editor";

pub const MAX_CUSTOM_FILTER_TABS: usize = 32;
pub const MAX_DISABLED_PLUGINS: usize = 512;
pub const MAX_PLUGIN_DISPLAY_MODES: usize = 512;

const BUILTIN_FILTER_IDS: &[&str] = &[
    "app", "folder", "pdf", "excel", "word", "ppt", "image", "archive",
];

pub fn sanitize_filters_and_plugins(cfg: &mut TomoyoConfig) {
    // 1. Disabled plugins (deduped lowercase IDs, bounded)
    let mut seen_dis = HashSet::new();
    let mut clean_dis = Vec::new();
    for raw in std::mem::take(&mut cfg.disabled_plugins) {
        let id = raw.trim().to_ascii_lowercase();
        let valid = !id.is_empty()
            && id.len() <= 96
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
        if valid && seen_dis.insert(id.clone()) && clean_dis.len() < MAX_DISABLED_PLUGINS {
            clean_dis.push(id);
        }
    }
    cfg.disabled_plugins = clean_dis;

    // 1b. Hotkey avoidance process list (deduped normalized `.exe` names)
    let mut seen_hk_ex = HashSet::new();
    let mut clean_hk_ex = Vec::new();
    for raw in std::mem::take(&mut cfg.hotkey_excluded_apps) {
        if let Some(exe) = super::normalize_exe_name(&raw)
            && seen_hk_ex.insert(exe.clone()) && clean_hk_ex.len() < 64 {
                clean_hk_ex.push(exe);
            }
    }
    cfg.hotkey_excluded_apps = clean_hk_ex;

    // 2. Hidden built-in filter tabs (never allow hiding `all`) & bar layout mode
    if !matches!(cfg.filter_bar_layout.as_str(), "wrap" | "single") {
        cfg.filter_bar_layout = "wrap".into();
    }
    let mut seen_hid = HashSet::new();
    cfg.hidden_builtin_filters = std::mem::take(&mut cfg.hidden_builtin_filters)
        .into_iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| BUILTIN_FILTER_IDS.contains(&s.as_str()) && seen_hid.insert(s.clone()))
        .collect();

    // 3. User-extensible custom filter tabs
    let mut seen_ids = HashSet::new();
    let mut clean_tabs = Vec::new();
    for mut tab in std::mem::take(&mut cfg.custom_filters) {
        let label: String = tab
            .label
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(16)
            .collect();
        if label.is_empty() {
            continue;
        }
        let mut id: String = tab
            .id
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .take(24)
            .collect();
        if id.is_empty() {
            id = format!("tab_{}", clean_tabs.len() + 1);
        }
        if !seen_ids.insert(id.clone()) {
            continue;
        }
        tab.extensions = tab
            .extensions
            .into_iter()
            .flat_map(|s| {
                s.split([';', ',', ' '])
                    .map(|p| p.trim().trim_start_matches('.').to_ascii_lowercase())
                    .collect::<Vec<_>>()
            })
            .filter(|e| !e.is_empty() && e.len() <= 16 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
            .take(24)
            .collect();
        tab.everything_expr = tab
            .everything_expr
            .chars()
            .filter(|c| !c.is_control() && *c != '"' && *c != '\'')
            .take(120)
            .collect::<String>()
            .trim()
            .to_string();
        if tab.extensions.is_empty() && tab.everything_expr.is_empty() {
            continue;
        }
        let tag_clean: String = tab
            .tag
            .trim()
            .trim_start_matches('@')
            .chars()
            .filter(|c| !c.is_control() && !c.is_whitespace())
            .take(18)
            .collect();
        let tag = if tag_clean.is_empty() {
            format!("@{id}")
        } else {
            format!("@{tag_clean}")
        };
        if clean_tabs.len() < MAX_CUSTOM_FILTER_TABS {
            clean_tabs.push(CustomFilterTab {
                id,
                label,
                tag,
                extensions: tab.extensions,
                everything_expr: tab.everything_expr,
                enabled: tab.enabled,
            });
        }
    }
    cfg.custom_filters = clean_tabs;

    // 4. Protocol v2: plugin_hotkeys & plugin_settings (bounded & TOML-safe primitives only)
    let mut clean_phk = BTreeMap::new();
    for (k, v) in std::mem::take(&mut cfg.plugin_hotkeys) {
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim().to_string();
        if !key.is_empty() && key.len() <= 128 && key.contains(':') && clean_phk.len() < 32
            && (val.is_empty() || super::parse_hotkey(&val).is_some()) {
                clean_phk.insert(key, val);
            }
    }
    cfg.plugin_hotkeys = clean_phk;

    let mut clean_pset = BTreeMap::new();
    for (pid, map) in std::mem::take(&mut cfg.plugin_settings) {
        let pid_clean = pid.trim().to_ascii_lowercase();
        if pid_clean.is_empty() || pid_clean.len() > 96 || clean_pset.len() >= 256 {
            continue;
        }
        let mut inner = BTreeMap::new();
        for (sk, sv) in map {
            let sk_clean = sk.trim().to_string();
            if sk_clean.is_empty() || sk_clean.len() > 64 || inner.len() >= 24 {
                continue;
            }
            let safe_val = match sv {
                serde_json::Value::Bool(b) => Some(serde_json::Value::Bool(b)),
                serde_json::Value::Number(n) => Some(serde_json::Value::Number(n)),
                serde_json::Value::String(s) => Some(serde_json::Value::String(
                    s.chars().filter(|c| !c.is_control()).take(260).collect(),
                )),
                _ => None,
            };
            if let Some(v) = safe_val {
                inner.insert(sk_clean, v);
            }
        }
        if !inner.is_empty() {
            clean_pset.insert(pid_clean, inner);
        }
    }
    cfg.plugin_settings = clean_pset;

    let mut clean_modes = BTreeMap::new();
    for (pid, mode) in std::mem::take(&mut cfg.plugin_display_modes) {
        let pid_clean = pid.trim().to_ascii_lowercase();
        let mode_clean = mode.trim().to_ascii_lowercase();
        if !pid_clean.is_empty()
            && pid_clean.len() <= 96
            && matches!(mode_clean.as_str(), "inline" | "window")
            && clean_modes.len() < MAX_PLUGIN_DISPLAY_MODES
        {
            clean_modes.insert(pid_clean, mode_clean);
        }
    }
    cfg.plugin_display_modes = clean_modes;
}

/// Losslessly migrates customized legacy `[clipboard]` / `[translate]` / `[capture]`
/// hotkeys and settings into Protocol v3 `plugin_hotkeys` / `plugin_settings` when absent.
/// Never overwrites an explicit entry already stored in `plugin_hotkeys` / `plugin_settings`
/// (including `""` for intentionally unbound hotkeys). Returns `true` if any key was added.
pub fn migrate_legacy_plugin_config(cfg: &mut TomoyoConfig) -> bool {
    let def = TomoyoConfig::default();
    let mut changed = false;

    let mut migrate_hk = |slot: &str, legacy: &str, default_val: &str| {
        if cfg.plugin_hotkeys.contains_key(slot) {
            return;
        }
        let val = legacy.trim();
        if !val.eq_ignore_ascii_case(default_val)
            && (val.is_empty() || super::parse_hotkey(val).is_some())
        {
            cfg.plugin_hotkeys.insert(slot.to_string(), val.to_string());
            changed = true;
        }
    };

    migrate_hk("dev.tomoyo.clipboard:open_cb", &cfg.clipboard.hotkey, &def.clipboard.hotkey);
    migrate_hk("dev.tomoyo.translate:selection_translate", &cfg.translate.hotkey, &def.translate.hotkey);
    migrate_hk("dev.tomoyo.capture:region", &cfg.capture.hotkey, &def.capture.hotkey);
    migrate_hk("dev.tomoyo.capture:pin_clipboard", &cfg.capture.pin_hotkey, &def.capture.pin_hotkey);
    migrate_hk("dev.tomoyo.capture:ocr", &cfg.capture.ocr_hotkey, &def.capture.ocr_hotkey);
    migrate_hk("dev.tomoyo.capture:record", &cfg.capture.record_hotkey, &def.capture.record_hotkey);

    let mut set_if_custom = |pid: &str, key: &str, val: serde_json::Value| {
        let bag = cfg.plugin_settings.entry(pid.to_string()).or_default();
        if !bag.contains_key(key) {
            bag.insert(key.to_string(), val);
            changed = true;
        }
    };

    if cfg.clipboard.capture_enabled != def.clipboard.capture_enabled {
        set_if_custom(PLUGIN_CLIPBOARD, "capture_enabled", serde_json::json!(cfg.clipboard.capture_enabled));
    }
    if cfg.clipboard.capture_images != def.clipboard.capture_images {
        set_if_custom(PLUGIN_CLIPBOARD, "capture_images", serde_json::json!(cfg.clipboard.capture_images));
    }
    if cfg.clipboard.persist_sqlite != def.clipboard.persist_sqlite {
        set_if_custom(PLUGIN_CLIPBOARD, "persist_sqlite", serde_json::json!(cfg.clipboard.persist_sqlite));
    }
    if cfg.clipboard.max_persisted_items != def.clipboard.max_persisted_items {
        set_if_custom(PLUGIN_CLIPBOARD, "max_persisted_items", serde_json::json!(cfg.clipboard.max_persisted_items.to_string()));
    }
    if cfg.clipboard.excluded_apps != def.clipboard.excluded_apps {
        set_if_custom(PLUGIN_CLIPBOARD, "excluded_apps", serde_json::json!(cfg.clipboard.excluded_apps.join(", ")));
    }

    if cfg.translate.target_lang != def.translate.target_lang {
        set_if_custom(PLUGIN_TRANSLATE, "target_lang", serde_json::json!(cfg.translate.target_lang));
    }
    if cfg.translate.engines != def.translate.engines {
        for eng in ["google", "mymemory", "deepl", "openai", "ollama"] {
            let has = cfg.translate.engines.iter().any(|e| e.eq_ignore_ascii_case(eng));
            let def_has = def.translate.engines.iter().any(|e| e.eq_ignore_ascii_case(eng));
            if has != def_has {
                set_if_custom(PLUGIN_TRANSLATE, &format!("engine_{eng}"), serde_json::json!(has));
            }
        }
    }
    if cfg.translate.openai_base_url != def.translate.openai_base_url {
        set_if_custom(PLUGIN_TRANSLATE, "openai_base_url", serde_json::json!(cfg.translate.openai_base_url));
    }
    if cfg.translate.openai_model != def.translate.openai_model {
        set_if_custom(PLUGIN_TRANSLATE, "openai_model", serde_json::json!(cfg.translate.openai_model));
    }
    if cfg.translate.ollama_url != def.translate.ollama_url {
        set_if_custom(PLUGIN_TRANSLATE, "ollama_url", serde_json::json!(cfg.translate.ollama_url));
    }
    if cfg.translate.ollama_model != def.translate.ollama_model {
        set_if_custom(PLUGIN_TRANSLATE, "ollama_model", serde_json::json!(cfg.translate.ollama_model));
    }

    if cfg.capture.save_dir != def.capture.save_dir {
        set_if_custom(PLUGIN_CAPTURE, "save_dir", serde_json::json!(cfg.capture.save_dir));
    }
    if cfg.capture.pin_opacity != def.capture.pin_opacity {
        set_if_custom(PLUGIN_CAPTURE, "pin_opacity", serde_json::json!(cfg.capture.pin_opacity));
    }
    if cfg.capture.copy_hex_lowercase != def.capture.copy_hex_lowercase {
        set_if_custom(PLUGIN_CAPTURE, "copy_hex_lowercase", serde_json::json!(cfg.capture.copy_hex_lowercase));
    }
    if cfg.capture.record_format != def.capture.record_format {
        set_if_custom(PLUGIN_CAPTURE, "record_format", serde_json::json!(cfg.capture.record_format));
    }
    if cfg.capture.record_fps != def.capture.record_fps {
        set_if_custom(PLUGIN_CAPTURE, "record_fps", serde_json::json!(cfg.capture.record_fps.to_string()));
    }
    if cfg.capture.record_audio != def.capture.record_audio {
        set_if_custom(PLUGIN_CAPTURE, "record_audio", serde_json::json!(cfg.capture.record_audio));
    }
    if cfg.capture.record_cursor != def.capture.record_cursor {
        set_if_custom(PLUGIN_CAPTURE, "record_cursor", serde_json::json!(cfg.capture.record_cursor));
    }
    let tool_pairs = [
        ("tool_rect", &cfg.capture.tool_hotkeys.rect, &def.capture.tool_hotkeys.rect),
        ("tool_ellipse", &cfg.capture.tool_hotkeys.ellipse, &def.capture.tool_hotkeys.ellipse),
        ("tool_arrow", &cfg.capture.tool_hotkeys.arrow, &def.capture.tool_hotkeys.arrow),
        ("tool_pen", &cfg.capture.tool_hotkeys.pen, &def.capture.tool_hotkeys.pen),
        ("tool_highlighter", &cfg.capture.tool_hotkeys.highlighter, &def.capture.tool_hotkeys.highlighter),
        ("tool_mosaic", &cfg.capture.tool_hotkeys.mosaic, &def.capture.tool_hotkeys.mosaic),
        ("tool_step", &cfg.capture.tool_hotkeys.step, &def.capture.tool_hotkeys.step),
        ("tool_text", &cfg.capture.tool_hotkeys.text, &def.capture.tool_hotkeys.text),
        ("tool_scroll", &cfg.capture.tool_hotkeys.scroll, &def.capture.tool_hotkeys.scroll),
        ("tool_pin", &cfg.capture.tool_hotkeys.pin, &def.capture.tool_hotkeys.pin),
        ("tool_ocr", &cfg.capture.tool_hotkeys.ocr, &def.capture.tool_hotkeys.ocr),
        ("tool_ocr_copy", &cfg.capture.tool_hotkeys.ocr_copy, &def.capture.tool_hotkeys.ocr_copy),
        ("tool_translate", &cfg.capture.tool_hotkeys.translate, &def.capture.tool_hotkeys.translate),
        ("tool_record", &cfg.capture.tool_hotkeys.record, &def.capture.tool_hotkeys.record),
    ];
    for (k, cur, dval) in tool_pairs {
        if !cur.eq_ignore_ascii_case(dval) {
            set_if_custom(PLUGIN_CAPTURE, k, serde_json::json!(cur));
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_hotkeys_and_settings_without_overwriting_v3_overrides() {
        let mut def = TomoyoConfig::default();
        assert!(!migrate_legacy_plugin_config(&mut def));
        assert!(def.plugin_hotkeys.is_empty());
        assert!(def.plugin_settings.is_empty());

        let mut legacy = TomoyoConfig::default();
        legacy.clipboard.hotkey = "Alt+C".into();
        legacy.clipboard.max_persisted_items = 1000;
        legacy.capture.hotkey = "F1".into();
        legacy.capture.pin_hotkey = String::new(); // explicitly unbound before upgrade
        legacy.capture.ocr_hotkey = "F4".into();
        legacy.capture.record_format = "gif".into();
        legacy.capture.tool_hotkeys.text = "T".into();
        legacy.translate.hotkey = "Ctrl+Shift+T".into();
        // Explicit v3 override (empty string = unbound) must NOT be clobbered by legacy capture.hotkey
        legacy.plugin_hotkeys.insert("dev.tomoyo.capture:region".into(), String::new());

        assert!(migrate_legacy_plugin_config(&mut legacy));
        assert_eq!(legacy.plugin_hotkeys.get("dev.tomoyo.clipboard:open_cb").map(String::as_str), Some("Alt+C"));
        assert_eq!(legacy.plugin_hotkeys.get("dev.tomoyo.capture:region").map(String::as_str), Some(""));
        assert_eq!(legacy.plugin_hotkeys.get("dev.tomoyo.capture:pin_clipboard").map(String::as_str), Some(""));
        assert_eq!(legacy.plugin_hotkeys.get("dev.tomoyo.capture:ocr").map(String::as_str), Some("F4"));
        assert_eq!(legacy.plugin_hotkeys.get("dev.tomoyo.translate:selection_translate").map(String::as_str), Some("Ctrl+Shift+T"));
        assert_eq!(legacy.plugin_settings[PLUGIN_CLIPBOARD]["max_persisted_items"], serde_json::json!("1000"));
        assert_eq!(legacy.plugin_settings[PLUGIN_CAPTURE]["record_format"], serde_json::json!("gif"));
        assert_eq!(legacy.plugin_settings[PLUGIN_CAPTURE]["tool_text"], serde_json::json!("T"));

        // Second run must be a strict no-op (idempotent)
        assert!(!migrate_legacy_plugin_config(&mut legacy));
    }
}
