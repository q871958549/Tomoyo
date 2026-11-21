//! Instant Calculator, Base/Time/Color/Encoding Converter & Context Intent actions.

mod eval;

pub use eval::eval_expr;

use base64::Engine;
use chrono::{Local, TimeZone, Utc};
use eval::{format_number, rgb_to_hsl};
use std::path::Path;
use tomoyo_core::{detect_intents, DetectedIntent, Provider};
use tomoyo_protocol::{ActionKind, ActionSpec, IconSpec, QueryContext, ResultCategory, ResultItem};
use uuid::Uuid;

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
                    out.push(copy_card("calc:b64", &encoded, "Base64 Encoded (Enter to copy)", "B64", 9_600));
                    return out;
                }
                "b64d" => {
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(ctx.args.trim()) {
                        let decoded = String::from_utf8_lossy(&bytes).into_owned();
                        out.push(copy_card("calc:b64d", &decoded, "Base64 Decoded (Enter to copy)", "B64", 9_600));
                    }
                    return out;
                }
                _ => {}
            }
        }

        let lower = trimmed.to_lowercase();
        if lower == "uuid" {
            let id = Uuid::new_v4().to_string();
            out.push(copy_card("calc:uuid", &id, "Generated UUID v4 (Enter to copy)", "UUID", 9_700));
            return out;
        }

        if lower == "now" || lower == "time" || lower == "timestamp" {
            let now = Local::now();
            out.push(copy_card(
                "calc:now_iso",
                &now.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
                "Current Local DateTime (Enter to copy)",
                "TIME",
                9_600,
            ));
            out.push(copy_card(
                "calc:now_ts",
                &now.timestamp().to_string(),
                "Current Unix Timestamp in Seconds (Enter to copy)",
                "UNIX",
                9_500,
            ));
            return out;
        }

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
                        let formatted = utc.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string();
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
                    out.push(copy_card(
                        "intent:color_rgb",
                        &rgb,
                        &format!("Color {hex}  ·  hsl({h:.0}, {s:.0}%, {l:.0}%)  (Enter to copy)"),
                        "HEX",
                        9_600,
                    ));
                }
                DetectedIntent::Url(url) => out.push(build_url_item(self.id(), url)),
                DetectedIntent::WindowsPath(path) => out.push(build_path_item(self.id(), path)),
                DetectedIntent::JsonSnippet => {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed)
                        && let Ok(pretty) = serde_json::to_string_pretty(&val) {
                            let mut item = copy_card(
                                "intent:json_pretty",
                                &pretty.lines().take(2).collect::<Vec<_>>().join(" "),
                                "Valid JSON detected · Enter to copy formatted JSON",
                                "JSON",
                                9_500,
                            );
                            item.copy_text = Some(pretty.clone());
                            item.actions = vec![ActionSpec {
                                id: "copy_pretty".into(),
                                label: "Copy Formatted JSON".into(),
                                shortcut: Some("Enter".into()),
                                kind: ActionKind::CopyText {
                                    text: pretty,
                                    paste_immediately: false,
                                },
                            }];
                            out.push(item);
                        }
                }
            }
        }
        out
    }
}

fn build_url_item(provider_id: &str, url: String) -> ResultItem {
    ResultItem {
        id: format!("intent:url:{url}"),
        provider_id: provider_id.into(),
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
    }
}

fn build_path_item(provider_id: &str, path: String) -> ResultItem {
    let exists = Path::new(&path).exists();
    let dir = if Path::new(&path).is_dir() {
        path.clone()
    } else {
        Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone())
    };
    ResultItem {
        id: format!("intent:path:{path}"),
        provider_id: provider_id.into(),
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
