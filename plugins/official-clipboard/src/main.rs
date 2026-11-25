//! Official Tomoyo Protocol v3 Hybrid Plugin: CopyQ-grade Clipboard History & Workbench (`cb`).
//!
//! Reads and writes the canonical `<tomoyo_data_dir>/clipboard.db` WAL SQLite store so existing
//! users preserve 100% of their history seamlessly after Phase 3d externalization.

mod config;
mod db_io;
mod filter;
mod items;
mod paste_stack;
mod store;

use base64::Engine;
use config::{toggle_runtime_paused, tomoyo_data_dir, ClipRuntimeConfig};
use filter::ParsedClipQuery;
use items::{build_clip_commands, build_clipboard_item, build_filter_hints, empty_clipboard_item};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use store::{ClipboardEntry, ClipboardPayload, ClipboardStore};
use tomoyo_sdk::{
    ExecuteRequest, ExecuteResponse, OpenWebViewRequest, PluginCallRequest, PluginEventKind,
    PluginEventPayload, PluginMeta, QueryContext, ResultItem, TomoyoPlugin,
};

pub const PLUGIN_ID: &str = "dev.tomoyo.clipboard";
const MAX_INLINE_IMAGE_BYTES: usize = 3 * 1024 * 1024;

struct CachedStore {
    persist: bool,
    max_items: usize,
    store: ClipboardStore,
}

struct ClipboardPlugin {
    cache: Mutex<Option<CachedStore>>,
    db_override: Option<PathBuf>,
}

impl ClipboardPlugin {
    fn new() -> Self {
        Self { cache: Mutex::new(None), db_override: None }
    }

    fn with_db_path(path: PathBuf) -> Self {
        Self { cache: Mutex::new(None), db_override: Some(path) }
    }

    fn with_store<R>(&self, cfg: &ClipRuntimeConfig, f: impl FnOnce(&ClipboardStore) -> R) -> R {
        let mut guard = self.cache.lock().unwrap();
        let needs_open = match guard.as_ref() {
            Some(c) => c.persist != cfg.persist_sqlite || c.max_items != cfg.max_persisted_items,
            None => true,
        };
        if needs_open {
            let db_path = if cfg.persist_sqlite {
                Some(self.db_override.clone().unwrap_or_else(|| tomoyo_data_dir().join("clipboard.db")))
            } else {
                None
            };
            *guard = Some(CachedStore {
                persist: cfg.persist_sqlite,
                max_items: cfg.max_persisted_items,
                store: ClipboardStore::open(db_path, cfg.max_persisted_items),
            });
        }
        f(&guard.as_ref().unwrap().store)
    }
}

impl TomoyoPlugin for ClipboardPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("剪贴板管理中心 (Clipboard Center)", env!("CARGO_PKG_VERSION"))
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let cfg = ClipRuntimeConfig::from_settings(&ctx.plugin_settings);
        let trimmed = ctx.args.trim();
        if trimmed == "@" || trimmed == "#" {
            let tags = self.with_store(&cfg, |s| s.list_tags());
            return build_filter_hints(&tags);
        }
        if trimmed.eq_ignore_ascii_case(":stack") || trimmed.eq_ignore_ascii_case("stack") {
            return paste_stack::build_stack_items(&paste_stack::PasteStack::default_path());
        }
        if trimmed.starts_with(':') {
            return build_clip_commands(trimmed, cfg.is_effectively_capturing());
        }
        let parsed = ParsedClipQuery::parse(trimmed);
        let entries = self.with_store(&cfg, |s| s.query_filtered(&parsed, ctx.limit.min(30)));
        if entries.is_empty() {
            return vec![empty_clipboard_item(trimmed)];
        }
        entries
            .into_iter()
            .enumerate()
            .map(|(idx, e)| build_clipboard_item(idx, e))
            .collect()
    }

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        let cfg = ClipRuntimeConfig::from_settings(&req.plugin_settings);
        if req.action_id.starts_with("stack_") {
            let path = paste_stack::PasteStack::default_path();
            if let Some(resp) = self.with_store(&cfg, |s| {
                paste_stack::handle_stack_action(&req.action_id, &req.payload, &path, Some(s))
            }) {
                return resp;
            }
        }
        match req.action_id.as_str() {
            "open_workbench" => {
                let focus_key = req
                    .payload
                    .get("focus_key")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                ExecuteResponse {
                    hide_window: true,
                    open_webview: Some(OpenWebViewRequest {
                        entry: None,
                        title: Some("Tomoyo 剪贴板工作台".into()),
                        initial_query: focus_key,
                        width: Some(1060),
                        height: Some(680),
                    }),
                    ..Default::default()
                }
            }
            "toggle_pin" => {
                let key = req.payload.get("id_key").and_then(Value::as_str).unwrap_or("");
                let pinned = self.with_store(&cfg, |s| s.toggle_pin(key));
                ExecuteResponse {
                    hide_window: false,
                    refresh_query: true,
                    toast: Some(if pinned { "★ 已置顶该剪贴板记录".into() } else { "已取消置顶".into() }),
                    ..Default::default()
                }
            }
            "delete" => {
                let key = req.payload.get("id_key").and_then(Value::as_str).unwrap_or("");
                let ok = self.with_store(&cfg, |s| s.delete_item(key));
                ExecuteResponse {
                    hide_window: false,
                    refresh_query: true,
                    toast: ok.then(|| "✓ 已删除该条剪贴板记录".into()),
                    ..Default::default()
                }
            }
            "clear_unpinned" => {
                let n = self.with_store(&cfg, |s| s.clear_unpinned());
                ExecuteResponse {
                    hide_window: false,
                    refresh_query: true,
                    toast: Some(format!("✓ 已清理 {n} 条非置顶剪贴板记录（置顶已保留）")),
                    ..Default::default()
                }
            }
            "pin_selection_text" => {
                let raw = req.payload.get("text").and_then(Value::as_str).unwrap_or("");
                let ok = self.with_store(&cfg, |s| s.push_and_pin_text(raw.to_string(), "super_panel"));
                ExecuteResponse {
                    hide_window: true,
                    toast: Some(if ok { "★ 已置顶存入剪贴板历史".into() } else { "未检测到可保存的文本".into() }),
                    ..Default::default()
                }
            }
            "toggle_capture" => {
                let paused = toggle_runtime_paused();
                ExecuteResponse {
                    hide_window: false,
                    refresh_query: true,
                    toast: Some(if paused {
                        "已暂停后台剪贴板采集（隐私临时开关）".into()
                    } else {
                        "✓ 已恢复后台剪贴板采集".into()
                    }),
                    ..Default::default()
                }
            }
            _ => ExecuteResponse::default(),
        }
    }

    fn on_event(&self, ev: &PluginEventPayload) -> ExecuteResponse {
        if ev.event == PluginEventKind::HotkeyTriggered {
            if ev.data.get("hotkey_id").and_then(Value::as_str) == Some("paste_stack_next") {
                let path = paste_stack::PasteStack::default_path();
                return paste_stack::handle_stack_action("stack_pop_copy", &json!({}), &path, None)
                    .unwrap_or_default();
            }
            return ExecuteResponse {
                hide_window: false,
                fill_query: Some("cb ".into()),
                ..Default::default()
            };
        }
        let cfg = ClipRuntimeConfig::from_settings(&ev.plugin_settings);
        if !cfg.is_effectively_capturing() {
            return ExecuteResponse::default();
        }
        let src = ev.data.get("source_app").and_then(Value::as_str).unwrap_or("");
        if !src.is_empty() && cfg.excluded_apps.iter().any(|e| e.eq_ignore_ascii_case(src)) {
            return ExecuteResponse::default();
        }
        match ev.event {
            PluginEventKind::ClipboardText => {
                let kind = ev.data.get("kind").and_then(Value::as_str).unwrap_or("text");
                if kind == "files" {
                    let paths: Vec<String> = ev
                        .data
                        .get("paths")
                        .and_then(Value::as_array)
                        .map(|arr| arr.iter().filter_map(Value::as_str).map(String::from).collect())
                        .unwrap_or_default();
                    if !paths.is_empty() {
                        self.with_store(&cfg, |s| s.push_files_with_source(paths, src));
                    }
                } else if let Some(txt) = ev.data.get("text").and_then(Value::as_str)
                    && !txt.trim().is_empty()
                {
                    self.with_store(&cfg, |s| s.push_text_with_source(txt.to_string(), src));
                }
            }
            PluginEventKind::ClipboardImage => {
                if cfg.capture_images
                    && let Some(path) = ev.data.get("path").and_then(Value::as_str)
                    && let Some(digest) = ev.data.get("digest").and_then(Value::as_str)
                {
                    let w = ev.data.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
                    let h = ev.data.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
                    self.with_store(&cfg, |s| {
                        s.push_image_with_source(path.to_string(), w, h, digest.to_string(), src)
                    });
                }
            }
            PluginEventKind::HotkeyTriggered => {}
        }
        ExecuteResponse::default()
    }

    fn call(&self, req: &PluginCallRequest) -> Result<Value, String> {
        let cfg = ClipRuntimeConfig::from_settings(&req.plugin_settings);
        match req.method.as_str() {
            "list" => {
                let query = req.params.get("query").and_then(Value::as_str).unwrap_or("");
                let focus_key = req.params.get("focus_key").and_then(Value::as_str).unwrap_or("");
                let parsed = ParsedClipQuery::parse(query);
                let data = self.with_store(&cfg, |store| {
                    let entries = store.query_filtered(&parsed, 200);
                    let items: Vec<Value> = entries.into_iter().map(entry_to_json).collect();
                    let tags: Vec<Value> = store
                        .list_tags()
                        .into_iter()
                        .map(|(tag, count)| json!({ "tag": tag, "count": count }))
                        .collect();
                    json!({
                        "focus_key": focus_key,
                        "total": store.item_count(),
                        "capture_enabled": cfg.is_effectively_capturing(),
                        "tags": tags,
                        "items": items,
                    })
                });
                Ok(data)
            }
            "toggle_pin" => {
                let id_key = req.params.get("id_key").and_then(Value::as_str).unwrap_or("");
                let pinned = self.with_store(&cfg, |s| s.toggle_pin(id_key));
                Ok(json!({ "ok": true, "action": "pin", "pinned": pinned, "key": id_key }))
            }
            "update" => {
                let id_key = req.params.get("id_key").and_then(Value::as_str).unwrap_or("");
                let text = req.params.get("text").and_then(Value::as_str).map(String::from);
                let tag = req.params.get("tag").and_then(Value::as_str).unwrap_or("");
                let note = req.params.get("note").and_then(Value::as_str).unwrap_or("");
                match self.with_store(&cfg, |s| s.update_item(id_key, text, tag, note)) {
                    Ok(new_key) => Ok(json!({ "ok": true, "action": "update", "key": new_key })),
                    Err(e) => Ok(json!({ "ok": false, "action": "update", "message": e })),
                }
            }
            "delete" => {
                let id_key = req.params.get("id_key").and_then(Value::as_str).unwrap_or("");
                let ok = self.with_store(&cfg, |s| s.delete_item(id_key));
                Ok(json!({ "ok": ok, "action": "delete" }))
            }
            "clear_unpinned" => {
                let count = self.with_store(&cfg, |s| s.clear_unpinned());
                Ok(json!({ "ok": true, "action": "clear", "count": count }))
            }
            "export" => {
                let exported = self.with_store(&cfg, |s| s.export_json());
                Ok(json!({ "ok": true, "json": exported }))
            }
            "import" => {
                let raw = req.params.get("json").and_then(Value::as_str).unwrap_or("");
                match self.with_store(&cfg, |s| s.import_json(raw)) {
                    Ok(count) => Ok(json!({ "ok": true, "action": "import", "count": count })),
                    Err(e) => Ok(json!({ "ok": false, "action": "import", "message": e })),
                }
            }
            other => Err(format!("unsupported clipboard RPC method `{other}`")),
        }
    }
}

fn entry_to_json(e: ClipboardEntry) -> Value {
    let subtype = e.subtype().as_str().to_string();
    let (kind, text, image_url, image_path, width, height, files) = match e.payload {
        ClipboardPayload::Text { text, .. } => ("text", text, None, None, 0, 0, Vec::new()),
        ClipboardPayload::Image { path, width, height } => {
            let url = encode_image_data_uri(Path::new(&path));
            ("image", String::new(), url, Some(path), width, height, Vec::new())
        }
        ClipboardPayload::Files { paths } => ("files", paths.join("\n"), None, None, 0, 0, paths),
    };
    json!({
        "key": e.key,
        "kind": kind,
        "subtype": subtype,
        "preview": e.preview_line,
        "text": text,
        "image_url": image_url,
        "image_path": image_path,
        "width": width,
        "height": height,
        "files": files,
        "ts": e.captured_at_unix,
        "pinned": e.pinned,
        "tag": e.tag,
        "note": e.note,
        "source_app": e.source_app,
    })
}

fn encode_image_data_uri(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() == 0 || meta.len() as usize > MAX_INLINE_IMAGE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

fn run_self_test() {
    let tmp = std::env::temp_dir().join(format!("tomoyo_clip_plug_st_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let plug = ClipboardPlugin::with_db_path(tmp.clone());

    let ev = PluginEventPayload::new(
        PluginEventKind::ClipboardText,
        json!({ "kind": "text", "text": "https://github.com/q871958549/Tomoyo", "source_app": "firefox" }),
    );
    let _ = plug.on_event(&ev);

    let hits = plug.query(&QueryContext::new("cb @link", 1, 10).with_keyword("cb", "@link"));
    assert_eq!(hits.len(), 1);

    let listed = plug
        .call(&PluginCallRequest {
            method: "list".into(),
            params: json!({ "query": "@link" }),
            plugin_settings: BTreeMap::new(),
            plugin_secrets: BTreeMap::new(),
        })
        .expect("list ok");
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);

    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
    let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    println!("[OK] tomoyo-plugin-clipboard v{} self-test passed", env!("CARGO_PKG_VERSION"));
}

fn main() {
    if std::env::args().any(|a| a == "--self-test") {
        run_self_test();
        return;
    }
    tomoyo_sdk::run(ClipboardPlugin::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_event_query_rpc_and_privacy_filter_lifecycle() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_clip_plug_ut_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let plug = ClipboardPlugin::with_db_path(tmp.clone());

        let mut settings = BTreeMap::new();
        settings.insert("excluded_apps".into(), json!("keepass.exe, bitwarden"));
        let mut secret_ev = PluginEventPayload::new(
            PluginEventKind::ClipboardText,
            json!({ "text": "super-secret-password", "source_app": "keepass.exe" }),
        );
        secret_ev.plugin_settings = settings.clone();
        let _ = plug.on_event(&secret_ev);

        let mut ok_ev = PluginEventPayload::new(
            PluginEventKind::ClipboardText,
            json!({ "text": "https://github.com/q871958549/Tomoyo", "source_app": "code" }),
        );
        ok_ev.plugin_settings = settings;
        let _ = plug.on_event(&ok_ev);

        let list = plug
            .call(&PluginCallRequest {
                method: "list".into(),
                params: json!({ "query": "" }),
                plugin_settings: BTreeMap::new(),
                plugin_secrets: BTreeMap::new(),
            })
            .unwrap();
        assert_eq!(list["total"], 1);
        assert_eq!(list["items"][0]["subtype"], "link");

        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
        let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    }
}
