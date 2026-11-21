//! Zero-resident BYOM/BYOK local LLM & OpenAI-compatible transform helper for `flows` (`llm_transform`).
//!
//! Reuses Tomoyo's native WinHTTP client + DPAPI-encrypted `openai_api_key` + local Ollama URL.
//! Zero extra crates, zero resident RAM/VRAM when idle.

use crate::flows::{FlowStep, FlowStepKind, WorkflowDef};
use crate::http::http_post_bytes;
use crate::translate::secrets::get_secret;
use crate::TranslateConfig;

/// Seeds 6 practical zero-bloat built-in declarative workflows on first run.
pub fn default_workflows() -> Vec<WorkflowDef> {
    vec![
        WorkflowDef {
            id: "shot_ocr_tr".into(),
            name: "截图识字并多源翻译".into(),
            keyword: "shottr".into(),
            hotkey: String::new(),
            description: "框选屏幕区域 → WinRT 离线 OCR → 立即打开多源翻译对比".into(),
            enabled: true,
            steps: vec![FlowStep { kind: FlowStepKind::CaptureTranslate, value: String::new() }],
        },
        WorkflowDef {
            id: "clip_clean_paste".into(),
            name: "剪贴板去换行纯文本直贴".into(),
            keyword: "cleanpaste".into(),
            hotkey: String::new(),
            description: "读取剪贴板 → 合并断行与多余空格 → 无格式直接贴回当前窗口（PDF 复制利器）".into(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::SingleLine, value: String::new() },
                FlowStep { kind: FlowStepKind::PasteText, value: String::new() },
            ],
        },
        WorkflowDef {
            id: "clip_translate".into(),
            name: "剪贴板一键多源翻译".into(),
            keyword: "cliptr".into(),
            hotkey: String::new(),
            description: "读取剪贴板或输入参数 → 清理首尾空白 → 拉起多源翻译工作台".into(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::Trim, value: String::new() },
                FlowStep { kind: FlowStepKind::LaunchTranslate, value: String::new() },
            ],
        },
        WorkflowDef {
            id: "clip_to_note".into(),
            name: "剪贴板整理并存为本地备忘".into(),
            keyword: "clipnote".into(),
            hotkey: String::new(),
            description: "读取剪贴板 → 去除多余空行 → 直接写入本地 Markdown 备忘录目录".into(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::TrimLines, value: String::new() },
                FlowStep { kind: FlowStepKind::SaveNote, value: "剪贴板摘录".into() },
                FlowStep { kind: FlowStepKind::Toast, value: "已将剪贴板保存为本地 Markdown 备忘录".into() },
            ],
        },
        WorkflowDef {
            id: "clip_web_search".into(),
            name: "剪贴板文本速查网页".into(),
            keyword: "clipweb".into(),
            hotkey: String::new(),
            description: "读取剪贴板 → 合并单行 → URL 编码并在浏览器中检索".into(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::SingleLine, value: String::new() },
                FlowStep { kind: FlowStepKind::UrlEncode, value: String::new() },
                FlowStep { kind: FlowStepKind::OpenUrl, value: "https://www.google.com/search?q={{text}}".into() },
            ],
        },
        WorkflowDef {
            id: "clip_ai_summary".into(),
            name: "剪贴板本地 AI 提炼要点".into(),
            keyword: "aisum".into(),
            hotkey: String::new(),
            description: "调用本地 Ollama 或已加密配置的 OpenAI 兼容模型提炼 3 条核心要点并复制（按需触发 0 常驻）".into(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::Trim, value: String::new() },
                FlowStep { kind: FlowStepKind::LlmTransform, value: "请用简体中文简明提炼以下内容的 3 条核心要点，直接输出条列结果：".into() },
                FlowStep { kind: FlowStepKind::CopyText, value: String::new() },
                FlowStep { kind: FlowStepKind::Toast, value: "已完成 AI 提炼并复制到剪贴板".into() },
            ],
        },
    ]
}

/// Executes a single bounded LLM text transformation using local Ollama or OpenAI-compatible API.
pub fn run_flow_llm_transform(
    cfg: &TranslateConfig,
    prompt: &str,
    input_text: &str,
) -> Result<String, String> {
    let clipped: String = input_text.trim().chars().take(12_000).collect();
    if clipped.is_empty() {
        return Err("输入文本与剪贴板均为空，无法执行 AI 文本处理".into());
    }
    let sys = if prompt.trim().is_empty() {
        "You are a concise, helpful desktop text assistant. Return only the processed result without chatter."
    } else {
        prompt.trim()
    };

    let has_openai_key = get_secret("openai_api_key").is_some();
    let prefer_ollama = cfg.engines.iter().any(|e| e.eq_ignore_ascii_case("ollama")) || !has_openai_key;

    if prefer_ollama {
        match call_ollama_transform(cfg, sys, &clipped) {
            Ok(out) => return Ok(out),
            Err(ollama_err) => {
                if has_openai_key {
                    return call_openai_transform(cfg, sys, &clipped)
                        .map_err(|oe| format!("本地 Ollama 不可用({ollama_err})，OpenAI 回退也失败：{oe}"));
                }
                return Err(format!(
                    "本地 Ollama 调用失败（请确认 Ollama 正在运行，或在设置中心→翻译引擎配置 OpenAI 兼容 API Key）：{ollama_err}"
                ));
            }
        }
    }

    call_openai_transform(cfg, sys, &clipped)
}

fn call_ollama_transform(cfg: &TranslateConfig, sys: &str, text: &str) -> Result<String, String> {
    let url = format!("{}/api/chat", cfg.ollama_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": cfg.ollama_model,
        "stream": false,
        "messages": [
            { "role": "system", "content": sys },
            { "role": "user", "content": text }
        ]
    });
    let resp = http_post_bytes(
        &url,
        &[("Content-Type", "application/json")],
        payload.to_string().as_bytes(),
        25_000,
        256 * 1024,
    )?;
    if resp.status_code != 200 {
        return Err(format!("Ollama HTTP {}", resp.status_code));
    }
    parse_ollama_chat_content(&resp.text_lossy())
}

fn call_openai_transform(cfg: &TranslateConfig, sys: &str, text: &str) -> Result<String, String> {
    let key = get_secret("openai_api_key")
        .ok_or_else(|| "未配置 OpenAI 兼容 API Key（请在设置中心→翻译引擎加密填入）".to_string())?;
    let url = format!("{}/chat/completions", cfg.openai_base_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": cfg.openai_model,
        "temperature": 0.2,
        "messages": [
            { "role": "system", "content": sys },
            { "role": "user", "content": text }
        ]
    });
    let auth = format!("Bearer {key}");
    let resp = http_post_bytes(
        &url,
        &[("Authorization", &auth), ("Content-Type", "application/json")],
        payload.to_string().as_bytes(),
        25_000,
        256 * 1024,
    )?;
    if resp.status_code != 200 {
        return Err(format!("OpenAI HTTP {}", resp.status_code));
    }
    parse_openai_chat_content(&resp.text_lossy())
}

fn parse_ollama_chat_content(raw: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("JSON 解析失败: {e}"))?;
    v.pointer("/message/content")
        .and_then(|s| s.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Ollama 响应缺少文本内容".into())
}

fn parse_openai_chat_content(raw: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("JSON 解析失败: {e}"))?;
    v.pointer("/choices/0/message/content")
        .and_then(|s| s.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "OpenAI 响应缺少文本内容".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ollama_and_openai_chat_responses() {
        let ol = r#"{"message":{"role":"assistant","content":"  1. 要点一\n2. 要点二  "}}"#;
        assert_eq!(parse_ollama_chat_content(ol).unwrap(), "1. 要点一\n2. 要点二");
        let oa = r#"{"choices":[{"message":{"content":"  Fixed command: cargo test  "}}]}"#;
        assert_eq!(parse_openai_chat_content(oa).unwrap(), "Fixed command: cargo test");
    }
}
