//! Parallel multi-engine translation adapters (Google, MyMemory+Youdao, DeepL, OpenAI, Ollama).

use crate::http_curl::{http_get, http_get_with_headers, http_post_bytes, url_encode_component};
use crate::store::{TranslateRuntimeConfig, TranslateStore};
use crate::style_prompt::{build_llm_system_prompt, normalize_style, style_label_zh};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Instant;

const MAX_INPUT_CHARS: usize = 4000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineTranslation {
    pub engine_id: String,
    pub engine_name: String,
    pub ok: bool,
    pub translated: String,
    pub detected_lang: Option<String>,
    pub phonetic: Option<String>,
    pub extra_dict: Option<String>,
    pub error: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateBatchResponse {
    pub source_text: String,
    pub target_lang: String,
    pub results: Vec<EngineTranslation>,
}

pub fn resolve_target_lang(text: &str, target: &str) -> String {
    let t = target.trim();
    if !t.is_empty() && t != "auto" {
        return t.to_string();
    }
    let total = text.chars().filter(|c| !c.is_whitespace()).count().max(1);
    let cjk = text
        .chars()
        .filter(|c| matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}'))
        .count();
    if cjk * 5 >= total { "en".into() } else { "zh-Hans".into() }
}

pub fn translate_parallel(
    cfg: &TranslateRuntimeConfig,
    secrets: &BTreeMap<String, String>,
    raw_text: &str,
    source_lang: &str,
    requested_target: &str,
    requested_style: &str,
) -> (TranslateBatchResponse, TranslateStore) {
    let style = normalize_style(requested_style);
    let text: String = raw_text.trim().chars().take(MAX_INPUT_CHARS).collect();
    let eff_target = resolve_target_lang(
        &text,
        if requested_target.is_empty() { &cfg.target_lang } else { requested_target },
    );
    let mut store = TranslateStore::load_default();
    if text.is_empty() {
        return (
            TranslateBatchResponse { source_text: text, target_lang: eff_target, results: Vec::new() },
            store,
        );
    }

    let engine_ids = if cfg.engines.is_empty() {
        vec!["google".into(), "mymemory".into()]
    } else {
        cfg.engines.clone()
    };

    let results: Vec<EngineTranslation> = std::thread::scope(|s| {
        let handles: Vec<_> = engine_ids
            .iter()
            .map(|id| {
                let id_ref = id.as_str();
                let txt_ref = text.as_str();
                let tgt_ref = eff_target.as_str();
                s.spawn(move || run_engine(id_ref, cfg, secrets, txt_ref, source_lang, tgt_ref, style))
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });

    if let Some(first_ok) = results.iter().find(|r| r.ok && !r.translated.trim().is_empty()) {
        store.record_history(&text, &first_ok.translated, &eff_target, &first_ok.engine_name);
        store.save_default();
    }

    (
        TranslateBatchResponse { source_text: text, target_lang: eff_target, results },
        store,
    )
}

fn run_engine(
    engine_id: &str,
    cfg: &TranslateRuntimeConfig,
    secrets: &BTreeMap<String, String>,
    text: &str,
    source_lang: &str,
    target_lang: &str,
    style: &str,
) -> EngineTranslation {
    let started = Instant::now();
    let suffix = if style == "standard" { String::new() } else { format!(" · {}", style_label_zh(style)) };
    let (name, res) = match engine_id {
        "google" | "bing" => ("Google 翻译".to_string(), run_google(text, source_lang, target_lang)),
        "mymemory" => ("MyMemory / 有道词典".to_string(), run_mymemory(text, source_lang, target_lang)),
        "deepl" => ("DeepL".to_string(), run_deepl(secrets, text, source_lang, target_lang)),
        "openai" => (format!("OpenAI 兼容{suffix}"), run_openai(cfg, secrets, text, target_lang, style)),
        "ollama" => (format!("Ollama 本地模型{suffix}"), run_ollama(cfg, text, target_lang, style)),
        _ => ("Unknown".to_string(), Err(format!("未知翻译引擎: {engine_id}"))),
    };
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match res {
        Ok(ok) => EngineTranslation {
            engine_id: engine_id.into(),
            engine_name: name,
            ok: true,
            translated: ok.text,
            detected_lang: ok.detected,
            phonetic: ok.phonetic,
            extra_dict: ok.dict,
            error: None,
            elapsed_ms,
        },
        Err(err) => EngineTranslation {
            engine_id: engine_id.into(),
            engine_name: name,
            ok: false,
            translated: String::new(),
            detected_lang: None,
            phonetic: None,
            extra_dict: None,
            error: Some(err),
            elapsed_ms,
        },
    }
}

struct EngineOk {
    text: String,
    detected: Option<String>,
    phonetic: Option<String>,
    dict: Option<String>,
}

fn run_google(text: &str, src: &str, dst: &str) -> Result<EngineOk, String> {
    const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
    let sl = if src.is_empty() || src == "auto" { "auto" } else { map_mymemory_lang(src) };
    let tl = map_mymemory_lang(dst);
    let mut last_status = 0u16;
    for client in ["gtx", "dict-chrome-ex"] {
        let url = format!(
            "https://translate.googleapis.com/translate_a/single?client={client}&sl={}&tl={}&dt=t&q={}",
            url_encode_component(sl),
            url_encode_component(tl),
            url_encode_component(text)
        );
        let resp = http_get_with_headers(&url, &[("User-Agent", UA)], 8000, 256 * 1024)?;
        if resp.status_code == 200 {
            return parse_google_response(&resp.text_lossy());
        }
        last_status = resp.status_code;
        if resp.status_code != 429 {
            break;
        }
    }
    Err(format!("Google HTTP {last_status}"))
}

fn parse_google_response(raw: &str) -> Result<EngineOk, String> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("Google JSON 解析失败: {e}"))?;
    let segs = v.as_array().and_then(|a| a.first()).and_then(|s| s.as_array()).ok_or("Google 返回空结果")?;
    let mut translated = String::new();
    for seg in segs {
        if let Some(piece) = seg.as_array().and_then(|a| a.first()).and_then(|x| x.as_str()) {
            translated.push_str(piece);
        }
    }
    if translated.trim().is_empty() {
        return Err("Google 缺少译文字段".into());
    }
    let detected = v.as_array().and_then(|a| a.get(2)).and_then(|x| x.as_str()).map(String::from);
    Ok(EngineOk { text: translated, detected, phonetic: None, dict: None })
}

fn run_mymemory(text: &str, src: &str, dst: &str) -> Result<EngineOk, String> {
    let mm_dst = map_mymemory_lang(dst);
    let mm_src = if src.is_empty() || src == "auto" {
        if mm_dst == "en" { "zh-CN" } else { "en" }
    } else {
        map_mymemory_lang(src)
    };
    let clipped: String = text.chars().take(480).collect();
    let url = format!(
        "https://api.mymemory.translated.net/get?q={}&langpair={}|{}",
        url_encode_component(&clipped),
        mm_src,
        mm_dst
    );
    let resp = http_get(&url, 8000, 128 * 1024)?;
    if resp.status_code != 200 {
        return Err(format!("MyMemory HTTP {}", resp.status_code));
    }
    let v: serde_json::Value = serde_json::from_str(&resp.text_lossy()).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let translated = v
        .pointer("/responseData/translatedText")
        .and_then(|s| s.as_str())
        .ok_or("MyMemory 缺少译文")?
        .to_string();

    let (phonetic, dict) = maybe_lookup_en_word(text);
    Ok(EngineOk { text: translated, detected: Some(mm_src.into()), phonetic, dict })
}

fn maybe_lookup_en_word(text: &str) -> (Option<String>, Option<String>) {
    let w = text.trim();
    if w.is_empty() || w.len() > 28 || !w.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-') {
        return (None, None);
    }
    if let Some(yd) = lookup_youdao_ec(w) {
        return yd;
    }
    let url = format!("https://api.dictionaryapi.dev/api/v2/entries/en/{}", w.to_ascii_lowercase());
    let Ok(resp) = http_get(&url, 3500, 64 * 1024) else { return (None, None) };
    if resp.status_code != 200 {
        return (None, None);
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp.text_lossy()) else {
        return (None, None);
    };
    let Some(entry) = v.as_array().and_then(|a| a.first()) else { return (None, None) };
    let phonetic = entry.get("phonetic").and_then(|s| s.as_str()).map(String::from).or_else(|| {
        entry
            .get("phonetics")
            .and_then(|p| p.as_array())
            .and_then(|a| a.iter().find_map(|x| x.get("text").and_then(|s| s.as_str())))
            .map(String::from)
    });
    let mut defs = Vec::new();
    if let Some(meanings) = entry.get("meanings").and_then(|m| m.as_array()) {
        for m in meanings.iter().take(3) {
            let pos = m.get("partOfSpeech").and_then(|s| s.as_str()).unwrap_or("");
            if let Some(def) = m.pointer("/definitions/0/definition").and_then(|s| s.as_str()) {
                defs.push(format!("[{pos}] {def}"));
            }
        }
    }
    (phonetic, (!defs.is_empty()).then(|| defs.join("\n")))
}

fn lookup_youdao_ec(word: &str) -> Option<(Option<String>, Option<String>)> {
    let url = format!(
        "https://dict.youdao.com/jsonapi?q={}&dicts=%7B%22count%22%3A1%2C%22dicts%22%3A%5B%5B%22ec%22%5D%5D%7D",
        url_encode_component(&word.to_ascii_lowercase())
    );
    let resp = http_get(&url, 3500, 64 * 1024).ok()?;
    if resp.status_code != 200 {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&resp.text_lossy()).ok()?;
    let simple = v.pointer("/simple/word/0");
    let us = simple.and_then(|w| w.get("usphone")).and_then(|s| s.as_str()).filter(|s| !s.is_empty());
    let uk = simple.and_then(|w| w.get("ukphone")).and_then(|s| s.as_str()).filter(|s| !s.is_empty());
    let phonetic = match (us, uk) {
        (Some(u), Some(k)) if u != k => Some(format!("US /{u}/ · UK /{k}/")),
        (Some(u), _) => Some(format!("/{u}/")),
        (None, Some(k)) => Some(format!("UK /{k}/")),
        _ => None,
    };
    let mut defs = Vec::new();
    if let Some(trs) = v.pointer("/ec/word/0/trs").and_then(|x| x.as_array()) {
        for item in trs.iter().take(4) {
            if let Some(line) = item.pointer("/tr/0/l/i/0").and_then(|s| s.as_str()) {
                defs.push(line.to_string());
            }
        }
    }
    if phonetic.is_none() && defs.is_empty() {
        return None;
    }
    Some((phonetic, (!defs.is_empty()).then(|| defs.join("\n"))))
}

fn run_deepl(secrets: &BTreeMap<String, String>, text: &str, src: &str, dst: &str) -> Result<EngineOk, String> {
    let key = secrets
        .get("deepl_api_key")
        .filter(|s| !s.trim().is_empty())
        .ok_or("未配置 DeepL API Key（请在设置中心→已安装插件→多源对照翻译中加密填入）")?;
    let endpoint = if key.ends_with(":fx") {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    };
    let mut payload = serde_json::json!({ "text": [text], "target_lang": map_deepl_lang(dst) });
    if !src.is_empty() && src != "auto" {
        payload["source_lang"] = serde_json::json!(map_deepl_lang(src));
    }
    let auth = format!("DeepL-Auth-Key {key}");
    let resp = http_post_bytes(
        endpoint,
        &[("Authorization", &auth), ("Content-Type", "application/json")],
        payload.to_string().as_bytes(),
        9000,
        256 * 1024,
    )?;
    if resp.status_code != 200 {
        return Err(format!("DeepL HTTP {}", resp.status_code));
    }
    let v: serde_json::Value = serde_json::from_str(&resp.text_lossy()).map_err(|e| format!("DeepL JSON 解析失败: {e}"))?;
    let first = v.pointer("/translations/0").ok_or("DeepL 返回空结果")?;
    let out = first.get("text").and_then(|s| s.as_str()).ok_or("DeepL 缺少译文")?.to_string();
    let detected = first.get("detected_source_language").and_then(|s| s.as_str()).map(String::from);
    Ok(EngineOk { text: out, detected, phonetic: None, dict: None })
}

fn run_openai(
    cfg: &TranslateRuntimeConfig,
    secrets: &BTreeMap<String, String>,
    text: &str,
    dst: &str,
    style: &str,
) -> Result<EngineOk, String> {
    let key = secrets
        .get("openai_api_key")
        .filter(|s| !s.trim().is_empty())
        .ok_or("未配置 OpenAI 兼容 API Key（请在设置中心→已安装插件→多源对照翻译中加密填入）")?;
    let url = format!("{}/chat/completions", cfg.openai_base_url.trim_end_matches('/'));
    let sys = build_llm_system_prompt(lang_display_name(dst), style);
    let payload = serde_json::json!({
        "model": cfg.openai_model,
        "temperature": 0.1,
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
        15_000,
        256 * 1024,
    )?;
    if resp.status_code != 200 {
        return Err(format!("OpenAI HTTP {}", resp.status_code));
    }
    let v: serde_json::Value = serde_json::from_str(&resp.text_lossy()).map_err(|e| format!("OpenAI JSON 解析失败: {e}"))?;
    let out = v
        .pointer("/choices/0/message/content")
        .and_then(|s| s.as_str())
        .ok_or("OpenAI 响应缺少译文内容")?
        .trim()
        .to_string();
    Ok(EngineOk { text: out, detected: None, phonetic: None, dict: None })
}

fn run_ollama(cfg: &TranslateRuntimeConfig, text: &str, dst: &str, style: &str) -> Result<EngineOk, String> {
    let url = format!("{}/api/chat", cfg.ollama_url.trim_end_matches('/'));
    let sys = build_llm_system_prompt(lang_display_name(dst), style);
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
        18_000,
        256 * 1024,
    )?;
    if resp.status_code != 200 {
        return Err(format!("Ollama HTTP {}（请确认本地 Ollama 服务已启动）", resp.status_code));
    }
    let v: serde_json::Value = serde_json::from_str(&resp.text_lossy()).map_err(|e| format!("Ollama JSON 解析失败: {e}"))?;
    let out = v
        .pointer("/message/content")
        .and_then(|s| s.as_str())
        .ok_or("Ollama 响应缺少译文")?
        .trim()
        .to_string();
    Ok(EngineOk { text: out, detected: None, phonetic: None, dict: None })
}

fn map_mymemory_lang(code: &str) -> &'static str {
    match code {
        "zh-Hans" | "zh" | "zh-CN" => "zh-CN",
        "ja" => "ja",
        "ko" => "ko",
        "fr" => "fr",
        "de" => "de",
        _ => "en",
    }
}

fn map_deepl_lang(code: &str) -> &'static str {
    match code {
        "zh-Hans" | "zh" | "zh-CN" => "ZH",
        "ja" => "JA",
        "ko" => "KO",
        "fr" => "FR",
        "de" => "DE",
        _ => "EN",
    }
}

fn lang_display_name(code: &str) -> &'static str {
    match code {
        "zh-Hans" | "zh" => "Simplified Chinese",
        "ja" => "Japanese",
        "ko" => "Korean",
        "fr" => "French",
        "de" => "German",
        _ => "English",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_auto_target_lang_and_parses_google_json() {
        assert_eq!(resolve_target_lang("人工智能启动器", "auto"), "en");
        assert_eq!(resolve_target_lang("Hello plugin world", "auto"), "zh-Hans");
        let parsed = parse_google_response(r#"[[["你好","Hello",null,null,10]],null,"en"]"#).unwrap();
        assert_eq!(parsed.text, "你好");
        assert_eq!(parsed.detected.as_deref(), Some("en"));
    }
}
