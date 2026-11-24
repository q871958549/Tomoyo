use base64::Engine;
use tomoyo_wasm_sdk::{
    export_tomoyo_wasm_plugin, ActionKind, ActionSpec, IconSpec, PreviewSpec, QueryContext,
    ResultCategory, ResultItem, TomoyoWasmPlugin,
};

struct WasmToolkit;

impl TomoyoWasmPlugin for WasmToolkit {
    fn query(ctx: &QueryContext) -> Vec<ResultItem> {
        let kw = ctx.keyword.as_deref().unwrap_or("wasm");
        let args = ctx.args.trim();

        if args.is_empty() {
            let now = tomoyo_wasm_sdk::host::now_unix();
            return vec![
                ResultItem {
                    id: "wasm:hint".into(),
                    provider_id: "dev.tomoyo.example-wasm".into(),
                    title: "WASM Capability Sandbox Ready (0 ambient permissions)".into(),
                    subtitle: format!(
                        "Try: `jwt <token>` · `unit 2.5gb` · `unit 4096kb` · host clock: {now}"
                    ),
                    category: ResultCategory::Plugin,
                    score: 9_600,
                    icon: IconSpec::Monogram("WSM".into()),
                    badge: Some("WASM".into()),
                    copy_text: Some(now.to_string()),
                    actions: vec![],
                    preview: Some(PreviewSpec::RichCard {
                        title: "WASM Sandbox Isolation".into(),
                        subtitle: "Zero ambient permissions · Native RichCard preview".into(),
                        badges: vec!["WASM".into(), "Zero-JIT".into(), "16 MB Cap".into()],
                        rows: vec![
                            ("Engine".into(), "wasmi (Pure Interpreter)".into()),
                            ("Fuel Cap".into(), "5,000,000 ops/call".into()),
                            ("Permissions".into(), "None (Pure Compute)".into()),
                            ("Examples".into(), "unit 512mb  |  jwt eyJ...".into()),
                        ],
                        body: "Declarative RichCard renders natively via Tomoyo's SDF+GDI/Pango pipeline with 0 MB WebView overhead.".into(),
                    }),
                },
            ];
        }

        let mut items = Vec::new();

        // 1. JWT Decoder (`jwt <header.payload.sig>` or auto-detected 3-segment token)
        if kw == "jwt" || args.split('.').count() == 3 {
            if let Some(jwt_items) = decode_jwt(args) {
                items.extend(jwt_items);
            }
        }

        // 2. Byte / Storage Unit Converter (`unit 2.5gb`, `unit 1024kb`, `wasm 64mb`)
        if let Some(conv_items) = convert_byte_units(args) {
            items.extend(conv_items);
        }

        if items.is_empty() {
            items.push(ResultItem {
                id: "wasm:echo".into(),
                provider_id: "dev.tomoyo.example-wasm".into(),
                title: format!("WASM Sandbox: {args}"),
                subtitle: format!(
                    "Length: {} chars · {} UTF-8 bytes · Pass `unit 2gb` or a JWT token",
                    args.chars().count(),
                    args.len()
                ),
                category: ResultCategory::Plugin,
                score: 9_200,
                icon: IconSpec::Monogram("WSM".into()),
                badge: Some("WASM".into()),
                copy_text: Some(args.to_string()),
                actions: vec![ActionSpec {
                    id: "copy".into(),
                    label: "Copy Text".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::CopyText {
                        text: args.to_string(),
                        paste_immediately: false,
                    },
                }],
                preview: None,
            });
        }

        items
    }
}

fn decode_jwt(token: &str) -> Option<Vec<ResultItem>> {
    let mut parts = token.split('.');
    let header_b64 = parts.next()?;
    let payload_b64 = parts.next()?;
    let _sig = parts.next()?;

    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header_raw = engine.decode(header_b64).ok()?;
    let payload_raw = engine.decode(payload_b64).ok()?;
    let header_json: serde_json::Value = serde_json::from_slice(&header_raw).ok()?;
    let payload_json: serde_json::Value = serde_json::from_slice(&payload_raw).ok()?;

    let payload_pretty = serde_json::to_string_pretty(&payload_json).ok()?;
    let header_pretty = serde_json::to_string_pretty(&header_json).ok()?;

    let alg = header_json
        .get("alg")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let sub = payload_json
        .get("sub")
        .and_then(|v| v.as_str())
        .unwrap_or("(no sub)");

    Some(vec![
        ResultItem {
            id: "wasm:jwt:payload".into(),
            provider_id: "dev.tomoyo.example-wasm".into(),
            title: format!("JWT Payload (alg={alg}, sub={sub})"),
            subtitle: "Decoded safely inside WASM sandbox · Enter to copy pretty JSON".into(),
            category: ResultCategory::Plugin,
            score: 9_800,
            icon: IconSpec::Monogram("JWT".into()),
            badge: Some("WASM".into()),
            copy_text: Some(payload_pretty.clone()),
            actions: vec![ActionSpec {
                id: "copy_payload".into(),
                label: "Copy JWT Payload JSON".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText {
                    text: payload_pretty.clone(),
                    paste_immediately: false,
                },
            }],
            preview: Some(PreviewSpec::Text {
                title: format!("JWT Payload ({alg})"),
                body: payload_pretty,
            }),
        },
        ResultItem {
            id: "wasm:jwt:header".into(),
            provider_id: "dev.tomoyo.example-wasm".into(),
            title: format!("JWT Header ({alg})"),
            subtitle: "Enter to copy decoded JWT header JSON".into(),
            category: ResultCategory::Plugin,
            score: 9_700,
            icon: IconSpec::Monogram("JWT".into()),
            badge: Some("HEADER".into()),
            copy_text: Some(header_pretty.clone()),
            actions: vec![ActionSpec {
                id: "copy_header".into(),
                label: "Copy JWT Header JSON".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText {
                    text: header_pretty.clone(),
                    paste_immediately: false,
                },
            }],
            preview: Some(PreviewSpec::Text {
                title: "JWT Header".into(),
                body: header_pretty,
            }),
        },
    ])
}

fn convert_byte_units(input: &str) -> Option<Vec<ResultItem>> {
    let s = input.trim().to_ascii_lowercase().replace(' ', "");
    let split_idx = s.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let num: f64 = s[..split_idx].parse().ok()?;
    let unit = &s[split_idx..];

    let multiplier: f64 = match unit {
        "b" | "byte" | "bytes" => 1.0,
        "kb" | "k" | "kib" => 1024.0,
        "mb" | "m" | "mib" => 1024.0 * 1024.0,
        "gb" | "g" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "tb" | "t" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };

    let bytes = num * multiplier;
    let kb = bytes / 1024.0;
    let mb = kb / 1024.0;
    let gb = mb / 1024.0;
    let tb = gb / 1024.0;

    let summary = format!("{:.0} B · {:.2} KB · {:.2} MB · {:.4} GB", bytes, kb, mb, gb);
    Some(vec![ResultItem {
        id: format!("wasm:unit:{s}"),
        provider_id: "dev.tomoyo.example-wasm".into(),
        title: format!("{input} = {:.2} MB ({:.4} GB)", mb, gb),
        subtitle: format!("{:.0} bytes · {:.2} KB · Enter to copy", bytes, kb),
        category: ResultCategory::Calc,
        score: 9_750,
        icon: IconSpec::Monogram("WSM".into()),
        badge: Some("WASM".into()),
        copy_text: Some(summary),
        actions: vec![ActionSpec {
            id: "copy_bytes".into(),
            label: "Copy Exact Bytes".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::CopyText {
                text: format!("{:.0}", bytes),
                paste_immediately: false,
            },
        }],
        preview: Some(PreviewSpec::RichCard {
            title: format!("Unit Conversion ({input})"),
            subtitle: "Binary IEC/JEDEC storage conversion".into(),
            badges: vec!["WASM".into(), "CALC".into()],
            rows: vec![
                ("Bytes".into(), format!("{:.0} B", bytes)),
                ("KiB / KB".into(), format!("{:.2} KB", kb)),
                ("MiB / MB".into(), format!("{:.2} MB", mb)),
                ("GiB / GB".into(), format!("{:.4} GB", gb)),
                ("TiB / TB".into(), format!("{:.6} TB", tb)),
            ],
            body: String::new(),
        }),
    }])
}

export_tomoyo_wasm_plugin!(WasmToolkit);
