//! Bounded custom Quick Scripts & Code Snippets provider (`snip <q>` + global fuzzy/Pinyin).
//!
//! Reads `%LOCALAPPDATA%\Tomoyo\snippets.toml` (zero timers, reloads on `:reload`), expands
//! dynamic placeholders (`{{date}}`, `{{datetime}}`, `{{unix}}`, `{{uuid}}`, `{{clipboard}}`),
//! and pastes immediately or opens `snippets.toml` for fast editing.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tomoyo_core::{score_match, IndexedText, Lang, Provider, TomoyoConfig};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, PreviewSpec, QueryContext, ResultCategory, ResultItem,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetEntry {
    pub keyword: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnippetsFile {
    #[serde(default)]
    snippets: Vec<SnippetEntry>,
}

struct IndexedSnippet {
    entry: SnippetEntry,
    indexed: IndexedText,
}

pub struct SnippetsProvider {
    config: Arc<RwLock<TomoyoConfig>>,
    entries: RwLock<Vec<IndexedSnippet>>,
    path: PathBuf,
}

impl SnippetsProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        let path = tomoyo_core::sync_root_dir().join("snippets.toml");
        let entries = load_or_init_snippets(&path);
        Self {
            config,
            entries: RwLock::new(entries),
            path,
        }
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }
}

impl Provider for SnippetsProvider {
    fn id(&self) -> &'static str {
        "builtin.snippets"
    }

    fn name(&self) -> &'static str {
        "Quick Scripts & Snippets"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["snip"]
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let dedicated = ctx.keyword.is_some();
        let needle = if dedicated { ctx.args.trim() } else { ctx.trimmed.as_str() };
        if !dedicated && needle.chars().count() < 2 {
            return Vec::new();
        }
        let lang = self.lang();
        let clip_text = ctx
            .clipboard
            .as_ref()
            .map(|c| c.text.as_str())
            .unwrap_or("");
        let Ok(list) = self.entries.read() else {
            return Vec::new();
        };
        let path_str = self.path.to_string_lossy().into_owned();

        let mut out = Vec::new();
        for item in list.iter() {
            let score = if dedicated && needle.is_empty() {
                8_200
            } else if item.entry.keyword.eq_ignore_ascii_case(needle) {
                9_800
            } else {
                let Some(s) = score_match(needle, &item.indexed) else {
                    continue;
                };
                if dedicated { s + 1_200 } else { s }
            };
            let expanded = expand_template(&item.entry.template, clip_text);
            let single_line = expanded.replace(['\r', '\n'], " ");
            out.push(ResultItem {
                id: format!("snip:{}", item.entry.keyword),
                provider_id: self.id().into(),
                title: format!("{} · {}", item.entry.keyword, item.entry.title),
                subtitle: single_line.chars().take(90).collect(),
                category: ResultCategory::Plugin,
                score,
                icon: IconSpec::Monogram("SNP".into()),
                badge: Some("SNIPPET".into()),
                copy_text: Some(expanded.clone()),
                actions: vec![
                    ActionSpec {
                        id: "paste_snip".into(),
                        label: lang.pick("直接粘贴片段", "Paste Snippet").into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::CopyText {
                            text: expanded.clone(),
                            paste_immediately: true,
                        },
                    },
                    ActionSpec {
                        id: "copy_snip".into(),
                        label: lang.pick("仅复制到剪贴板", "Copy Only").into(),
                        shortcut: Some("Ctrl+Enter".into()),
                        kind: ActionKind::CopyText {
                            text: expanded.clone(),
                            paste_immediately: false,
                        },
                    },
                    ActionSpec {
                        id: "edit_snippets".into(),
                        label: lang.pick("编辑 snippets.toml", "Edit snippets.toml").into(),
                        shortcut: None,
                        kind: ActionKind::LaunchPath {
                            path: path_str.clone(),
                            args: None,
                            working_dir: None,
                        },
                    },
                ],
                preview: Some(PreviewSpec::Text {
                    title: format!("Snippet: {}", item.entry.title),
                    body: expanded,
                }),
            });
        }
        out.sort_by(|a, b| b.score.cmp(&a.score));
        out.truncate(ctx.limit.min(20));
        out
    }

    fn reload(&self) {
        let active_path = tomoyo_core::sync_root_dir().join("snippets.toml");
        let fresh = load_or_init_snippets(&active_path);
        if let Ok(mut guard) = self.entries.write() {
            *guard = fresh;
        }
    }
}

pub fn expand_template(tpl: &str, clipboard_text: &str) -> String {
    let now = Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let datetime = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let unix = now.timestamp().to_string();
    let mut out = tpl
        .replace("{{date}}", &date)
        .replace("{{datetime}}", &datetime)
        .replace("{{unix}}", &unix)
        .replace("{{clipboard}}", clipboard_text);
    if out.contains("{{uuid}}") {
        let id = uuid::Uuid::new_v4().to_string();
        out = out.replace("{{uuid}}", &id);
    }
    out
}

fn load_or_init_snippets(path: &Path) -> Vec<IndexedSnippet> {
    if !path.is_file() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let defaults = default_snippets_toml();
        let _ = std::fs::write(path, &defaults);
    }
    let raw = std::fs::read_to_string(path).unwrap_or_else(|_| default_snippets_toml());
    let parsed: SnippetsFile = toml::from_str(&raw).unwrap_or(SnippetsFile {
        snippets: Vec::new(),
    });
    parsed
        .snippets
        .into_iter()
        .take(500)
        .map(|entry| {
            let corpus = format!("{} {} {}", entry.keyword, entry.title, entry.tags.join(" "));
            IndexedSnippet {
                indexed: IndexedText::new(&corpus),
                entry,
            }
        })
        .collect()
}

fn default_snippets_toml() -> String {
    r#"# Tomoyo Code Snippets & Quick Templates (`snip <keyword>` or global fuzzy search)
# Placeholders: {{date}}, {{datetime}}, {{unix}}, {{uuid}}, {{clipboard}}

[[snippets]]
keyword = "iso"
title = "当前日期时间 (ISO)"
tags = ["date", "time", "shijian"]
template = "{{datetime}}"

[[snippets]]
keyword = "today"
title = "今日日期 YYYY-MM-DD"
tags = ["riqi", "date"]
template = "{{date}}"

[[snippets]]
keyword = "uuid"
title = "随机生成 UUID v4"
tags = ["id", "guid"]
template = "{{uuid}}"

[[snippets]]
keyword = "stamp"
title = "当前 Unix 秒级时间戳"
tags = ["timestamp", "epoch"]
template = "{{unix}}"

[[snippets]]
keyword = "quote"
title = "Markdown 引用当前剪贴板内容"
tags = ["md", "markdown", "clipboard"]
template = "> {{clipboard}}"
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_dynamic_snippet_placeholders() {
        let rendered = expand_template("id={{uuid}} clip={{clipboard}}", "hello_tomoyo");
        assert!(rendered.contains("clip=hello_tomoyo"));
        assert!(!rendered.contains("{{uuid}}"));
        assert!(rendered.len() > 20);
    }
}
