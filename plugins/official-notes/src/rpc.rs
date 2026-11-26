//! JSON-RPC (`tomoyo/call`) bridge powering the standalone Svelte 5 Notes Workbench.

use crate::store::{
    archive_note, collect_tag_counts, create_note_file_ex, default_tag_from_settings,
    delete_note_file, load_notes_with_archive, notes_dir_from_settings, open_or_create_daily_note,
    restore_note, save_note_file, toggle_pin_note, toggle_todo_note, NoteDoc,
};
use serde_json::{json, Value};
use tomoyo_sdk::PluginCallRequest;

pub(crate) fn note_to_json(d: &NoteDoc) -> Value {
    let backlinks: Vec<Value> = d
        .backlinks
        .iter()
        .map(|(file_name, title)| json!({ "file_name": file_name, "title": title }))
        .collect();
    json!({
        "file_name": d.file_name,
        "path": d.path.to_string_lossy(),
        "title": d.title,
        "snippet": d.snippet,
        "body": d.body,
        "tags": d.tags,
        "pinned": d.pinned,
        "archived": d.archived,
        "created_secs": d.created_secs,
        "modified_secs": d.modified_secs,
        "source": d.source,
        "todo_open": d.todo_open,
        "todo_done": d.todo_done,
        "wikilinks": d.wikilinks,
        "backlinks": backlinks,
    })
}

fn parse_string_array(val: Option<&Value>) -> Vec<String> {
    val.and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn handle_rpc(req: &PluginCallRequest) -> Result<Value, String> {
    let dir = notes_dir_from_settings(&req.plugin_settings);
    let def_tag = default_tag_from_settings(&req.plugin_settings);

    match req.method.as_str() {
        "list" => {
            let query = req.params.get("query").and_then(Value::as_str).unwrap_or("").trim();
            let filter = req.params.get("filter").and_then(Value::as_str).unwrap_or("all");
            let active_tag = req.params.get("tag").and_then(Value::as_str).unwrap_or("").trim();
            let focus_file = req.params.get("focus_file").and_then(Value::as_str).unwrap_or("");

            let all_docs = load_notes_with_archive(&dir, true);
            let total = all_docs.iter().filter(|d| !d.archived).count();
            let pinned_count = all_docs.iter().filter(|d| !d.archived && d.pinned).count();
            let todo_notes_count = all_docs.iter().filter(|d| !d.archived && d.todo_open > 0).count();
            let archived_count = all_docs.iter().filter(|d| d.archived).count();
            let tags: Vec<Value> = collect_tag_counts(&all_docs)
                .into_iter()
                .map(|(tag, count)| json!({ "tag": tag, "count": count }))
                .collect();

            let tokens: Vec<String> = query
                .split_whitespace()
                .map(|t| t.to_ascii_lowercase())
                .collect();

            let filtered: Vec<Value> = all_docs
                .iter()
                .filter(|d| match filter {
                    "archived" => d.archived,
                    "pinned" => !d.archived && d.pinned,
                    "todo" => !d.archived && (d.todo_open > 0 || d.todo_done > 0),
                    _ => !d.archived,
                })
                .filter(|d| {
                    active_tag.is_empty()
                        || d.tags.iter().any(|t| t.eq_ignore_ascii_case(active_tag))
                })
                .filter(|d| {
                    if tokens.is_empty() {
                        return true;
                    }
                    let title_low = d.title.to_ascii_lowercase();
                    let body_low = d.body.to_ascii_lowercase();
                    tokens.iter().all(|tok| {
                        title_low.contains(tok)
                            || body_low.contains(tok)
                            || d.tags.iter().any(|t| t.to_ascii_lowercase().contains(tok))
                    })
                })
                .map(note_to_json)
                .collect();

            Ok(json!({
                "notes_dir": dir.to_string_lossy(),
                "default_tag": def_tag,
                "total": total,
                "pinned_count": pinned_count,
                "todo_notes_count": todo_notes_count,
                "archived_count": archived_count,
                "focus_file": focus_file,
                "tags": tags,
                "notes": filtered,
            }))
        }
        "create" => {
            if req.params.get("daily").and_then(Value::as_bool) == Some(true) {
                let doc = open_or_create_daily_note(&dir, &def_tag)?;
                return Ok(json!({ "ok": true, "action": "create", "note": note_to_json(&doc) }));
            }
            let title = req
                .params
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("新备忘录");
            let body = req.params.get("body").and_then(Value::as_str).unwrap_or("");
            let mut tags = parse_string_array(req.params.get("tags"));
            if !def_tag.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(&def_tag)) {
                tags.push(def_tag);
            }
            let pinned = req.params.get("pinned").and_then(Value::as_bool).unwrap_or(false);
            let source = req.params.get("source").and_then(Value::as_str).unwrap_or("workbench");
            let doc = create_note_file_ex(&dir, title, body, &tags, pinned, source)?;
            Ok(json!({ "ok": true, "action": "create", "note": note_to_json(&doc) }))
        }
        "save" => {
            let file_name = req.params.get("file_name").and_then(Value::as_str).unwrap_or("");
            let title = req.params.get("title").and_then(Value::as_str).unwrap_or("未命名备忘");
            let body = req.params.get("body").and_then(Value::as_str).unwrap_or("");
            let tags = parse_string_array(req.params.get("tags"));
            let pinned = req.params.get("pinned").and_then(Value::as_bool).unwrap_or(false);
            let doc = save_note_file(&dir, file_name, title, body, &tags, pinned)?;
            Ok(json!({ "ok": true, "action": "save", "note": note_to_json(&doc) }))
        }
        "toggle_pin" => {
            let file_name = req.params.get("file_name").and_then(Value::as_str).unwrap_or("");
            let pinned = toggle_pin_note(&dir, file_name)?;
            Ok(json!({ "ok": true, "action": "pin", "pinned": pinned, "file_name": file_name }))
        }
        "toggle_todo" => {
            let file_name = req.params.get("file_name").and_then(Value::as_str).unwrap_or("");
            let line_idx = req.params.get("line_idx").and_then(Value::as_u64).unwrap_or(0) as usize;
            let doc = toggle_todo_note(&dir, file_name, line_idx)?;
            Ok(json!({ "ok": true, "action": "todo", "note": note_to_json(&doc) }))
        }
        "archive" => {
            let file_name = req.params.get("file_name").and_then(Value::as_str).unwrap_or("");
            archive_note(&dir, file_name)?;
            Ok(json!({ "ok": true, "action": "archive", "file_name": file_name }))
        }
        "restore" => {
            let file_name = req.params.get("file_name").and_then(Value::as_str).unwrap_or("");
            restore_note(&dir, file_name)?;
            Ok(json!({ "ok": true, "action": "restore", "file_name": file_name }))
        }
        "delete" => {
            let file_name = req.params.get("file_name").and_then(Value::as_str).unwrap_or("");
            let archived = req.params.get("archived").and_then(Value::as_bool).unwrap_or(false);
            delete_note_file(&dir, file_name, archived)?;
            Ok(json!({ "ok": true, "action": "delete", "file_name": file_name }))
        }
        "export_all" => {
            let docs = load_notes_with_archive(&dir, false);
            let items: Vec<Value> = docs.iter().map(note_to_json).collect();
            let json = serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into());
            Ok(json!({ "ok": true, "json": json }))
        }
        other => Err(format!("不支持的备忘录 RPC 方法: `{other}`")),
    }
}
