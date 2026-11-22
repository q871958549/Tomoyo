//! Fluent, zero-overhead builders and settings accessors for third-party plugin authors.

use crate::{
    ActionKind, ActionSpec, ExecuteResponse, IconSpec, PreviewSpec, QueryContext, ResultCategory,
    ResultItem,
};
use serde_json::Value;

impl QueryContext {
    /// Returns trimmed query arguments after the trigger keyword, or `fallback` when empty.
    pub fn input_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        let t = self.args.trim();
        if t.is_empty() { fallback } else { t }
    }

    /// Returns trimmed clipboard text if present and non-empty.
    pub fn clipboard_text(&self) -> Option<&str> {
        self.clipboard
            .as_ref()
            .map(|c| c.text.trim())
            .filter(|s| !s.is_empty())
    }

    /// Reads a Protocol v2 string setting from `plugin_settings`.
    pub fn setting_str(&self, key: &str) -> Option<&str> {
        self.plugin_settings.get(key).and_then(Value::as_str)
    }

    /// Reads a Protocol v2 boolean setting from `plugin_settings` with a default fallback.
    pub fn setting_bool(&self, key: &str, default: bool) -> bool {
        self.plugin_settings
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    }

    /// Reads a Protocol v2 integer setting from `plugin_settings` with a default fallback.
    pub fn setting_i64(&self, key: &str, default: i64) -> i64 {
        self.plugin_settings
            .get(key)
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64)))
            .unwrap_or(default)
    }

    /// Reads a Protocol v2 numeric setting as `f64` with a default fallback.
    pub fn setting_f64(&self, key: &str, default: f64) -> f64 {
        self.plugin_settings
            .get(key)
            .and_then(Value::as_f64)
            .unwrap_or(default)
    }
}

impl ResultItem {
    /// Creates a standard plugin result row with sensible defaults (`ResultCategory::Plugin`, score 9500).
    pub fn plugin(
        id: impl Into<String>,
        provider_id: impl Into<String>,
        title: impl Into<String>,
        subtitle: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            provider_id: provider_id.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            category: ResultCategory::Plugin,
            score: 9_500,
            icon: IconSpec::Monogram("EXT".into()),
            badge: None,
            copy_text: None,
            actions: Vec::new(),
            preview: None,
        }
    }

    pub fn with_score(mut self, score: i64) -> Self {
        self.score = score;
        self
    }

    pub fn with_icon(mut self, icon: IconSpec) -> Self {
        self.icon = icon;
        self
    }

    pub fn with_monogram(mut self, monogram: impl Into<String>) -> Self {
        self.icon = IconSpec::Monogram(monogram.into());
        self
    }

    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn with_copy(mut self, text: impl Into<String>) -> Self {
        self.copy_text = Some(text.into());
        self
    }

    pub fn with_action(mut self, action: ActionSpec) -> Self {
        self.actions.push(action);
        self
    }

    pub fn with_preview(mut self, preview: PreviewSpec) -> Self {
        self.preview = Some(preview);
        self
    }

    pub fn with_text_preview(mut self, title: impl Into<String>, body: impl Into<String>) -> Self {
        self.preview = Some(PreviewSpec::Text {
            title: title.into(),
            body: body.into(),
        });
        self
    }

    pub fn with_kv_preview(
        mut self,
        title: impl Into<String>,
        entries: Vec<(String, String)>,
    ) -> Self {
        self.preview = Some(PreviewSpec::KeyValue {
            title: title.into(),
            entries,
        });
        self
    }
}

impl ActionSpec {
    pub fn copy(text: impl Into<String>) -> Self {
        Self {
            id: "copy".into(),
            label: "复制".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::CopyText {
                text: text.into(),
                paste_immediately: false,
            },
        }
    }

    pub fn copy_and_paste(text: impl Into<String>) -> Self {
        Self {
            id: "paste".into(),
            label: "粘贴到当前窗口".into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::CopyText {
                text: text.into(),
                paste_immediately: true,
            },
        }
    }

    pub fn open_url(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: "open_url".into(),
            label: label.into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::OpenUrl { url: url.into() },
        }
    }

    pub fn fill_query(label: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            id: "fill_query".into(),
            label: label.into(),
            shortcut: None,
            kind: ActionKind::FillQuery {
                query: query.into(),
            },
        }
    }

    pub fn plugin_exec(
        plugin_id: impl Into<String>,
        action_id: impl Into<String>,
        label: impl Into<String>,
        payload: Value,
    ) -> Self {
        let act = action_id.into();
        Self {
            id: act.clone(),
            label: label.into(),
            shortcut: None,
            kind: ActionKind::PluginExecute {
                plugin_id: plugin_id.into(),
                action_id: act,
                payload,
            },
        }
    }
}

impl ExecuteResponse {
    pub fn toast(msg: impl Into<String>) -> Self {
        Self {
            hide_window: false,
            toast: Some(msg.into()),
            ..Default::default()
        }
    }

    pub fn copy(text: impl Into<String>, toast: impl Into<String>) -> Self {
        Self {
            hide_window: true,
            copy_text: Some(text.into()),
            toast: Some(toast.into()),
            ..Default::default()
        }
    }

    pub fn fill(query: impl Into<String>) -> Self {
        Self {
            hide_window: false,
            fill_query: Some(query.into()),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClipboardSnapshot;

    #[test]
    fn builders_and_settings_helpers_work_cleanly() {
        let mut ctx = QueryContext::new("calc  ", 1, 10).with_keyword("calc", "  ");
        ctx.clipboard = Some(ClipboardSnapshot {
            text: " 42 ".into(),
            captured_at_unix: 1,
        });
        ctx.plugin_settings.insert("mode".into(), serde_json::json!("deg"));
        ctx.plugin_settings.insert("prec".into(), serde_json::json!(6));
        ctx.plugin_settings.insert("sci".into(), serde_json::json!(true));

        assert_eq!(ctx.input_or("default"), "default");
        assert_eq!(ctx.clipboard_text(), Some("42"));
        assert_eq!(ctx.setting_str("mode"), Some("deg"));
        assert_eq!(ctx.setting_i64("prec", 2), 6);
        assert!(ctx.setting_bool("sci", false));

        let item = ResultItem::plugin("calc:1", "dev.example.calc", "42", "Decimal")
            .with_monogram("=")
            .with_badge("CALC")
            .with_copy("42")
            .with_action(ActionSpec::copy("42"))
            .with_kv_preview("Result", vec![("Dec".into(), "42".into())]);
        assert_eq!(item.actions.len(), 1);
        assert_eq!(item.badge.as_deref(), Some("CALC"));
        assert!(matches!(item.preview, Some(PreviewSpec::KeyValue { .. })));

        let resp = ExecuteResponse::copy("42", "Copied 42");
        assert!(resp.hide_window);
        assert_eq!(resp.copy_text.as_deref(), Some("42"));
    }
}
