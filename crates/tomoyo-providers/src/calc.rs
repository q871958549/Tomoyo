use base64::Engine;
use chrono::{Local, TimeZone, Utc};
use std::path::Path;
use tomoyo_core::{detect_intents, DetectedIntent, Provider};
use tomoyo_protocol::{ActionKind, ActionSpec, IconSpec, QueryContext, ResultCategory, ResultItem};
use uuid::Uuid;

/// Instant Calculator, Base/Time/Color/Encoding Converter & Context Intent actions.
pub struct CalcProvider;

impl CalcProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CalcProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for CalcProvider {
    fn id(&self) -> &'static str {
        "builtin.calc"
    }

    fn name(&self) -> &'static str {
        "Calculator & Quick Converters"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["=", "b64", "b64d"]
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let mut out = Vec::new();
        let trimmed = ctx.trimmed.as_str();
        if trimmed.is_empty() {
            return out;
        }

        // Explicit prefix handling: `= <expr>`, `b64 <text>`, `b64d <text>`
        if let Some(kw) = ctx.keyword.as_deref() {
            match kw {
                "=" => {
                    if let Some(item) = eval_math_item(ctx.args.trim(), 9_800) {
                        out.push(item);
                    }
                    return out;
                }
                "b64" => {
                    let encoded = base64::engine::general_purpose::STANDARD.encode(ctx.args.as_bytes());
                    out.push(copy_card(
                        "calc:b64",
                        &encoded,
                        "Base64 Encoded (Enter to copy)",
                        "B64",
                        9_600,
                    ));
                    return out;
                }
                "b64d" => {
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(ctx.args.trim()) {
                        let decoded = String::from_utf8_lossy(&bytes).into_owned();
                        out.push(copy_card(
                            "calc:b64d",
                            &decoded,
                            "Base64 Decoded (Enter to copy)",
                            "B64",
                            9_600,
                        ));
                    }
                    return out;
                }
                _ => {}
            }
        }

        let lower = trimmed.to_lowercase();

        // Instant utility keywords
        if lower == "uuid" {
            let id = Uuid::new_v4().to_string();
            out.push(copy_card(
                "calc:uuid",
                &id,
                "Generated UUID v4 (Enter to copy)",
                "UUID",
                9_700,
            ));
            return out;
        }

        if lower == "now" || lower == "time" || lower == "timestamp" {
            let now = Local::now();
            let ts = now.timestamp().to_string();
            let iso = now.format("%Y-%m-%d %H:%M:%S %:z").to_string();
            out.push(copy_card(
                "calc:now_iso",
                &iso,
                "Current Local DateTime (Enter to copy)",
                "TIME",
                9_600,
            ));
            out.push(copy_card(
                "calc:now_ts",
                &ts,
                "Current Unix Timestamp in Seconds (Enter to copy)",
                "UNIX",
                9_500,
            ));
            return out;
        }

        // Context Intent Detection ("Super Match")
        for intent in detect_intents(trimmed) {
            match intent {
                DetectedIntent::MathCandidate => {
                    if let Some(item) = eval_math_item(trimmed, 9_500) {
                        out.push(item);
                    }
                }
                DetectedIntent::UnixTimestamp { raw, is_millis } => {
                    let dt = if is_millis {
                        Utc.timestamp_millis_opt(raw).single()
                    } else {
                        Utc.timestamp_opt(raw, 0).single()
                    };
                    if let Some(utc) = dt {
                        let local = utc.with_timezone(&Local);
                        let formatted = local.format("%Y-%m-%d %H:%M:%S").to_string();
                        out.push(copy_card(
                            "intent:timestamp",
                            &formatted,
                            &format!(
                                "Unix {} -> UTC {}",
                                if is_millis { "ms" } else { "sec" },
                                utc.format("%Y-%m-%d %H:%M:%S UTC")
                            ),
                            "TIME",
                            9_600,
                        ));
                    }
                }
                DetectedIntent::HexColor { hex, r, g, b } => {
                    let rgb = format!("rgb({r}, {g}, {b})");
                    let (h, s, l) = rgb_to_hsl(r, g, b);
                    let hsl = format!("hsl({h:.0}, {s:.0}%, {l:.0}%)");
                    out.push(copy_card(
                        "intent:color_rgb",
                        &rgb,
                        &format!("Color {hex}  ·  {hsl}  (Enter to copy)"),
                        "HEX",
                        9_600,
                    ));
                }
                DetectedIntent::Url(url) => {
                    out.push(ResultItem {
                        id: format!("intent:url:{url}"),
                        provider_id: self.id().into(),
                        title: format!("Open URL: {url}"),
                        subtitle: "Open in default web browser".into(),
                        category: ResultCategory::Web,
                        score: 9_400,
                        icon: IconSpec::Monogram("WEB".into()),
                        badge: Some("URL".into()),
                        copy_text: Some(url.clone()),
                        actions: vec![
                            ActionSpec {
                                id: "open_url".into(),
                                label: "Open in Browser".into(),
                                shortcut: Some("Enter".into()),
                                kind: ActionKind::OpenUrl { url: url.clone() },
                            },
                            ActionSpec {
                                id: "copy_url".into(),
                                label: "Copy URL".into(),
                                shortcut: Some("Ctrl+C".into()),
                                kind: ActionKind::CopyText {
                                    text: url,
                                    paste_immediately: false,
                                },
                            },
                        ],
                        preview: None,
                    });
                }
                DetectedIntent::WindowsPath(path) => {
                    let exists = Path::new(&path).exists();
                    let dir = if Path::new(&path).is_dir() {
                        path.clone()
                    } else {
                        Path::new(&path)
                            .parent()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.clone())
                    };
                    out.push(ResultItem {
                        id: format!("intent:path:{path}"),
                        provider_id: self.id().into(),
                        title: format!("Open Path: {path}"),
                        subtitle: if exists {
                            "Path exists on disk · Enter to open · Tab for Explorer / Terminal".into()
                        } else {
                            "Path detected (not found on disk)".into()
                        },
                        category: ResultCategory::Folder,
                        score: 9_300,
                        icon: IconSpec::ShellPath(path.clone()),
                        badge: Some(if exists { "PATH" } else { "NEW" }.into()),
                        copy_text: Some(path.clone()),
                        actions: vec![
                            ActionSpec {
                                id: "open_path".into(),
                                label: "Open Path".into(),
                                shortcut: Some("Enter".into()),
                                kind: ActionKind::LaunchPath {
                                    path: path.clone(),
                                    args: None,
                                    working_dir: Some(dir.clone()),
                                },
                            },
                            ActionSpec {
                                id: "reveal".into(),
                                label: "Reveal in Explorer".into(),
                                shortcut: None,
                                kind: ActionKind::RevealInExplorer { path: path.clone() },
                            },
                            ActionSpec {
                                id: "term".into(),
                                label: "Open Terminal Here".into(),
                                shortcut: None,
                                kind: ActionKind::OpenTerminalAt { dir },
                            },
                        ],
                        preview: None,
                    });
                }
                DetectedIntent::JsonSnippet => {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        if let Ok(pretty) = serde_json::to_string_pretty(&val) {
                            out.push(copy_card(
                                "intent:json_pretty",
                                &pretty.lines().take(2).collect::<Vec<_>>().join(" "),
                                "Valid JSON detected · Enter to copy formatted JSON",
                                "JSON",
                                9_500,
                            ));
                            if let Some(last) = out.last_mut() {
                                last.copy_text = Some(pretty.clone());
                                last.actions = vec![ActionSpec {
                                    id: "copy_pretty".into(),
                                    label: "Copy Formatted JSON".into(),
                                    shortcut: Some("Enter".into()),
                                    kind: ActionKind::CopyText {
                                        text: pretty,
                                        paste_immediately: false,
                                    },
                                }];
                            }
                        }
                    }
                }
            }
        }

        out
    }
}

fn copy_card(id: &str, title: &str, subtitle: &str, badge: &str, score: i64) -> ResultItem {
    ResultItem {
        id: id.into(),
        provider_id: "builtin.calc".into(),
        title: title.into(),
        subtitle: subtitle.into(),
        category: ResultCategory::Calc,
        score,
        icon: IconSpec::Monogram("=".into()),
        badge: Some(badge.into()),
        copy_text: Some(title.into()),
        actions: vec![ActionSpec {
            id: "copy".into(),
            label: "Copy Result".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::CopyText {
                text: title.into(),
                paste_immediately: false,
            },
        }],
        preview: None,
    }
}

fn eval_math_item(expr: &str, score: i64) -> Option<ResultItem> {
    let val = eval_expr(expr)?;
    if !val.is_finite() {
        return None;
    }
    let formatted = format_number(val);
    let mut subtitle = format!("{expr} = {formatted}  (Enter to copy)");
    if val.fract() == 0.0 && val >= 0.0 && val <= (u64::MAX as f64) {
        let int_v = val as u64;
        if int_v <= 0xFFFF_FFFF {
            subtitle = format!("{expr} = {formatted}  ·  0x{int_v:X}  ·  0b{int_v:b}");
        }
    }
    Some(copy_card("calc:result", &formatted, &subtitle, "CALC", score))
}

fn format_number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-10 && v.abs() < 1e15 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.8}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let rf = r as f64 / 255.0;
    let gf = g as f64 / 255.0;
    let bf = b as f64 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l * 100.0);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let mut h = if (max - rf).abs() < f64::EPSILON {
        (gf - bf) / d + if gf < bf { 6.0 } else { 0.0 }
    } else if (max - gf).abs() < f64::EPSILON {
        (bf - rf) / d + 2.0
    } else {
        (rf - gf) / d + 4.0
    };
    h *= 60.0;
    (h, s * 100.0, l * 100.0)
}

// --- Zero-dependency Recursive Descent Math Expression Evaluator ---

pub fn eval_expr(input: &str) -> Option<f64> {
    let mut p = Parser::new(input);
    let val = p.parse_add_sub()?;
    p.skip_ws();
    if p.pos == p.src.len() {
        Some(val)
    } else {
        None
    }
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            src: s.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.src.get(self.pos).copied()
    }

    fn parse_add_sub(&mut self) -> Option<f64> {
        let mut left = self.parse_mul_div()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.pos += 1;
                    left += self.parse_mul_div()?;
                }
                Some(b'-') => {
                    self.pos += 1;
                    left -= self.parse_mul_div()?;
                }
                _ => break,
            }
        }
        Some(left)
    }

    fn parse_mul_div(&mut self) -> Option<f64> {
        let mut left = self.parse_pow()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.pos += 1;
                    left *= self.parse_pow()?;
                }
                Some(b'/') => {
                    self.pos += 1;
                    let r = self.parse_pow()?;
                    if r == 0.0 {
                        return None;
                    }
                    left /= r;
                }
                Some(b'%') => {
                    self.pos += 1;
                    let r = self.parse_pow()?;
                    if r == 0.0 {
                        return None;
                    }
                    left %= r;
                }
                _ => break,
            }
        }
        Some(left)
    }

    fn parse_pow(&mut self) -> Option<f64> {
        let base = self.parse_unary()?;
        if self.peek() == Some(b'^') {
            self.pos += 1;
            let exp = self.parse_pow()?;
            Some(base.powf(exp))
        } else {
            Some(base)
        }
    }

    fn parse_unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(b'-') => {
                self.pos += 1;
                Some(-self.parse_unary()?)
            }
            Some(b'+') => {
                self.pos += 1;
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Option<f64> {
        let ch = self.peek()?;
        if ch == b'(' {
            self.pos += 1;
            let val = self.parse_add_sub()?;
            if self.peek() != Some(b')') {
                return None;
            }
            self.pos += 1;
            return Some(val);
        }

        // Hex (`0x...`) or Binary (`0b...`)
        if ch == b'0' && self.pos + 2 < self.src.len() {
            let next = self.src[self.pos + 1];
            if next == b'x' || next == b'X' {
                self.pos += 2;
                let start = self.pos;
                while self.pos < self.src.len() && self.src[self.pos].is_ascii_hexdigit() {
                    self.pos += 1;
                }
                let s = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
                return u64::from_str_radix(s, 16).ok().map(|v| v as f64);
            }
            if next == b'b' || next == b'B' {
                self.pos += 2;
                let start = self.pos;
                while self.pos < self.src.len() && matches!(self.src[self.pos], b'0' | b'1') {
                    self.pos += 1;
                }
                let s = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
                return u64::from_str_radix(s, 2).ok().map(|v| v as f64);
            }
        }

        // Decimal number
        if ch.is_ascii_digit() || ch == b'.' {
            let start = self.pos;
            while self.pos < self.src.len()
                && (self.src[self.pos].is_ascii_digit() || self.src[self.pos] == b'.')
            {
                self.pos += 1;
            }
            let s = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
            return s.parse::<f64>().ok();
        }

        // Functions & constants
        if ch.is_ascii_alphabetic() {
            let start = self.pos;
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_alphanumeric() {
                self.pos += 1;
            }
            let ident = std::str::from_utf8(&self.src[start..self.pos])
                .ok()?
                .to_lowercase();
            return match ident.as_str() {
                "pi" => Some(std::f64::consts::PI),
                "e" => Some(std::f64::consts::E),
                fn_name => {
                    if self.peek() != Some(b'(') {
                        return None;
                    }
                    self.pos += 1;
                    let arg = self.parse_add_sub()?;
                    if self.peek() != Some(b')') {
                        return None;
                    }
                    self.pos += 1;
                    match fn_name {
                        "sqrt" => Some(arg.sqrt()),
                        "abs" => Some(arg.abs()),
                        "sin" => Some(arg.sin()),
                        "cos" => Some(arg.cos()),
                        "tan" => Some(arg.tan()),
                        "ln" => Some(arg.ln()),
                        "log" | "log10" => Some(arg.log10()),
                        "floor" => Some(arg.floor()),
                        "ceil" => Some(arg.ceil()),
                        "round" => Some(arg.round()),
                        _ => None,
                    }
                }
            };
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_math_and_bases() {
        assert_eq!(eval_expr("(2 + 3) * 4"), Some(20.0));
        assert_eq!(eval_expr("2^10"), Some(1024.0));
        assert_eq!(eval_expr("0xff + 1"), Some(256.0));
        assert_eq!(eval_expr("sqrt(144)"), Some(12.0));
        assert_eq!(eval_expr("0b1010"), Some(10.0));
    }
}
