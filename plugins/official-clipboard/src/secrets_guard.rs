//! In-memory-only ephemeral vault & timed auto-clear for detected API keys, tokens & private keys.
//!
//! Sensitive clipboard entries NEVER touch SQLite (`clipboard.db`) and automatically expire
//! after `secret_ttl_secs`, displaying only masked text in launcher previews.

use crate::filter::ParsedClipQuery;
use crate::PLUGIN_ID;
use serde_json::{json, Value};
use tomoyo_sdk::{ActionKind, ActionSpec, ExecuteResponse, IconSpec, PreviewSpec, ResultCategory, ResultItem};

const MAX_EPHEMERAL_SECRETS: usize = 8;

#[derive(Debug, Clone)]
pub struct EphemeralSecret {
    pub key: String,
    pub raw: String,
    pub label: &'static str,
    pub source_app: String,
    pub expires_at: i64,
}

#[derive(Default)]
pub struct EphemeralSecretStore {
    entries: Vec<EphemeralSecret>,
}

/// Detects high-risk credentials (PEM keys, GitHub/OpenAI/Stripe/AWS/Slack tokens, JWT, Bearer, k=v secrets).
pub fn detect_sensitive_secret(raw: &str) -> Option<&'static str> {
    let t = raw.trim();
    if t.len() < 16 || t.len() > 8192 {
        return None;
    }
    if t.contains("-----BEGIN ") && t.contains("PRIVATE KEY-----") {
        return Some("PEM 私钥");
    }
    if t.contains('\n') || t.contains('\r') {
        return None;
    }
    if !t.chars().any(char::is_whitespace) {
        if (t.starts_with("ghp_") || t.starts_with("gho_") || t.starts_with("ghu_") || t.starts_with("ghs_") || t.starts_with("github_pat_")) && t.len() >= 20 {
            return Some("GitHub Token");
        }
        if (t.starts_with("sk-") || t.starts_with("sk_live_") || t.starts_with("rk_live_")) && t.len() >= 20 {
            return Some("API Key");
        }
        if (t.starts_with("AKIA") || t.starts_with("ASIA")) && t.len() == 20 && t.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Some("AWS Access Key");
        }
        if (t.starts_with("xoxb-") || t.starts_with("xoxp-") || t.starts_with("xoxa-")) && t.len() >= 20 {
            return Some("Slack Token");
        }
        if t.starts_with("eyJ") && t.len() >= 40 && t.split('.').count() == 3 {
            return Some("JWT 令牌");
        }
    }
    if let Some(tok) = t.strip_prefix("Bearer ").or_else(|| t.strip_prefix("bearer "))
        && tok.trim().len() >= 20 && !tok.trim().contains(' ')
    {
        return Some("Bearer Token");
    }
    let low = t.to_ascii_lowercase();
    for pref in ["password=", "passwd=", "api_key=", "secret_key=", "access_token="] {
        if let Some(idx) = low.find(pref) {
            let val = t[idx + pref.len()..].trim();
            if val.len() >= 8 && !val.contains(' ') {
                return Some("敏感密钥键值");
            }
        }
    }
    None
}

pub fn mask_secret_text(raw: &str) -> String {
    let t = raw.trim();
    if t.contains("-----BEGIN ") && t.contains("PRIVATE KEY-----") {
        return "[PEM 私钥已脱敏保护 · 限时内存暂存，不落盘]".into();
    }
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= 10 {
        return "••••••••".into();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}••••{tail} ({} chars)", chars.len())
}

impl EphemeralSecretStore {
    pub fn push(&mut self, raw: String, label: &'static str, source_app: &str, ttl_secs: u64, now_unix: i64) {
        self.purge_expired(now_unix);
        self.entries.retain(|e| e.raw != raw);
        let ttl = ttl_secs.clamp(15, 600) as i64;
        self.entries.insert(0, EphemeralSecret {
            key: format!("secret:{now_unix}:{}", raw.len()),
            raw,
            label,
            source_app: source_app.to_string(),
            expires_at: now_unix + ttl,
        });
        self.entries.truncate(MAX_EPHEMERAL_SECRETS);
    }

    pub fn purge_expired(&mut self, now_unix: i64) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.expires_at > now_unix);
        before - self.entries.len()
    }

    pub fn active_len(&mut self, now_unix: i64) -> usize {
        self.purge_expired(now_unix);
        self.entries.len()
    }

    pub fn active_items(&mut self, parsed: &ParsedClipQuery, now_unix: i64) -> Vec<ResultItem> {
        self.purge_expired(now_unix);
        if parsed.pinned_only || matches!(parsed.subtype, Some(crate::filter::ClipboardSubtype::Image | crate::filter::ClipboardSubtype::Files)) {
            return Vec::new();
        }
        self.entries.iter().filter(|e| {
            let hay = format!("{} {} secret 隐私 凭据", e.label, e.source_app).to_lowercase();
            parsed.tokens.iter().all(|tok| hay.contains(tok.as_str()))
        }).enumerate().map(|(i, e)| {
            let rem = (e.expires_at - now_unix).max(1);
            let masked = mask_secret_text(&e.raw);
            let src = if e.source_app.is_empty() { String::new() } else { format!(" · {}", e.source_app) };
            ResultItem {
                id: format!("cb:sec:{i}:{}", e.expires_at),
                provider_id: PLUGIN_ID.into(),
                title: format!("🔒 [{}] {masked}", e.label),
                subtitle: format!("仅内存限时保留 {rem}s{src}（未写入 SQLite）· Enter 粘贴 · Ctrl+D 立即销毁"),
                category: ResultCategory::Clipboard,
                score: 9_600 - (i as i64 * 15),
                icon: IconSpec::Monogram("KEY".into()),
                badge: Some(format!("{rem}s")),
                copy_text: None,
                actions: vec![
                    ActionSpec { id: "paste_sec".into(), label: "粘贴敏感文本到原窗口".into(), shortcut: Some("Enter".into()), kind: ActionKind::CopyText { text: e.raw.clone(), paste_immediately: true } },
                    ActionSpec { id: "copy_sec".into(), label: "仅复制到剪贴板".into(), shortcut: Some("Ctrl+Enter".into()), kind: ActionKind::CopyText { text: e.raw.clone(), paste_immediately: false } },
                    ActionSpec { id: "del_sec".into(), label: "立即从内存销毁此凭据".into(), shortcut: Some("Ctrl+D".into()), kind: ActionKind::PluginExecute { plugin_id: PLUGIN_ID.into(), action_id: "delete_secret".into(), payload: json!({ "key": e.key }) } },
                    ActionSpec { id: "purge_secs".into(), label: "清空全部内存敏感凭据 (cb :purge)".into(), shortcut: None, kind: ActionKind::PluginExecute { plugin_id: PLUGIN_ID.into(), action_id: "purge_secrets".into(), payload: json!({}) } },
                ],
                preview: Some(PreviewSpec::Text {
                    title: format!("隐私脱敏保护 · {}", e.label),
                    body: format!("脱敏摘要：{masked}\n剩余销毁时间：{rem} 秒\n存储策略：纯内存临时暂存，绝不写入 clipboard.db"),
                }),
            }
        }).collect()
    }

    pub fn handle_action(&mut self, action_id: &str, payload: &Value, now_unix: i64) -> Option<ExecuteResponse> {
        match action_id {
            "delete_secret" => {
                let k = payload.get("key").and_then(Value::as_str).unwrap_or("");
                self.entries.retain(|e| e.key != k);
                Some(ExecuteResponse { hide_window: false, refresh_query: true, toast: Some("✓ 已从内存销毁敏感凭据".into()), ..Default::default() })
            }
            "purge_secrets" => {
                self.purge_expired(now_unix);
                let n = self.entries.len();
                self.entries.clear();
                Some(ExecuteResponse { hide_window: false, refresh_query: true, toast: Some(format!("✓ 已立即销毁 {n} 条内存敏感凭据")), ..Default::default() })
            }
            _ => None,
        }
    }
}

pub fn build_purge_command_item(active_count: usize) -> ResultItem {
    ResultItem {
        id: "cb:cmd:purge".into(),
        provider_id: PLUGIN_ID.into(),
        title: format!("cb :purge — 立即清空全部内存敏感凭据（当前 {active_count} 条）"),
        subtitle: "API Key / Token / PEM 私钥仅内存限时暂存，此指令可提前彻底抹除".into(),
        category: ResultCategory::Clipboard,
        score: 9_880,
        icon: IconSpec::Monogram("KEY".into()),
        badge: Some(format!("{active_count} KEY")),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "purge_secrets".into(),
            label: "立即抹除全部敏感凭据".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute { plugin_id: PLUGIN_ID.into(), action_id: "purge_secrets".into(), payload: json!({}) },
        }],
        preview: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_masks_and_expires_sensitive_tokens_without_touching_sqlite() {
        assert_eq!(detect_sensitive_secret("ghp_1234567890abcdefghij"), Some("GitHub Token"));
        assert_eq!(detect_sensitive_secret("sk-proj-abcdef1234567890"), Some("API Key"));
        assert_eq!(detect_sensitive_secret("AKIAIOSFODNN7EXAMPLE"), Some("AWS Access Key"));
        assert_eq!(detect_sensitive_secret("-----BEGIN OPENSSH PRIVATE KEY-----\nAAA\n-----END OPENSSH PRIVATE KEY-----"), Some("PEM 私钥"));
        assert!(detect_sensitive_secret("https://github.com/q871958549/Tomoyo").is_none());
        assert!(detect_sensitive_secret("let x = 42;").is_none());

        let masked = mask_secret_text("sk-proj-abcdef1234567890");
        assert!(masked.starts_with("sk-p••••7890") && !masked.contains("abcdef"));

        let mut store = EphemeralSecretStore::default();
        store.push("sk-proj-abcdef1234567890".into(), "API Key", "code", 30, 1000);
        let parsed = ParsedClipQuery::parse("");
        assert_eq!(store.active_items(&parsed, 1010).len(), 1);
        assert_eq!(store.active_items(&parsed, 1031).len(), 0);
    }
}
