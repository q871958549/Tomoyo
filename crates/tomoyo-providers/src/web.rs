use std::sync::{Arc, RwLock};
use tomoyo_core::{Provider, TomoyoConfig};
use tomoyo_protocol::{ActionKind, ActionSpec, IconSpec, QueryContext, ResultCategory, ResultItem};

/// Config-driven quick web search provider (`g rust`, `gh tokio`, `crates wasmi`, `bd ...`, etc.).
pub struct WebSearchProvider {
    config: Arc<RwLock<TomoyoConfig>>,
}

impl WebSearchProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        Self { config }
    }
}

impl Provider for WebSearchProvider {
    fn id(&self) -> &'static str {
        "builtin.web"
    }

    fn name(&self) -> &'static str {
        "Web Search Engines"
    }

    fn matches_prefix(&self, token_lower: &str) -> bool {
        self.config
            .read()
            .map(|c| {
                c.web_searches
                    .iter()
                    .any(|e| e.keyword.eq_ignore_ascii_case(token_lower))
            })
            .unwrap_or(false)
    }

    fn supports_global(&self) -> bool {
        false
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let Some(kw) = ctx.keyword.as_deref() else {
            return Vec::new();
        };
        let Ok(cfg) = self.config.read() else {
            return Vec::new();
        };
        let Some(engine) = cfg
            .web_searches
            .iter()
            .find(|e| e.keyword.eq_ignore_ascii_case(kw))
        else {
            return Vec::new();
        };

        let term = ctx.args.trim();
        let encoded = url_encode(term);
        let url = engine.url.replace("{q}", &encoded);

        let title = if term.is_empty() {
            format!("Search {}...", engine.name)
        } else {
            format!("{}: {}", engine.name, term)
        };

        vec![ResultItem {
            id: format!("web:{}:{term}", engine.keyword),
            provider_id: self.id().into(),
            title,
            subtitle: url.clone(),
            category: ResultCategory::Web,
            score: 9_600,
            icon: IconSpec::Monogram(engine.keyword.to_uppercase().chars().take(3).collect()),
            badge: Some("WEB".into()),
            copy_text: Some(url.clone()),
            actions: vec![
                ActionSpec {
                    id: "open".into(),
                    label: format!("Open in Browser ({})", engine.name),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::OpenUrl { url: url.clone() },
                },
                ActionSpec {
                    id: "copy".into(),
                    label: "Copy Search URL".into(),
                    shortcut: Some("Ctrl+C".into()),
                    kind: ActionKind::CopyText {
                        text: url,
                        paste_immediately: false,
                    },
                },
            ],
            preview: None,
        }]
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            other => {
                use std::fmt::Write;
                let _ = write!(&mut out, "%{other:02X}");
            }
        }
    }
    out
}
