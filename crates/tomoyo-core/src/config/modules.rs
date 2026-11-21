//! Modular official plugin identifiers & sanitization for extensible search tabs and disabled plugins.

use super::TomoyoConfig;
use std::collections::{BTreeMap, HashSet};
use tomoyo_protocol::CustomFilterTab;

pub const PLUGIN_CLIPBOARD: &str = "dev.tomoyo.clipboard";
pub const PLUGIN_TRANSLATE: &str = "dev.tomoyo.translate";
pub const PLUGIN_CAPTURE: &str = "dev.tomoyo.capture";
pub const PLUGIN_OCR: &str = "dev.tomoyo.ocr";

pub const MAX_CUSTOM_FILTER_TABS: usize = 12;
pub const MAX_DISABLED_PLUGINS: usize = 64;

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

    // 2. Hidden built-in filter tabs (never allow hiding `all`)
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
        if pid_clean.is_empty() || pid_clean.len() > 96 || clean_pset.len() >= 32 {
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
}
