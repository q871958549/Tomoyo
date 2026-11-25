//! Base64, URL percent-encoding/query-param parser, and JWT expiry inspector for WASM DevTools.

use crate::time_uuid::{copy_item, format_relative, format_utc_iso};
use base64::Engine;
use tomoyo_wasm_sdk::{
    ActionKind, ActionSpec, IconSpec, PreviewSpec, ResultCategory, ResultItem,
};

const PID: &str = "dev.tomoyo.devtools";

pub fn build_base64_items(input: &str) -> Vec<ResultItem> {
    let mut items = Vec::new();
    let encoded = base64::engine::general_purpose::STANDARD.encode(input.as_bytes());
    items.push(copy_item(
        "dev:b64:enc",
        &format!("Base64 编码：{}", clip(&encoded, 48)),
        &format!("原长 {} 字节 → 编码后 {} 字符 · Enter 复制", input.len(), encoded.len()),
        &encoded,
        9_850,
        "B64+",
    ));

    if let Some(decoded) = try_decode_base64(input) {
        items.push(copy_item(
            "dev:b64:dec",
            &format!("Base64 解码：{}", clip(&decoded, 48)),
            &format!("成功还原为有效 UTF-8 文本（{} 字节）· Enter 复制", decoded.len()),
            &decoded,
            9_920,
            "B64-",
        ));
    }
    items
}

pub fn try_decode_base64(raw: &str) -> Option<String> {
    let clean: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() < 4 || !clean.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'-' | b'_' | b'=')) {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&clean)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&clean))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&clean))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&clean))
        .ok()?;
    let txt = String::from_utf8(bytes).ok()?;
    if txt.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        return None;
    }
    Some(txt)
}

pub fn build_url_items(input: &str) -> Vec<ResultItem> {
    let mut items = Vec::new();
    let decoded = percent_decode(input);
    let encoded = percent_encode(input);
    if decoded != input {
        items.push(copy_item(
            "dev:url:dec",
            &format!("URL 解码：{}", clip(&decoded, 52)),
            "已还原 %XX 百分号转义 · Enter 复制",
            &decoded,
            9_930,
            "URL-",
        ));
    }
    items.push(copy_item(
        "dev:url:enc",
        &format!("URL 编码：{}", clip(&encoded, 52)),
        "RFC 3986 URI 组件百分号编码 · Enter 复制",
        &encoded,
        9_860,
        "URL+",
    ));

    if let Some((_, query)) = input.split_once('?') {
        let q = query.split('#').next().unwrap_or(query);
        let pairs: Vec<(String, String)> = q
            .split('&')
            .filter(|s| !s.is_empty())
            .take(16)
            .map(|pair| match pair.split_once('=') {
                Some((k, v)) => (percent_decode(k), percent_decode(v)),
                None => (percent_decode(pair), String::new()),
            })
            .collect();
        if !pairs.is_empty() {
            let json_obj: serde_json::Map<String, serde_json::Value> = pairs
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            let pretty = serde_json::to_string_pretty(&json_obj).unwrap_or_default();
            items.push(ResultItem {
                id: "dev:url:params".into(),
                provider_id: PID.into(),
                title: format!("URL Query 参数拆解（共 {} 项）", pairs.len()),
                subtitle: "已自动解码全部键值对 · Enter 复制为 JSON 对象".into(),
                category: ResultCategory::Plugin,
                score: 9_910,
                icon: IconSpec::Monogram("QRY".into()),
                badge: Some("QUERY".into()),
                copy_text: Some(pretty.clone()),
                actions: vec![ActionSpec {
                    id: "copy_q".into(),
                    label: "复制参数 JSON".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::CopyText { text: pretty.clone(), paste_immediately: false },
                }],
                preview: Some(PreviewSpec::RichCard {
                    title: "URL 查询参数清单".into(),
                    subtitle: format!("共解析 {} 个参数", pairs.len()),
                    badges: vec!["URL".into(), "WASM".into()],
                    rows: pairs,
                    body: pretty,
                }),
            });
        }
    }
    items
}

pub fn decode_jwt_items(token: &str, now_unix: i64) -> Option<Vec<ResultItem>> {
    let mut parts = token.trim().split('.');
    let h_b64 = parts.next()?;
    let p_b64 = parts.next()?;
    let _sig = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let eng = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let h_json: serde_json::Value = serde_json::from_slice(&eng.decode(h_b64).ok()?).ok()?;
    let p_json: serde_json::Value = serde_json::from_slice(&eng.decode(p_b64).ok()?).ok()?;
    let h_pretty = serde_json::to_string_pretty(&h_json).ok()?;
    let p_pretty = serde_json::to_string_pretty(&p_json).ok()?;

    let alg = h_json.get("alg").and_then(|v| v.as_str()).unwrap_or("unknown");
    let sub = p_json.get("sub").and_then(|v| v.as_str()).unwrap_or("-");
    let mut rows = vec![("Algorithm".into(), alg.into()), ("Subject (sub)".into(), sub.into())];
    let mut badges = vec!["JWT".into(), alg.into()];
    let mut exp_note = "无过期字段 (exp)".to_string();
    if let Some(exp) = p_json.get("exp").and_then(|v| v.as_i64()) {
        let expired = now_unix > 0 && exp < now_unix;
        let rel = format_relative(exp, now_unix);
        let iso = format_utc_iso(exp, 0);
        badges.push(if expired { "已过期".into() } else { "有效".into() });
        exp_note = format!("{} ({iso} · {rel})", if expired { "已过期" } else { "有效" });
        rows.push(("过期时间 (exp)".into(), exp_note.clone()));
    }
    if let Some(iat) = p_json.get("iat").and_then(|v| v.as_i64()) {
        rows.push(("签发时间 (iat)".into(), format_utc_iso(iat, 0)));
    }

    Some(vec![
        ResultItem {
            id: "dev:jwt:payload".into(),
            provider_id: PID.into(),
            title: format!("JWT Payload (alg={alg}, sub={sub})"),
            subtitle: format!("{exp_note} · Enter 复制 Payload JSON"),
            category: ResultCategory::Plugin,
            score: 9_960,
            icon: IconSpec::Monogram("JWT".into()),
            badge: Some(badges.last().cloned().unwrap_or_else(|| "JWT".into())),
            copy_text: Some(p_pretty.clone()),
            actions: vec![
                ActionSpec { id: "copy_p".into(), label: "复制 Payload JSON".into(), shortcut: Some("Enter".into()), kind: ActionKind::CopyText { text: p_pretty.clone(), paste_immediately: false } },
                ActionSpec { id: "copy_h".into(), label: "复制 Header JSON".into(), shortcut: Some("Ctrl+Enter".into()), kind: ActionKind::CopyText { text: h_pretty.clone(), paste_immediately: false } },
            ],
            preview: Some(PreviewSpec::RichCard {
                title: format!("JWT 令牌透视 ({alg})"),
                subtitle: "零权限 WASM 沙箱离线解码（绝不外发 Token）".into(),
                badges,
                rows,
                body: p_pretty,
            }),
        },
    ])
}

pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn clip(s: &str, max: usize) -> String {
    let clean: String = s.chars().map(|c| if matches!(c, '\r' | '\n') { ' ' } else { c }).take(max).collect();
    if s.chars().count() > max { format!("{clean}…") } else { clean }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_decodes_b64_url_and_jwt() {
        assert_eq!(try_decode_base64("SGVsbG8gVG9tb3lv").as_deref(), Some("Hello Tomoyo"));
        assert_eq!(percent_decode("hello%20%E4%B8%AD%E6%96%87"), "hello 中文");
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ0b21veW8iLCJleHAiOjE5MDAwMDAwMDB9.sig";
        let res = decode_jwt_items(jwt, 1_700_000_000).unwrap();
        assert_eq!(res.len(), 1);
        assert!(res[0].subtitle.contains("有效"));
    }
}
