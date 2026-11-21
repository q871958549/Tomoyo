//! Desensitized single-file config backup & validated restore (strips machine-bound DPAPI secrets).

use serde::{Deserialize, Serialize};
use tomoyo_core::{data_dir, TomoyoConfig, CURRENT_VERSION};

#[derive(Debug, Serialize, Deserialize)]
struct TomoyoBackupBundle {
    #[serde(default = "default_schema")]
    schema_version: u32,
    #[serde(default)]
    app_version: String,
    config: TomoyoConfig,
    #[serde(default)]
    snippets_toml: Option<String>,
}

fn default_schema() -> u32 {
    1
}

/// Builds a desensitized JSON backup (never includes `secrets.json` DPAPI keys).
pub fn export_desensitized_backup_json() -> String {
    let mut cfg = TomoyoConfig::load_or_create();
    let _ = cfg.sanitize();
    let snippets_path = data_dir().join("snippets.toml");
    let snippets_toml = std::fs::metadata(&snippets_path)
        .ok()
        .filter(|m| m.len() <= 262_144)
        .and_then(|_| std::fs::read_to_string(&snippets_path).ok());
    let bundle = TomoyoBackupBundle {
        schema_version: 1,
        app_version: CURRENT_VERSION.to_string(),
        config: cfg,
        snippets_toml,
    };
    let json = serde_json::to_string_pretty(&bundle).unwrap_or_else(|_| "{}".into());
    serde_json::json!({
        "event": "config_backup_exported",
        "ok": true,
        "json": json,
    })
    .to_string()
}

/// Validates, sanitizes, and restores a desensitized JSON backup.
pub fn import_desensitized_backup_json(raw: &str) -> (String, bool) {
    if raw.len() > 512 * 1024 {
        return (
            serde_json::json!({ "event": "config_backup_imported", "ok": false, "message": "备份文件超过 512 KB 上限" }).to_string(),
            false,
        );
    }
    let mut bundle: TomoyoBackupBundle = match serde_json::from_str(raw) {
        Ok(b) => b,
        Err(e) => {
            return (
                serde_json::json!({ "event": "config_backup_imported", "ok": false, "message": format!("备份 JSON 格式无效：{e}") }).to_string(),
                false,
            );
        }
    };
    let notes = bundle.config.sanitize();
    if let Err(e) = bundle.config.save() {
        return (
            serde_json::json!({ "event": "config_backup_imported", "ok": false, "message": format!("保存配置失败：{e}") }).to_string(),
            false,
        );
    }
    if let Some(snip) = bundle.snippets_toml.as_deref() {
        if snip.len() <= 262_144 {
            let _ = std::fs::create_dir_all(data_dir());
            let _ = std::fs::write(data_dir().join("snippets.toml"), snip);
        }
    }
    let note_tail = if notes.is_empty() { String::new() } else { format!("（已自动修正 {} 项）", notes.len()) };
    (
        serde_json::json!({
            "event": "config_backup_imported",
            "ok": true,
            "config": bundle.config,
            "message": format!("已校验并恢复脱敏配置备份{note_tail}（API Key 需按新机器重新录入）"),
        })
        .to_string(),
        true,
    )
}

pub fn clear_sync_dir_reply() -> (String, bool) {
    match tomoyo_core::set_sync_dir(None) {
        Ok((_, msg)) => (
            serde_json::json!({
                "event": "sync_dir_updated",
                "ok": true,
                "cancelled": false,
                "sync_dir": "",
                "config_path": tomoyo_core::TomoyoConfig::config_path().to_string_lossy(),
                "config": tomoyo_core::TomoyoConfig::load_or_create(),
                "message": msg
            })
            .to_string(),
            true,
        ),
        Err(e) => (
            serde_json::json!({ "event": "sync_dir_updated", "ok": false, "message": e }).to_string(),
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_export_never_leaks_secrets_and_sanitizes_on_import() {
        let exported = export_desensitized_backup_json();
        let v: serde_json::Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(v["ok"], true);
        let raw_json = v["json"].as_str().unwrap();
        assert!(!raw_json.contains("deepl_api_key"));
        assert!(!raw_json.contains("openai_api_key"));
        let parsed: TomoyoBackupBundle = serde_json::from_str(raw_json).unwrap();
        assert_eq!(parsed.schema_version, 1);
    }
}
