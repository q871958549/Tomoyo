//! Pluggable multi-engine native WinHTTP translation adapters (Google, MyMemory+Youdao Dict, DeepL, OpenAI, Ollama).

use super::config::TranslateConfig;
use super::secrets::{get_secret, plugin_secret_key};
use crate::http::{http_get, http_get_with_headers, http_post_bytes};
use crate::PLUGIN_TRANSLATE;
use serde::{Deserialize, Serialize};
use std::time::Instant;

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

/// Resolves target language when `target == "auto"`: CJK-heavy -> `"en"`, otherwise `"zh-Hans"`.
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

/// Pure-Rust UTF-8 query parameter percent encoder (zero external crate).
pub fn url_encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub fn run_engine(
    engine_id: &str,
    cfg: &TranslateConfig,
    text: &str,
    source_lang: &str,
    target_lang: &str,
) -> EngineTranslation {
    let started = Instant::now();
    let (name, res) = match engine_id {
        "google" | "bing" => ("Google 翻译", run_google(text, source_lang, target_lang)),
        "mymemory" => ("MyMemory / 有道词典", run_mymemory(text, source_lang, target_lang)),
        "deepl" => ("DeepL", run_deepl(text, source_lang, target_lang)),
        "openai" => ("OpenAI 兼容", run_openai(cfg, text, target_lang)),
        "ollama" => ("Ollama 本地模型", run_ollama(cfg, text, target_lang)),
        _ => ("Unknown", Err(format!("未知翻译引擎: {engine_id}"))),
    };
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match res {
        Ok(ok) => EngineTranslation {
            engine_id: engine_id.into(),
            engine_name: name.into(),
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
            engine_name: name.into(),
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
    let segs = v
        .as_array()
        .and_then(|a| a.first())
        .and_then(|s| s.as_array())
        .ok_or("Google 返回空结果")?;
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
        .get("responseData")
        .and_then(|d| d.get("translatedText"))
        .and_then(|s| s.as_str())
        .ok_or("MyMemory 缺少译文")?
        .to_string();

    let (phonetic, dict) = maybe_lookup_en_word(text);
    Ok(EngineOk {
        text: translated,
        detected: Some(mm_src.into()),
        phonetic,
        dict,
    })
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
    let phonetic = entry
        .get("phonetic")
        .and_then(|s| s.as_str())
        .map(String::from)
        .or_else(|| {
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
            if let Some(def) = m
                .get("definitions")
                .and_then(|d| d.as_array())
                .and_then(|a| a.first())
                .and_then(|d| d.get("definition"))
                .and_then(|s| s.as_str())
            {
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
    let simple = v.get("simple")?.get("word")?.as_array()?.first();
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

fn resolve_translate_secret(field_key: &str) -> Option<String> {
    get_secret(&plugin_secret_key(PLUGIN_TRANSLATE, field_key)).or_else(|| get_secret(field_key))
}

fn run_deepl(text: &str, src: &str, dst: &str) -> Result<EngineOk, String> {
    let key = resolve_translate_secret("deepl_api_key")
        .ok_or("未配置 DeepL API Key（请在设置中心→翻译引擎加密填入）")?;
    let endpoint = if key.ends_with(":fx") {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    };
    let mut payload = serde_json::json!({
        "text": [text],
        "target_lang": map_deepl_lang(dst),
    });
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
    parse_deepl_response(&resp.text_lossy())
}

fn parse_deepl_response(raw: &str) -> Result<EngineOk, String> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("DeepL JSON 解析失败: {e}"))?;
    let first = v.get("translations").and_then(|a| a.as_array()).and_then(|a| a.first()).ok_or("DeepL 返回空结果")?;
    let text = first.get("text").and_then(|s| s.as_str()).ok_or("DeepL 缺少译文")?.to_string();
    let detected = first.get("detected_source_language").and_then(|s| s.as_str()).map(String::from);
    Ok(EngineOk { text, detected, phonetic: None, dict: None })
}

fn run_openai(cfg: &TranslateConfig, text: &str, dst: &str) -> Result<EngineOk, String> {
    let key = resolve_translate_secret("openai_api_key")
        .ok_or("未配置 OpenAI 兼容 API Key（请在设置中心→翻译引擎加密填入）")?;
    let base = cfg.openai_base_url.trim_end_matches('/');
    let url = format!("{base}/chat/completions");
    let sys = format!("You are a concise, accurate translator. Translate the user's text into {}. Output only the translation without extra commentary.", lang_display_name(dst));
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
    parse_openai_response(&resp.text_lossy())
}

fn parse_openai_response(raw: &str) -> Result<EngineOk, String> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("OpenAI JSON 解析失败: {e}"))?;
    let content = v
        .get("choices")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|s| s.as_str())
        .ok_or("OpenAI 响应缺少译文内容")?
        .trim()
        .to_string();
    Ok(EngineOk { text: content, detected: None, phonetic: None, dict: None })
}

fn run_ollama(cfg: &TranslateConfig, text: &str, dst: &str) -> Result<EngineOk, String> {
    let url = format!("{}/api/chat", cfg.ollama_url.trim_end_matches('/'));
    let sys = format!("Translate the user's text accurately into {}. Return only the translated text.", lang_display_name(dst));
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
    let content = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|s| s.as_str())
        .ok_or("Ollama 响应缺少译文")?
        .trim()
        .to_string();
    Ok(EngineOk { text: content, detected: None, phonetic: None, dict: None })
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
    fn resolves_auto_target_language_accurately() {
        assert_eq!(resolve_target_lang("人工智能启动器", "auto"), "en");
        assert_eq!(resolve_target_lang("Hello native Windows launcher", "auto"), "zh-Hans");
        assert_eq!(resolve_target_lang("Hello", "ja"), "ja");
    }

    #[test]
    fn parses_google_deepl_and_openai_payloads() {
        let g = parse_google_response(r#"[[["你好","Hello",null,null,10],["世界"," world",null,null,10]],null,"en"]"#).unwrap();
        assert_eq!(g.text, "你好世界");
        assert_eq!(g.detected.as_deref(), Some("en"));

        let deepl = parse_deepl_response(r#"{"translations":[{"detected_source_language":"EN","text":"世界你好"}]}"#).unwrap();
        assert_eq!(deepl.text, "世界你好");

        let oa = parse_openai_response(r#"{"choices":[{"message":{"content":"  你好，Rust  "}}]}"#).unwrap();
        assert_eq!(oa.text, "你好，Rust");
    }

    #[test]
    #[ignore]
    fn live_google_and_mymemory_translation_smoke() {
        let cfg = TranslateConfig::default();
        let g = run_engine("google", &cfg, "resilient", "auto", "zh-Hans");
        eprintln!("GOOGLE: {g:?}");
        let m = run_engine("mymemory", &cfg, "resilient", "auto", "zh-Hans");
        eprintln!("MYMEMORY: {m:?}");
        assert!(g.ok, "Google translate failed: {:?}", g.error);
        assert!(m.ok, "MyMemory translate failed: {:?}", m.error);
        assert!(m.phonetic.is_some() && m.extra_dict.is_some(), "Expected Youdao phonetic + Chinese dict");
    }
}
