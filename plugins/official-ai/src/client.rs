//! Local Ollama & OpenAI-Compatible chat completions client (zero heavy async runtime).

use crate::http_curl::http_post_bytes;
use crate::prompts::{build_messages, AiMode};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiConfig {
    pub provider: String,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_base_url: String,
    pub openai_model: String,
    pub openai_api_key: String,
    pub reply_lang: String,
    pub timeout_secs: u32,
}

impl AiConfig {
    pub fn from_maps(
        settings: &BTreeMap<String, Value>,
        secrets: &BTreeMap<String, String>,
    ) -> Self {
        let get_str = |k: &str, default: &str| -> String {
            settings
                .get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(default)
                .to_string()
        };
        let timeout_secs = settings
            .get("timeout_secs")
            .and_then(Value::as_i64)
            .map(|n| n.clamp(5, 60) as u32)
            .unwrap_or(20);
        let openai_api_key = secrets
            .get("openai_api_key")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        Self {
            provider: get_str("provider", "auto").to_ascii_lowercase(),
            ollama_url: get_str("ollama_url", "http://127.0.0.1:11434")
                .trim_end_matches('/')
                .to_string(),
            ollama_model: get_str("ollama_model", "qwen2.5:7b"),
            openai_base_url: get_str("openai_base_url", "https://api.openai.com/v1")
                .trim_end_matches('/')
                .to_string(),
            openai_model: get_str("openai_model", "gpt-4o-mini"),
            openai_api_key,
            reply_lang: get_str("reply_lang", "zh"),
            timeout_secs,
        }
    }

    pub fn backend_hint(&self) -> String {
        match self.resolve_backend() {
            Ok(ResolvedBackend::OpenAi { model, .. }) => format!("OpenAI ({model})"),
            Ok(ResolvedBackend::Ollama { model, .. }) => format!("Ollama ({model})"),
            Err(_) => "未配置密钥/Ollama".into(),
        }
    }

    pub fn resolve_backend(&self) -> Result<ResolvedBackend, String> {
        match self.provider.as_str() {
            "openai" => {
                if self.openai_api_key.is_empty() {
                    return Err("已选 OpenAI 兼容接口，但尚未在设置中心填写 openai_api_key（加密存储）".into());
                }
                Ok(ResolvedBackend::OpenAi {
                    base_url: self.openai_base_url.clone(),
                    model: self.openai_model.clone(),
                    api_key: self.openai_api_key.clone(),
                })
            }
            "ollama" => Ok(ResolvedBackend::Ollama {
                url: self.ollama_url.clone(),
                model: self.ollama_model.clone(),
            }),
            _ => {
                if !self.openai_api_key.is_empty() {
                    Ok(ResolvedBackend::OpenAi {
                        base_url: self.openai_base_url.clone(),
                        model: self.openai_model.clone(),
                        api_key: self.openai_api_key.clone(),
                    })
                } else {
                    Ok(ResolvedBackend::Ollama {
                        url: self.ollama_url.clone(),
                        model: self.ollama_model.clone(),
                    })
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedBackend {
    Ollama {
        url: String,
        model: String,
    },
    OpenAi {
        base_url: String,
        model: String,
        api_key: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiReply {
    pub text: String,
    pub model: String,
    pub elapsed_ms: u64,
}

pub fn build_ollama_chat_body(model: &str, system: &str, user: &str) -> Vec<u8> {
    json!({
        "model": model,
        "stream": false,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    })
    .to_string()
    .into_bytes()
}

pub fn build_openai_chat_body(model: &str, system: &str, user: &str) -> Vec<u8> {
    json!({
        "model": model,
        "temperature": 0.2,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    })
    .to_string()
    .into_bytes()
}

pub fn parse_ollama_chat_reply(raw: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(raw).map_err(|e| format!("Ollama 响应解析失败: {e}"))?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        return Err(format!("Ollama 错误: {err}"));
    }
    let content = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Ollama 返回空内容".to_string())?;
    Ok(content.to_string())
}

pub fn parse_openai_chat_reply(raw: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(raw).map_err(|e| format!("OpenAI 响应解析失败: {e}"))?;
    if let Some(err) = v.get("error").and_then(|e| e.get("message")).and_then(Value::as_str) {
        return Err(format!("接口错误: {err}"));
    }
    let content = v
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "AI 接口未返回有效文本".to_string())?;
    Ok(content.to_string())
}

pub fn invoke_llm(cfg: &AiConfig, mode: AiMode, input: &str) -> Result<AiReply, String> {
    let clean = input.trim();
    if clean.is_empty() {
        return Err("请输入问题或在剪贴板准备好文本后再试".into());
    }
    let bounded: String = clean.chars().take(12_000).collect();
    let (system, user) = build_messages(mode, &bounded, &cfg.reply_lang);
    let backend = cfg.resolve_backend()?;
    let timeout_ms = cfg.timeout_secs * 1000;
    let started = std::time::Instant::now();

    match backend {
        ResolvedBackend::Ollama { url, model } => {
            let endpoint = format!("{url}/api/chat");
            let body = build_ollama_chat_body(&model, &system, &user);
            let resp = http_post_bytes(
                &endpoint,
                &[("Content-Type", "application/json")],
                &body,
                timeout_ms,
                512 * 1024,
            )?;
            let raw = resp.text_lossy();
            if resp.status_code >= 400 {
                return Err(parse_ollama_chat_reply(&raw)
                    .err()
                    .unwrap_or_else(|| format!("Ollama HTTP {}", resp.status_code)));
            }
            let text = parse_ollama_chat_reply(&raw)?;
            Ok(AiReply {
                text,
                model: format!("Ollama/{model}"),
                elapsed_ms: started.elapsed().as_millis() as u64,
            })
        }
        ResolvedBackend::OpenAi {
            base_url,
            model,
            api_key,
        } => {
            let endpoint = format!("{base_url}/chat/completions");
            let auth = format!("Bearer {api_key}");
            let body = build_openai_chat_body(&model, &system, &user);
            let resp = http_post_bytes(
                &endpoint,
                &[
                    ("Content-Type", "application/json"),
                    ("Authorization", &auth),
                ],
                &body,
                timeout_ms,
                512 * 1024,
            )?;
            let raw = resp.text_lossy();
            if resp.status_code >= 400 {
                return Err(parse_openai_chat_reply(&raw)
                    .err()
                    .unwrap_or_else(|| format!("OpenAI HTTP {}", resp.status_code)));
            }
            let text = parse_openai_chat_reply(&raw)?;
            Ok(AiReply {
                text,
                model,
                elapsed_ms: started.elapsed().as_millis() as u64,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_auto_backend_locally_first() {
        let cfg = AiConfig::from_maps(&BTreeMap::new(), &BTreeMap::new());
        assert!(matches!(cfg.resolve_backend(), Ok(ResolvedBackend::Ollama { .. })));

        let mut secrets = BTreeMap::new();
        secrets.insert("openai_api_key".into(), "sk-test-123".into());
        let cfg2 = AiConfig::from_maps(&BTreeMap::new(), &secrets);
        assert!(matches!(cfg2.resolve_backend(), Ok(ResolvedBackend::OpenAi { .. })));

        let mut force_openai = BTreeMap::new();
        force_openai.insert("provider".into(), json!("openai"));
        let cfg3 = AiConfig::from_maps(&force_openai, &BTreeMap::new());
        assert!(cfg3.resolve_backend().is_err());
    }

    #[test]
    fn parses_ollama_and_openai_json_responses() {
        let ollama_ok = r#"{"model":"qwen2.5:7b","message":{"role":"assistant","content":"Done."},"done":true}"#;
        assert_eq!(parse_ollama_chat_reply(ollama_ok).unwrap(), "Done.");
        assert!(parse_ollama_chat_reply(r#"{"error":"model not found"}"#).is_err());

        let oa_ok = r#"{"choices":[{"message":{"role":"assistant","content":"Fixed!"}}]}"#;
        assert_eq!(parse_openai_chat_reply(oa_ok).unwrap(), "Fixed!");
        assert!(parse_openai_chat_reply(r#"{"error":{"message":"invalid key"}}"#).is_err());
    }
}
