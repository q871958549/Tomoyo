//! Ready-to-clone Tomoyo WASM ABI v2 Starter Plugin.
//!
//! Build:  `cargo build --target wasm32-unknown-unknown --release`
//! Test:   `cargo test`
//! Debug:  `tomoyo-cli dev . --query "hello"`

use tomoyo_wasm_sdk::{
    export_tomoyo_wasm_plugin, host, ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse,
    IconSpec, PreviewSpec, QueryContext, ResultCategory, ResultItem, TomoyoWasmPlugin,
};

const PLUGIN_ID: &str = "community.yourname.starter";

struct StarterPlugin;

impl TomoyoWasmPlugin for StarterPlugin {
    fn query(ctx: &QueryContext) -> Vec<ResultItem> {
        let arg = ctx.args.trim();
        let now = host::now_unix();
        let runs = host::kv_get("runs").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + 1;
        let _ = host::kv_set("runs", &runs.to_string());

        let text = if arg.is_empty() { "Hello, Tomoyo WASM!" } else { arg };
        let reversed: String = text.chars().rev().collect();

        vec![ResultItem {
            id: "starter:main".into(),
            provider_id: PLUGIN_ID.into(),
            title: format!("Echo: {text}"),
            subtitle: format!("Reversed: {reversed} · Calls: {runs} · Enter to copy"),
            category: ResultCategory::Plugin,
            score: 9_800,
            icon: IconSpec::Monogram("WSM".into()),
            badge: Some("WASM".into()),
            copy_text: Some(reversed.clone()),
            actions: vec![
                ActionSpec {
                    id: "copy".into(),
                    label: "Copy Reversed Text".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::CopyText { text: reversed.clone(), paste_immediately: false },
                },
                ActionSpec {
                    id: "ping".into(),
                    label: "Run Plugin Execute Toast".into(),
                    shortcut: Some("Ctrl+Enter".into()),
                    kind: ActionKind::PluginExecute {
                        plugin_id: PLUGIN_ID.into(),
                        action_id: "ping".into(),
                        payload: serde_json::json!({ "text": text }),
                    },
                },
            ],
            preview: Some(PreviewSpec::RichCard {
                title: "Tomoyo WASM Starter Card".into(),
                subtitle: "Rendered natively with 0 MB WebView overhead".into(),
                badges: vec!["WASM ABI v2".into(), "16 MB Cap".into()],
                rows: vec![
                    ("Input".into(), text.into()),
                    ("Reversed".into(), reversed),
                    ("Host Unix Clock".into(), now.to_string()),
                    ("KV Call Counter".into(), runs.to_string()),
                ],
                body: "Edit src/lib.rs, run `cargo build --target wasm32-unknown-unknown --release`, and test with `tomoyo-cli dev .`.".into(),
            }),
        }]
    }

    fn execute(req: &ExecuteRequest) -> ExecuteResponse {
        if req.action_id == "ping" {
            let txt = req.payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
            return ExecuteResponse {
                hide_window: false,
                toast: Some(format!("✓ WASM execute handled: {txt}")),
                ..Default::default()
            };
        }
        ExecuteResponse::default()
    }
}

export_tomoyo_wasm_plugin!(StarterPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverses_input_and_handles_execute() {
        let mut ctx = QueryContext::new("demo tomoyo", 1, 10);
        ctx.keyword = Some("demo".into());
        ctx.args = "tomoyo".into();
        let items = StarterPlugin::query(&ctx);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].copy_text.as_deref(), Some("oyomot"));
    }
}
