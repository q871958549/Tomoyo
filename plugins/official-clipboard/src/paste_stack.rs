//! FIFO Sequential Paste Stack (`cb :stack`) for CopyQ/Raycast-style multi-field form filling.
//!
//! Users enqueue items (via Tab menu or `stack_load_recent`) and pop them sequentially via
//! `cb :stack` or an opt-in global hotkey (`paste_stack_next`). Bounded to 32 entries.

use crate::config::tomoyo_data_dir;
use crate::db_io::{make_preview_line, truncate_utf8};
use crate::store::{ClipboardPayload, ClipboardStore, MAX_ITEM_BYTES};
use crate::PLUGIN_ID;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use tomoyo_sdk::{
    ActionKind, ActionSpec, ExecuteResponse, IconSpec, PreviewSpec, ResultCategory, ResultItem,
};

const MAX_STACK_ITEMS: usize = 32;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PasteStack {
    items: VecDeque<String>,
}

impl PasteStack {
    pub fn default_path() -> PathBuf {
        tomoyo_data_dir().join("clipboard_stack.json")
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let mut s: Self = serde_json::from_str(&raw).unwrap_or_default();
        while s.items.len() > MAX_STACK_ITEMS {
            s.items.pop_back();
        }
        s
    }

    pub fn save_to(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn push(&mut self, text: &str) -> usize {
        if text.trim().is_empty() {
            return self.items.len();
        }
        let clipped = truncate_utf8(text, MAX_ITEM_BYTES);
        if self.items.len() >= MAX_STACK_ITEMS {
            self.items.pop_front();
        }
        self.items.push_back(clipped);
        self.items.len()
    }

    pub fn load_batch(&mut self, batch: Vec<String>) -> usize {
        self.items.clear();
        for text in batch.into_iter().take(MAX_STACK_ITEMS) {
            if !text.trim().is_empty() {
                self.items.push_back(truncate_utf8(&text, MAX_ITEM_BYTES));
            }
        }
        self.items.len()
    }

    pub fn pop_front(&mut self) -> Option<(String, usize)> {
        let front = self.items.pop_front()?;
        Some((front, self.items.len()))
    }

    pub fn clear(&mut self) -> usize {
        let n = self.items.len();
        self.items.clear();
        n
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

pub fn enqueue_action(text: &str) -> ActionSpec {
    ActionSpec {
        id: "stack_push".into(),
        label: "加入顺序粘贴队列 (Paste Stack)".into(),
        shortcut: None,
        kind: ActionKind::PluginExecute {
            plugin_id: PLUGIN_ID.into(),
            action_id: "stack_push".into(),
            payload: json!({ "text": text }),
        },
    }
}

pub fn build_stack_command_item() -> ResultItem {
    let count = PasteStack::load_from(&PasteStack::default_path()).len();
    ResultItem {
        id: "cb:cmd:stack".into(),
        provider_id: PLUGIN_ID.into(),
        title: format!("cb :stack — 顺序粘贴队列 Paste Stack（当前 {count} 项）"),
        subtitle: "按入队顺序逐项弹出并复制，适合填表/批量粘贴（可在插件热键中绑定全局弹出快捷键）".into(),
        category: ResultCategory::Clipboard,
        score: 9_880,
        icon: IconSpec::Monogram("STK".into()),
        badge: Some(format!("{count} 项")),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "fill_stack".into(),
            label: "查看顺序粘贴队列".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::FillQuery { query: "cb :stack".into() },
        }],
        preview: None,
    }
}

pub fn build_stack_items(path: &Path) -> Vec<ResultItem> {
    let stack = PasteStack::load_from(path);
    let mut out = Vec::new();
    let total = stack.items.len();

    if total == 0 {
        out.push(ResultItem {
            id: "cb:stack:empty".into(),
            provider_id: PLUGIN_ID.into(),
            title: "顺序粘贴队列为空 — 回车将最近 5 条剪贴板文本装入队列".into(),
            subtitle: "也可在任意剪贴板记录按 Tab →「加入顺序粘贴队列」，或配置全局弹出快捷键".into(),
            category: ResultCategory::Clipboard,
            score: 9_900,
            icon: IconSpec::Monogram("STK".into()),
            badge: Some("0/32".into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "stack_load_recent".into(),
                label: "装入最近 5 条剪贴板文本（按复制先后顺序）".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "stack_load_recent".into(),
                    payload: json!({ "count": 5 }),
                },
            }],
            preview: None,
        });
        return out;
    }

    for (idx, text) in stack.items.iter().enumerate() {
        let preview_line = make_preview_line(text);
        let is_head = idx == 0;
        let actions = if is_head {
            vec![
                ActionSpec {
                    id: "stack_pop_copy".into(),
                    label: "弹出队首并复制到剪贴板".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::PluginExecute {
                        plugin_id: PLUGIN_ID.into(),
                        action_id: "stack_pop_copy".into(),
                        payload: json!({}),
                    },
                },
                ActionSpec {
                    id: "paste_keep".into(),
                    label: "直接粘贴（保留在队列中）".into(),
                    shortcut: Some("Ctrl+Enter".into()),
                    kind: ActionKind::CopyText { text: text.clone(), paste_immediately: true },
                },
            ]
        } else {
            vec![ActionSpec {
                id: "paste_item".into(),
                label: "直接粘贴该项".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText { text: text.clone(), paste_immediately: true },
            }]
        };
        out.push(ResultItem {
            id: format!("cb:stack:{idx}"),
            provider_id: PLUGIN_ID.into(),
            title: format!("#{}/{} {preview_line}", idx + 1, total),
            subtitle: if is_head {
                "队首待弹出 · Enter 弹出并复制（进入下一项） · Ctrl+Enter 直接粘贴且保留".into()
            } else {
                "排队中 · 按顺序等待弹出 · Enter 可直接粘贴该项".into()
            },
            category: ResultCategory::Clipboard,
            score: 9_900 - (idx as i64 * 10),
            icon: IconSpec::Monogram(if is_head { "▶".into() } else { format!("#{}", idx + 1) }),
            badge: Some(if is_head { "NEXT".into() } else { format!("#{}", idx + 1) }),
            copy_text: Some(text.clone()),
            actions,
            preview: Some(PreviewSpec::Text {
                title: format!("顺序粘贴队列 #{}/{}", idx + 1, total),
                body: text.clone(),
            }),
        });
    }

    out.push(ResultItem {
        id: "cb:stack:reload".into(),
        provider_id: PLUGIN_ID.into(),
        title: "重新装入最近 5 条剪贴板文本".into(),
        subtitle: "按从早到晚的复制顺序覆盖当前队列".into(),
        category: ResultCategory::Clipboard,
        score: 9_200,
        icon: IconSpec::Monogram("↻".into()),
        badge: Some("LOAD".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "stack_load_recent".into(),
            label: "装入最近 5 条文本".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "stack_load_recent".into(),
                payload: json!({ "count": 5 }),
            },
        }],
        preview: None,
    });
    out.push(ResultItem {
        id: "cb:stack:clear".into(),
        provider_id: PLUGIN_ID.into(),
        title: format!("清空顺序粘贴队列（共 {total} 项）"),
        subtitle: "仅清空粘贴队列，不影响剪贴板历史记录".into(),
        category: ResultCategory::Clipboard,
        score: 9_100,
        icon: IconSpec::Monogram("CLR".into()),
        badge: Some("CLEAR".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "stack_clear".into(),
            label: "清空队列".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "stack_clear".into(),
                payload: json!({}),
            },
        }],
        preview: None,
    });
    out
}

pub fn handle_stack_action(
    action_id: &str,
    payload: &Value,
    path: &Path,
    store: Option<&ClipboardStore>,
) -> Option<ExecuteResponse> {
    match action_id {
        "stack_push" => {
            let text = payload.get("text").and_then(Value::as_str).unwrap_or("");
            let mut stack = PasteStack::load_from(path);
            let len = stack.push(text);
            stack.save_to(path);
            Some(ExecuteResponse {
                hide_window: false,
                refresh_query: false,
                toast: Some(format!("✓ 已加入顺序粘贴队列（共 {len} 项，输入 cb :stack 查看）")),
                ..Default::default()
            })
        }
        "stack_pop_copy" => {
            let mut stack = PasteStack::load_from(path);
            let resp = match stack.pop_front() {
                Some((text, rem)) => {
                    stack.save_to(path);
                    ExecuteResponse {
                        hide_window: true,
                        copy_text: Some(text),
                        toast: Some(if rem == 0 {
                            "✓ 已弹出队列最后一项并复制（队列已清空）".into()
                        } else {
                            format!("✓ 已弹出队首并复制（队列还剩 {rem} 项）")
                        }),
                        ..Default::default()
                    }
                }
                None => ExecuteResponse {
                    hide_window: false,
                    toast: Some("顺序粘贴队列为空（输入 cb :stack 装入）".into()),
                    ..Default::default()
                },
            };
            Some(resp)
        }
        "stack_load_recent" => {
            let count = payload.get("count").and_then(Value::as_u64).unwrap_or(5).clamp(1, 20) as usize;
            let mut texts: Vec<String> = store
                .map(|s| {
                    let parsed = crate::filter::ParsedClipQuery::parse("@text");
                    s.query_filtered(&parsed, count)
                        .into_iter()
                        .filter_map(|e| match e.payload {
                            ClipboardPayload::Text { text, .. } => Some(text),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            texts.reverse(); // Chronological FIFO: earliest copied pastes first
            let mut stack = PasteStack::load_from(path);
            let loaded = stack.load_batch(texts);
            stack.save_to(path);
            Some(ExecuteResponse {
                hide_window: false,
                refresh_query: true,
                toast: Some(format!("✓ 已按先后顺序装入 {loaded} 条剪贴板文本")),
                ..Default::default()
            })
        }
        "stack_clear" => {
            let mut stack = PasteStack::load_from(path);
            let n = stack.clear();
            stack.save_to(path);
            Some(ExecuteResponse {
                hide_window: false,
                refresh_query: true,
                toast: Some(format!("✓ 已清空顺序粘贴队列（共移除 {n} 项）")),
                ..Default::default()
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_stack_fifo_lifecycle_and_persistence() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_stack_ut_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let mut st = PasteStack::default();
        assert_eq!(st.push("first-field"), 1);
        assert_eq!(st.push("second-field"), 2);
        st.save_to(&tmp);

        let mut loaded = PasteStack::load_from(&tmp);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.pop_front(), Some(("first-field".into(), 1)));
        assert_eq!(loaded.pop_front(), Some(("second-field".into(), 0)));
        assert_eq!(loaded.pop_front(), None);

        let _ = std::fs::remove_file(&tmp);
    }
}
