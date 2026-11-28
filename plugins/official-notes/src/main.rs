//! Official Tomoyo Protocol v3 Hybrid Plugin: Local-First Quick Notes Workbench (`dev.tomoyo.notes`).
//!
//! Zero core bloat: runs on-demand as a STDIO JSON-RPC process (0 MB idle RAM) backed by
//! human-readable `.md` + YAML Frontmatter files and an optional Svelte 5 standalone window.

mod frontmatter;
mod items;
mod ops;
mod rpc;
mod store;

use items::{
    build_cli_hint_item, build_clip_item, build_create_item, build_daily_item, build_note_item,
    build_open_workbench_item, build_quick_create_item,
};
use serde_json::Value;
use std::path::PathBuf;
use store::{
    archive_note, create_note_file_ex, default_tag_from_settings, delete_note_file,
    load_notes_with_archive, notes_dir_from_settings, open_or_create_daily_note, parse_create_input,
    toggle_pin_note, NoteDoc,
};
use tomoyo_sdk::{
    ExecuteRequest, ExecuteResponse, OpenWebViewRequest, PluginCallRequest, PluginMeta,
    QueryContext, ResultItem, TomoyoPlugin,
};

pub(crate) const PLUGIN_ID: &str = "dev.tomoyo.notes";

struct NotesPlugin;

fn max_results_from(ctx: &QueryContext) -> usize {
    ctx.plugin_settings
        .get("max_results")
        .and_then(Value::as_u64)
        .map(|n| (n as usize).clamp(5, 30))
        .unwrap_or(15)
        .min(ctx.limit.max(1))
}

fn workbench_response(focus_file: String) -> ExecuteResponse {
    ExecuteResponse {
        hide_window: true,
        open_webview: Some(OpenWebViewRequest {
            entry: None,
            title: Some("Tomoyo 本地备忘录工作台".into()),
            initial_query: focus_file,
            width: Some(1080),
            height: Some(680),
        }),
        ..Default::default()
    }
}

impl TomoyoPlugin for NotesPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("本地快速备忘录", env!("CARGO_PKG_VERSION"))
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let dir = notes_dir_from_settings(&ctx.plugin_settings);
        let def_tag = default_tag_from_settings(&ctx.plugin_settings);
        let args = ctx.args.trim();
        let mut out = Vec::new();

        if args.eq_ignore_ascii_case("today") || args.eq_ignore_ascii_case("daily") {
            out.push(build_daily_item());
            return out;
        }

        if let Some((title, mut content)) = parse_create_input(args) {
            if !def_tag.is_empty() && !content.contains(&def_tag) {
                content = if content.is_empty() {
                    def_tag.clone()
                } else {
                    format!("{content}\n\n{def_tag}")
                };
            }
            out.push(build_create_item(title, content, dir.display().to_string()));
            return out;
        }

        if args.eq_ignore_ascii_case("clip")
            && let Some(clip) = ctx.clipboard.as_ref().filter(|c| !c.text.trim().is_empty())
        {
            let title: String = clip
                .text
                .lines()
                .next()
                .unwrap_or("剪贴板笔记")
                .trim()
                .chars()
                .take(32)
                .collect();
            let body = if !def_tag.is_empty() && !clip.text.contains(&def_tag) {
                format!("{}\n\n{def_tag}", clip.text)
            } else {
                clip.text.clone()
            };
            out.push(build_clip_item(title, body));
            return out;
        }

        let want_archived = args.split_whitespace().any(|t| t.eq_ignore_ascii_case("@archived"));
        let want_pin = args.split_whitespace().any(|t| t.eq_ignore_ascii_case("@pin"));
        let want_todo = args.split_whitespace().any(|t| t.eq_ignore_ascii_case("@todo"));
        let tokens: Vec<String> = args
            .split_whitespace()
            .filter(|t| !matches!(t.to_ascii_lowercase().as_str(), "@archived" | "@pin" | "@todo"))
            .map(|t| t.to_ascii_lowercase())
            .collect();

        let docs = load_notes_with_archive(&dir, want_archived);
        let active_total = docs.iter().filter(|d| !d.archived).count();
        let total_open_todos: usize = docs.iter().filter(|d| !d.archived).map(|d| d.todo_open).sum();

        if args.is_empty() {
            out.push(build_open_workbench_item(active_total, total_open_todos, 9_960));
        }

        let mut hits: Vec<(i64, &NoteDoc)> = docs
            .iter()
            .filter(|d| if want_archived { d.archived } else { !d.archived })
            .filter(|d| !want_pin || d.pinned)
            .filter(|d| !want_todo || d.todo_open > 0)
            .filter_map(|d| {
                if tokens.is_empty() {
                    return Some((if d.pinned { 8_400 } else { 8_000 }, d));
                }
                let title_low = d.title.to_ascii_lowercase();
                let body_low = d.body.to_ascii_lowercase();
                if tokens.iter().all(|t| {
                    title_low.contains(t)
                        || body_low.contains(t)
                        || d.tags.iter().any(|tg| tg.to_ascii_lowercase().contains(t))
                }) {
                    let boost = if tokens.iter().all(|t| title_low.contains(t)) { 9_400 } else { 8_600 };
                    Some((boost + if d.pinned { 200 } else { 0 }, d))
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.modified_secs.cmp(&a.1.modified_secs)));

        for (i, (score, doc)) in hits.into_iter().take(max_results_from(ctx)).enumerate() {
            out.push(build_note_item(doc, score - i as i64));
        }

        if !args.is_empty() && out.is_empty() {
            let quick_content = if def_tag.is_empty() {
                args.to_string()
            } else {
                format!("{args}\n\n{def_tag}")
            };
            out.push(build_quick_create_item(args, quick_content, dir.display().to_string()));
        } else if args.is_empty() && active_total == 0 {
            out.push(build_cli_hint_item());
        }
        out
    }

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        let dir = notes_dir_from_settings(&req.plugin_settings);
        let def_tag = default_tag_from_settings(&req.plugin_settings);
        match req.action_id.as_str() {
            "open_workbench" => {
                let focus_file = if req.payload.get("daily").and_then(Value::as_bool) == Some(true) {
                    open_or_create_daily_note(&dir, &def_tag)
                        .map(|d| d.file_name)
                        .unwrap_or_default()
                } else {
                    req.payload
                        .get("focus_file")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                };
                workbench_response(focus_file)
            }
            "save_selection_note" => {
                let raw = req.payload.get("text").and_then(Value::as_str).unwrap_or("").trim();
                if raw.is_empty() {
                    return ExecuteResponse {
                        hide_window: true,
                        toast: Some("未检测到可保存的文本".into()),
                        ..Default::default()
                    };
                }
                let title: String = raw
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("划词备忘")
                    .trim()
                    .chars()
                    .take(32)
                    .collect();
                let tags = if def_tag.is_empty() { Vec::new() } else { vec![def_tag] };
                match create_note_file_ex(&dir, &title, raw, &tags, false, "super_panel") {
                    Ok(_) => ExecuteResponse {
                        hide_window: true,
                        toast: Some(format!("✓ 已一键存为本地备忘：{title}")),
                        ..Default::default()
                    },
                    Err(e) => ExecuteResponse { hide_window: true, toast: Some(e), ..Default::default() },
                }
            }
            "create" => {
                let title = req.payload.get("title").and_then(Value::as_str).unwrap_or("未命名备忘");
                let content = req.payload.get("content").and_then(Value::as_str).unwrap_or("");
                let target_dir = req
                    .payload
                    .get("dir")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dir.clone());
                match create_note_file_ex(&target_dir, title, content, &[], false, "launcher") {
                    Ok(doc) => ExecuteResponse {
                        hide_window: false,
                        toast: Some(format!("已保存本地 Markdown 备忘：{}", doc.file_name)),
                        fill_query: Some("note ".into()),
                        ..Default::default()
                    },
                    Err(e) => ExecuteResponse { hide_window: false, toast: Some(e), ..Default::default() },
                }
            }
            "toggle_pin" => {
                let file_name = req.payload.get("file_name").and_then(Value::as_str).unwrap_or("");
                match toggle_pin_note(&dir, file_name) {
                    Ok(pinned) => ExecuteResponse {
                        hide_window: false,
                        refresh_query: true,
                        toast: Some(if pinned { "★ 已置顶该备忘录".into() } else { "已取消置顶".into() }),
                        ..Default::default()
                    },
                    Err(e) => ExecuteResponse { hide_window: false, toast: Some(e), ..Default::default() },
                }
            }
            "archive" => {
                let file_name = req.payload.get("file_name").and_then(Value::as_str).unwrap_or("");
                match archive_note(&dir, file_name) {
                    Ok(()) => ExecuteResponse {
                        hide_window: false,
                        refresh_query: true,
                        toast: Some("已归档到 .archive/".into()),
                        ..Default::default()
                    },
                    Err(e) => ExecuteResponse { hide_window: false, toast: Some(e), ..Default::default() },
                }
            }
            "delete" => {
                let file_name = req.payload.get("file_name").and_then(Value::as_str).unwrap_or("");
                let archived = req.payload.get("archived").and_then(Value::as_bool).unwrap_or(false);
                match delete_note_file(&dir, file_name, archived) {
                    Ok(()) => ExecuteResponse {
                        hide_window: false,
                        refresh_query: true,
                        toast: Some("已永久删除本地备忘录".into()),
                        ..Default::default()
                    },
                    Err(e) => ExecuteResponse { hide_window: false, toast: Some(e), ..Default::default() },
                }
            }
            _ => ExecuteResponse::default(),
        }
    }

    fn call(&self, req: &PluginCallRequest) -> Result<Value, String> {
        rpc::handle_rpc(req)
    }
}

fn main() {
    tomoyo_sdk::run(NotesPlugin);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use store::{create_note_file, load_notes_from, save_note_file, toggle_todo_note};

    #[test]
    fn frontmatter_todos_backlinks_archive_and_rpc_work_end_to_end() {
        let dir = std::env::temp_dir().join(format!("tomoyo_notes_v2_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let p = create_note_file(&dir, "架构备忘", "- [ ] 实现独立工作台\n坚持微内核 #rust [[部署清单]]").unwrap();
        assert!(p.exists());
        let fname = p.file_name().unwrap().to_string_lossy().into_owned();

        let _p2 = create_note_file(&dir, "部署清单", "发布到 GitHub Releases").unwrap();
        let docs = load_notes_from(&dir);
        assert_eq!(docs.len(), 2);
        let linked = docs.iter().find(|d| d.title == "部署清单").unwrap();
        assert_eq!(linked.backlinks.len(), 1);
        assert_eq!(linked.backlinks[0].1, "架构备忘");

        let arch = docs.iter().find(|d| d.file_name == fname).unwrap();
        assert_eq!(arch.todo_open, 1);
        assert_eq!(arch.todo_done, 0);

        let toggled = toggle_todo_note(&dir, &fname, 2).unwrap();
        assert_eq!(toggled.todo_open, 0);
        assert_eq!(toggled.todo_done, 1);

        let mut settings = std::collections::BTreeMap::new();
        settings.insert("notes_dir".into(), serde_json::json!(dir.to_string_lossy()));
        settings.insert("default_tag".into(), serde_json::json!("inbox"));

        let list_res = NotesPlugin
            .call(&PluginCallRequest {
                method: "list".into(),
                params: serde_json::json!({ "filter": "all" }),
                plugin_settings: settings.clone(),
                plugin_secrets: Default::default(),
            })
            .unwrap();
        assert_eq!(list_res["total"], 2);

        let saved = save_note_file(&dir, &fname, "架构备忘 v2", "- [x] 完成\n[[部署清单]]", &["#v2".into()], true).unwrap();
        assert!(saved.pinned);
        assert!(saved.tags.iter().any(|t| t == "#v2"));
        assert!(store::safe_note_path(&dir, "../evil.md", false).is_err());

        archive_note(&dir, &fname).unwrap();
        assert_eq!(load_notes_from(&dir).len(), 1);
        assert_eq!(load_notes_with_archive(&dir, true).len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn query_empty_offers_workbench_and_selection_saves_note() {
        let dir = std::env::temp_dir().join(format!("tomoyo_notes_q_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut ctx = QueryContext::new("", 1, 15);
        ctx.plugin_settings.insert("notes_dir".into(), serde_json::json!(dir.to_string_lossy()));
        ctx.plugin_settings.insert("default_tag".into(), serde_json::json!("inbox"));

        let empty_hits = NotesPlugin.query(&ctx);
        assert_eq!(empty_hits[0].id, "note:workbench");

        let resp = NotesPlugin.execute(&ExecuteRequest {
            action_id: "save_selection_note".into(),
            item_id: String::new(),
            payload: serde_json::json!({ "text": "Super Panel 一键存备忘测试" }),
            plugin_settings: ctx.plugin_settings,
            plugin_secrets: Default::default(),
        });
        assert!(resp.toast.as_deref().unwrap_or("").contains("已一键存为本地备忘"));
        assert_eq!(load_notes_from(&dir).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
