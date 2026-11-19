use tomoyo_sdk::{
    ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse, IconSpec, PluginMeta, QueryContext,
    ResultCategory, ResultItem, TomoyoPlugin,
};

struct HelloCasePlugin;

impl TomoyoPlugin for HelloCasePlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("Rust Hello & Case Converter", "0.1.0")
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let input = ctx.args.trim();
        let sample = if input.is_empty() {
            "Hello Tomoyo Open Source!"
        } else {
            input
        };

        let upper = sample.to_uppercase();
        let lower = sample.to_lowercase();
        let snake = to_snake_case(sample);
        let kebab = snake.replace('_', "-");
        let reversed: String = sample.chars().rev().collect();

        vec![
            make_item("upper", &upper, "UPPERCASE · Rust SDK Plugin", 9_500),
            make_item("lower", &lower, "lowercase · Rust SDK Plugin", 9_400),
            make_item("snake", &snake, "snake_case · Rust SDK Plugin", 9_300),
            make_item("kebab", &kebab, "kebab-case · Rust SDK Plugin", 9_200),
            make_item("rev", &reversed, "Reversed · Rust SDK Plugin", 9_100),
        ]
    }

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        let value = req
            .payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        ExecuteResponse {
            hide_window: true,
            toast: Some(format!("Copied via Rust plugin: {value}")),
            copy_text: Some(value),
            ..Default::default()
        }
    }
}

fn make_item(id: &str, text: &str, subtitle: &str, score: i64) -> ResultItem {
    ResultItem {
        id: format!("hello:{id}"),
        provider_id: "dev.tomoyo.example-hello".into(),
        title: text.into(),
        subtitle: subtitle.into(),
        category: ResultCategory::Plugin,
        score,
        icon: IconSpec::Monogram("RS".into()),
        badge: Some("RUST EXT".into()),
        copy_text: Some(text.into()),
        actions: vec![ActionSpec {
            id: "copy_via_plugin".into(),
            label: "Copy Result".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: "dev.tomoyo.example-hello".into(),
                action_id: id.into(),
                payload: serde_json::json!({ "text": text }),
            },
        }],
        preview: None,
    }
}

fn to_snake_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 4);
    let mut prev_underscore = true;
    for ch in input.chars() {
        if ch.is_alphanumeric() {
            if ch.is_uppercase() && !prev_underscore {
                out.push('_');
            }
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn main() {
    tomoyo_sdk::run(HelloCasePlugin);
}
