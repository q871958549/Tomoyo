//! Official standalone optional Tomoyo plugin: Local Markdown Notes (`dev.tomoyo.notes`).
//!
//! Zero core bloat: runs purely as an on-demand STDIO JSON-RPC child process (0 MB idle RAM)
//! storing open, human-readable `.md` files under `%LOCALAPPDATA%\Tomoyo\notes\` and exposes
//! Protocol v3 `[[text_actions]]` for one-click note capture from Tomoyo Super Panel.

mod store;

use std::fs;
use std::path::PathBuf;
use store::{
    create_note_file, default_tag_from_settings, load_notes_from, notes_dir,
    notes_dir_from_settings, parse_create_input, NoteDoc,
};
use tomoyo_sdk::{
    ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse, IconSpec, PluginMeta, PreviewSpec,
    QueryContext, ResultCategory, ResultItem, TomoyoPlugin,
};

const PLUGIN_ID: &str = "dev.tomoyo.notes";

struct NotesPlugin;

fn max_results_from(ctx: &QueryContext) -> usize {
    ctx.plugin_settings
        .get("max_results")
        .and_then(|v| v.as_u64())
        .map(|n| (n as usize).clamp(5, 30))
        .unwrap_or(15)
        .min(ctx.limit.max(1))
}

fn build_note_item(doc: &NoteDoc, score: i64) -> ResultItem {
    let path_str = doc.path.to_string_lossy().into_owned();
    let tag_str = if doc.tags.is_empty() { String::new() } else { format!(" · {}", doc.tags.join(" ")) };
    ResultItem {
        id: format!("note:{}", doc.file_name),
        provider_id: PLUGIN_ID.into(),
        title: doc.title.clone(),
        subtitle: format!("{}{}", doc.snippet, tag_str),
        category: ResultCategory::Plugin,
        score,
        icon: IconSpec::Monogram("MD".into()),
        badge: Some("NOTE".into()),
        copy_text: Some(doc.body.clone()),
        actions: vec![
            ActionSpec {
                id: "copy".into(),
                label: "复制备忘内容".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText { text: doc.body.clone(), paste_immediately: false },
            },
            ActionSpec {
                id: "edit_in_md".into(),
                label: "交付 Quiet.md / Markdown 编辑器精编".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::FillQuery { query: format!("md \"{path_str}\"") },
            },
            ActionSpec {
                id: "open_system".into(),
                label: "用系统默认关联程序打开".into(),
                shortcut: None,
                kind: ActionKind::LaunchPath { path: path_str.clone(), args: None, working_dir: None },
            },
            ActionSpec {
                id: "reveal".into(),
                label: "在资源管理器中定位".into(),
                shortcut: Some("Shift+Enter".into()),
                kind: ActionKind::RevealInExplorer { path: path_str.clone() },
            },
            ActionSpec {
                id: "delete".into(),
                label: "删除该备忘录".into(),
                shortcut: None,
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "delete".into(),
                    payload: serde_json::json!({ "path": path_str }),
                },
            },
        ],
        preview: Some(PreviewSpec::Text { title: doc.title.clone(), body: doc.body.clone() }),
    }
}

impl TomoyoPlugin for NotesPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("本地快速备忘录", env!("CARGO_PKG_VERSION"))
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let dir = notes_dir_from_settings(&ctx.plugin_settings);
        let dir_payload = dir.to_string_lossy().into_owned();
        let def_tag = default_tag_from_settings(&ctx.plugin_settings);
        let args = ctx.args.trim();
        let mut out = Vec::new();

        if let Some((title, mut content)) = parse_create_input(args) {
            if !def_tag.is_empty() && !content.contains(&def_tag) {
                content = if content.is_empty() { def_tag.clone() } else { format!("{content}\n\n{def_tag}") };
            }
            out.push(ResultItem {
                id: "note:create".into(),
                provider_id: PLUGIN_ID.into(),
                title: format!("新建备忘录：{title}"),
                subtitle: format!("保存为纯本地 Markdown ({}) · 回车立即创建", dir.display()),
                category: ResultCategory::Plugin,
                score: 9_950,
                icon: IconSpec::Monogram("+".into()),
                badge: Some("NEW".into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "create".into(),
                    label: "创建并复制内容".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::PluginExecute {
                        plugin_id: PLUGIN_ID.into(),
                        action_id: "create".into(),
                        payload: serde_json::json!({ "title": title, "content": content, "dir": dir_payload }),
                    },
                }],
                preview: Some(PreviewSpec::Text {
                    title: format!("即将创建：{title}"),
                    body: format!("# {title}\n\n{content}"),
                }),
            });
            return out;
        }

        if args.eq_ignore_ascii_case("clip")
            && let Some(clip) = ctx.clipboard.as_ref().filter(|c| !c.text.trim().is_empty())
        {
            let title: String = clip.text.lines().next().unwrap_or("剪贴板笔记").trim().chars().take(32).collect();
            let body = if !def_tag.is_empty() && !clip.text.contains(&def_tag) {
                format!("{}\n\n{def_tag}", clip.text)
            } else {
                clip.text.clone()
            };
            out.push(ResultItem {
                id: "note:from_clip".into(),
                provider_id: PLUGIN_ID.into(),
                title: format!("将剪贴板保存为备忘：{title}"),
                subtitle: "按回车将当前剪贴板文本落盘为本地 Markdown".into(),
                category: ResultCategory::Plugin,
                score: 9_900,
                icon: IconSpec::Monogram("CB".into()),
                badge: Some("CLIP→MD".into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "create".into(),
                    label: "保存为本地 .md 备忘".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::PluginExecute {
                        plugin_id: PLUGIN_ID.into(),
                        action_id: "create".into(),
                        payload: serde_json::json!({ "title": title, "content": body, "dir": dir_payload }),
                    },
                }],
                preview: Some(PreviewSpec::Text { title, body }),
            });
            return out;
        }

        let docs = load_notes_from(&dir);
        let tokens: Vec<String> = args.split_whitespace().map(|t| t.to_ascii_lowercase()).collect();
        let mut hits: Vec<(i64, &NoteDoc)> = docs
            .iter()
            .filter_map(|d| {
                if tokens.is_empty() {
                    return Some((8_000, d));
                }
                let title_low = d.title.to_ascii_lowercase();
                let body_low = d.body.to_ascii_lowercase();
                if tokens.iter().all(|t| title_low.contains(t) || body_low.contains(t)) {
                    let boost = if tokens.iter().all(|t| title_low.contains(t)) { 9_400 } else { 8_600 };
                    Some((boost, d))
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.modified_secs.cmp(&a.1.modified_secs)));

        for (i, (score, doc)) in hits.into_iter().take(max_results_from(ctx)).enumerate() {
            out.push(build_note_item(doc, score - i as i64));
        }

        if out.is_empty() {
            let dir_str = dir.to_string_lossy().into_owned();
            let quick_content = if def_tag.is_empty() { args.to_string() } else { format!("{args}\n\n{def_tag}") };
            out.push(ResultItem {
                id: "note:hint".into(),
                provider_id: PLUGIN_ID.into(),
                title: if args.is_empty() { "尚无本地备忘录 · 输入 `note + 标题 :: 内容` 新建".into() } else { format!("未找到「{args}」· 按回车直接新建为备忘录") },
                subtitle: format!("存储目录：{dir_str}（支持 `note clip` 一键转存剪贴板）"),
                category: ResultCategory::Plugin,
                score: 8_500,
                icon: IconSpec::Monogram("MD".into()),
                badge: Some("NOTES".into()),
                copy_text: None,
                actions: if args.is_empty() {
                    vec![ActionSpec {
                        id: "open_dir".into(),
                        label: "打开备忘录目录".into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::LaunchPath { path: dir_str, args: None, working_dir: None },
                    }]
                } else {
                    vec![ActionSpec {
                        id: "create_quick".into(),
                        label: "立即创建为本地备忘录".into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::PluginExecute {
                            plugin_id: PLUGIN_ID.into(),
                            action_id: "create".into(),
                            payload: serde_json::json!({ "title": args, "content": quick_content, "dir": dir_str }),
                        },
                    }]
                },
                preview: None,
            });
        }
        out
    }

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        match req.action_id.as_str() {
            "save_selection_note" => {
                let raw = req.payload.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
                if raw.is_empty() {
                    return ExecuteResponse { hide_window: true, toast: Some("未检测到可保存的文本".into()), ..Default::default() };
                }
                let dir = notes_dir_from_settings(&req.plugin_settings);
                let def_tag = default_tag_from_settings(&req.plugin_settings);
                let title: String = raw
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("划词备忘")
                    .trim()
                    .chars()
                    .take(32)
                    .collect();
                let content = if !def_tag.is_empty() && !raw.contains(&def_tag) {
                    format!("{raw}\n\n{def_tag}")
                } else {
                    raw.to_string()
                };
                match create_note_file(&dir, &title, &content) {
                    Ok(_) => ExecuteResponse {
                        hide_window: true,
                        toast: Some(format!("✓ 已一键存为本地备忘：{title}")),
                        ..Default::default()
                    },
                    Err(e) => ExecuteResponse { hide_window: true, toast: Some(e), ..Default::default() },
                }
            }
            "create" => {
                let title = req.payload.get("title").and_then(|v| v.as_str()).unwrap_or("未命名备忘");
                let content = req.payload.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let target_dir = req
                    .payload
                    .get("dir")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(notes_dir);
                match create_note_file(&target_dir, title, content) {
                    Ok(p) => ExecuteResponse {
                        hide_window: false,
                        toast: Some(format!("已保存本地 Markdown 备忘：{}", p.file_name().unwrap_or_default().to_string_lossy())),
                        fill_query: Some("note ".into()),
                        ..Default::default()
                    },
                    Err(e) => ExecuteResponse { hide_window: false, toast: Some(e), ..Default::default() },
                }
            }
            "delete" => {
                if let Some(p) = req.payload.get("path").and_then(|v| v.as_str()) {
                    let path = PathBuf::from(p);
                    let is_md = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("txt"));
                    if is_md && fs::remove_file(&path).is_ok() {
                        return ExecuteResponse {
                            hide_window: false,
                            toast: Some("已删除本地备忘录".into()),
                            fill_query: Some("note ".into()),
                            ..Default::default()
                        };
                    }
                }
                ExecuteResponse { hide_window: false, toast: Some("删除备忘录失败".into()), ..Default::default() }
            }
            _ => ExecuteResponse::default(),
        }
    }
}

fn main() {
    tomoyo_sdk::run(NotesPlugin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_searches_and_saves_selection_notes() {
        let dir = std::env::temp_dir().join(format!("tomoyo_notes_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let p = create_note_file(&dir, "架构备忘", "坚持微内核 #rust #tomoyo").unwrap();
        assert!(p.exists());
        let docs = load_notes_from(&dir);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].title, "架构备忘");
        assert!(docs[0].tags.contains(&"#rust".to_string()));
        assert_eq!(parse_create_input("+ 会议纪要 :: 明天上线"), Some(("会议纪要".into(), "明天上线".into())));

        let mut settings = std::collections::BTreeMap::new();
        settings.insert("notes_dir".into(), serde_json::json!(dir.to_string_lossy()));
        settings.insert("default_tag".into(), serde_json::json!("inbox"));
        let resp = NotesPlugin.execute(&ExecuteRequest {
            action_id: "save_selection_note".into(),
            item_id: String::new(),
            payload: serde_json::json!({ "text": "Super Panel 一键存备忘测试" }),
            plugin_settings: settings,
            plugin_secrets: Default::default(),
        });
        assert!(resp.toast.as_deref().unwrap_or("").contains("已一键存为本地备忘"));
        assert_eq!(load_notes_from(&dir).len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }
}
