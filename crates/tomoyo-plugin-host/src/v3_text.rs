//! Protocol v3 Super Panel action resolvers (`[[text_actions]]` & `[[file_actions]]`).

use crate::host::PluginHost;
use crate::runtime::RegisteredPlugin;
use crate::v3_support::collect_file_actions_for;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tomoyo_protocol::{ActionKind, ActionSpec};

/// Collects zero-I/O text/selection actions declared via `[[text_actions]]` by enabled plugins.
pub(crate) fn collect_text_actions_for(
    plugins: &HashMap<String, RegisteredPlugin>,
    disabled_ids: &HashSet<String>,
    selected_text: &str,
    kind_lower: &str,
) -> Vec<ActionSpec> {
    let mut sorted: Vec<&RegisteredPlugin> = plugins.values().collect();
    sorted.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    let mut out = Vec::new();
    for plug in sorted {
        if disabled_ids.contains(&plug.manifest.id.to_ascii_lowercase()) {
            continue;
        }
        for decl in &plug.manifest.text_actions {
            if decl.id.trim().is_empty() || decl.label.trim().is_empty() {
                continue;
            }
            let matched = decl.match_kinds.is_empty()
                || decl.match_kinds.iter().any(|raw| {
                    let clean = raw.trim().to_ascii_lowercase();
                    clean == "any" || clean == "*" || clean == kind_lower
                });
            if !matched {
                continue;
            }
            let kind = if !decl.query_template.trim().is_empty() {
                ActionKind::FillQuery {
                    query: decl.query_template.replace("{text}", selected_text),
                }
            } else {
                ActionKind::PluginExecute {
                    plugin_id: plug.manifest.id.clone(),
                    action_id: decl.id.clone(),
                    payload: serde_json::json!({
                        "text": selected_text,
                        "kind": kind_lower,
                        "source": "super_panel",
                    }),
                }
            };
            out.push(ActionSpec {
                id: format!("plug:{}:{}", plug.manifest.id, decl.id),
                label: decl.label.clone(),
                shortcut: decl.shortcut.clone(),
                kind,
            });
            if out.len() >= 12 {
                return out;
            }
        }
    }
    out
}

impl PluginHost {
    /// Collects matching Protocol v3 `[[text_actions]]` for Super Panel text selections.
    pub fn text_actions_for(&self, selected_text: &str, kind: &str) -> Vec<ActionSpec> {
        let kind_lower = kind.trim().to_ascii_lowercase();
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        collect_text_actions_for(&st.plugins, &st.disabled_ids, selected_text, &kind_lower)
    }

    /// Collects matching Protocol v3 `[[file_actions]]` for a single file path in Super Panel.
    pub fn file_actions_for_path(&self, full_path: &str) -> Vec<ActionSpec> {
        let ext = Path::new(full_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let (lock, _) = &*self.state;
        let st = lock.lock().unwrap();
        collect_file_actions_for(&st.plugins, &st.disabled_ids, full_path, &ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomoyo_protocol::{PluginManifest, PluginTextActionDecl, ResourceLimits, RuntimeConfig, TriggerConfig};

    fn dummy_plugin(id: &str, text_actions: Vec<PluginTextActionDecl>) -> RegisteredPlugin {
        RegisteredPlugin {
            dir: std::path::PathBuf::from("."),
            manifest: PluginManifest {
                id: id.into(),
                name: id.into(),
                version: "3.0.0".into(),
                description: String::new(),
                author: String::new(),
                license: String::new(),
                homepage: None,
                icon: None,
                category: None,
                display_mode: Default::default(),
                tags: vec![],
                runtime: RuntimeConfig::StdioJsonrpc { command: "noop".into(), args: vec![], env: Default::default() },
                triggers: TriggerConfig::default(),
                permissions: vec![],
                resources: ResourceLimits::default(),
                hotkeys: vec![],
                events: vec![],
                settings: vec![],
                ui: None,
                tray_actions: vec![],
                file_actions: vec![],
                text_actions,
            },
            live: None,
        }
    }

    #[test]
    fn collects_matching_text_actions_respecting_kind_and_disabled() {
        let mut map = HashMap::new();
        map.insert(
            "dev.a".into(),
            dummy_plugin(
                "dev.a",
                vec![
                    PluginTextActionDecl {
                        id: "fmt_json".into(),
                        label: "JSON 格式化".into(),
                        match_kinds: vec!["json".into()],
                        shortcut: None,
                        query_template: "json {text}".into(),
                    },
                    PluginTextActionDecl {
                        id: "save_note".into(),
                        label: "存为速记".into(),
                        match_kinds: vec!["any".into()],
                        shortcut: None,
                        query_template: String::new(),
                    },
                ],
            ),
        );
        map.insert(
            "dev.off".into(),
            dummy_plugin(
                "dev.off",
                vec![PluginTextActionDecl {
                    id: "skip".into(),
                    label: "Disabled".into(),
                    match_kinds: vec![],
                    shortcut: None,
                    query_template: String::new(),
                }],
            ),
        );
        let disabled: HashSet<String> = ["dev.off".into()].into_iter().collect();

        let plain = collect_text_actions_for(&map, &disabled, "hello", "text");
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].label, "存为速记");

        let json_acts = collect_text_actions_for(&map, &disabled, "{\"a\":1}", "json");
        assert_eq!(json_acts.len(), 2);
        assert!(matches!(&json_acts[0].kind, ActionKind::FillQuery { query } if query == "json {\"a\":1}"));
    }
}
