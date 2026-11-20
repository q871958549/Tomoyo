//! Event-driven, strictly memory-bounded CopyQ-grade SQLite Clipboard Center provider (`cb`).

mod db_io;
mod filter;
mod items;
mod store;

pub use db_io::{sanitize_note, sanitize_tag};
pub use filter::{normalize_single_line, ClipboardSubtype, ParsedClipQuery};
pub use store::{ClipboardEntry, ClipboardPayload, ClipboardStore};

use items::{build_clipboard_item, build_filter_hints};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tomoyo_core::{Lang, Provider, TomoyoConfig};
use tomoyo_protocol::{
    ActionKind, ActionSpec, ClipboardSnapshot, IconSpec, InternalCommandKind, QueryContext,
    ResultCategory, ResultItem,
};

pub struct ClipboardProvider {
    store: ClipboardStore,
    config: Arc<RwLock<TomoyoConfig>>,
}

impl ClipboardProvider {
    pub fn with_sqlite(
        db_path: Option<PathBuf>,
        max_persisted: usize,
        config: Arc<RwLock<TomoyoConfig>>,
    ) -> Self {
        Self {
            store: ClipboardStore::open(db_path, max_persisted),
            config,
        }
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }

    fn capture_enabled(&self) -> bool {
        self.config.read().map(|c| c.clipboard.capture_enabled).unwrap_or(true)
    }

    pub fn push_text(&self, raw: String) {
        self.store.push_text(raw);
    }

    pub fn push_text_with_source(&self, raw: String, source_app: &str) {
        self.store.push_text_with_source(raw, source_app);
    }

    pub fn push_image(&self, path: String, width: u32, height: u32, digest_key: String) {
        self.store.push_image(path, width, height, digest_key);
    }

    pub fn push_image_with_source(
        &self,
        path: String,
        width: u32,
        height: u32,
        digest_key: String,
        source_app: &str,
    ) {
        self.store.push_image_with_source(path, width, height, digest_key, source_app);
    }

    pub fn push_files_with_source(&self, paths: Vec<String>, source_app: &str) {
        self.store.push_files_with_source(paths, source_app);
    }

    pub fn toggle_pin(&self, id_key: &str) -> bool {
        self.store.toggle_pin(id_key)
    }

    pub fn delete_item(&self, id_key: &str) -> bool {
        self.store.delete_item(id_key)
    }

    pub fn clear_unpinned(&self) -> usize {
        self.store.clear_unpinned()
    }

    pub fn reload_from_db(&self) {
        self.store.reload_from_db();
    }

    pub fn item_count(&self) -> usize {
        self.store.item_count()
    }

    pub fn shutdown_checkpoint(&self) {
        self.store.shutdown_checkpoint();
    }

    pub fn latest_snapshot(&self) -> Option<ClipboardSnapshot> {
        let guard = self.store.history.read().ok()?;
        for entry in guard.iter() {
            if let ClipboardPayload::Text { text, .. } = &entry.payload {
                return Some(ClipboardSnapshot {
                    text: text.clone(),
                    captured_at_unix: entry.captured_at_unix,
                });
            }
        }
        None
    }
}

impl Provider for ClipboardProvider {
    fn id(&self) -> &'static str {
        "builtin.clipboard"
    }

    fn name(&self) -> &'static str {
        "Clipboard Center (SQLite Text + Image + Files)"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["cb"]
    }

    fn supports_global(&self) -> bool {
        false
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let lang = self.lang();
        let trimmed = ctx.args.trim();

        if trimmed == "@" || trimmed == "#" {
            return build_filter_hints(self.id(), &self.store.list_tags(), lang);
        }
        if trimmed.starts_with(':') {
            return build_clip_commands(self.id(), trimmed, self.capture_enabled(), lang);
        }

        let parsed = ParsedClipQuery::parse(trimmed);
        let entries = self.store.query_filtered(&parsed, ctx.limit.min(30));
        if entries.is_empty() {
            return vec![empty_clipboard_item(self.id(), trimmed, lang)];
        }
        entries
            .into_iter()
            .enumerate()
            .map(|(idx, e)| build_clipboard_item(self.id(), idx, e, lang))
            .collect()
    }

    fn trim_memory(&self) {
        self.store.trim_memory();
    }
}

fn build_clip_commands(provider_id: &str, cmd: &str, capturing: bool, lang: Lang) -> Vec<ResultItem> {
    let low = cmd.to_ascii_lowercase();
    let mut out = Vec::new();
    if ":manage".starts_with(&low) || ":m".starts_with(&low) || low == ":" {
        out.push(ResultItem {
            id: "cb:cmd:manage".into(),
            provider_id: provider_id.into(),
            title: lang.pick("cb :manage — 打开剪贴板工作台（分组标签 / 编辑 / 备份）", "cb :manage — Open Clipboard Workbench").into(),
            subtitle: lang.pick("按需拉起独立窗口，支持编辑文本、标注标签与导出 JSON，关闭即释放 100% 内存", "Ephemeral workbench for editing, tagging & backup (0 idle RAM)").into(),
            category: ResultCategory::Clipboard,
            score: 9_900,
            icon: IconSpec::Monogram("CB".into()),
            badge: Some("Ctrl+E".into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "open_mgr".into(),
                label: lang.pick("打开剪贴板工作台", "Open Workbench").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::InternalCommand {
                    command: InternalCommandKind::OpenClipboardManager { focus_key: None },
                },
            }],
            preview: None,
        });
    }
    if ":pause".starts_with(&low) || ":resume".starts_with(&low) || low == ":" {
        out.push(ResultItem {
            id: "cb:cmd:pause".into(),
            provider_id: provider_id.into(),
            title: if capturing {
                lang.pick("cb :pause — 暂停后台剪贴板采集（隐私临时开关）", "cb :pause — Pause Clipboard Capture").into()
            } else {
                lang.pick("cb :resume — 恢复后台剪贴板采集", "cb :resume — Resume Clipboard Capture").into()
            },
            subtitle: lang.pick("暂停期间新复制内容不会写入历史库", "Temporarily stops recording new clipboard entries").into(),
            category: ResultCategory::Clipboard,
            score: 9_850,
            icon: IconSpec::Monogram(if capturing { "II".into() } else { "▶".into() }),
            badge: Some(if capturing { "ON".into() } else { "PAUSED".into() }),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "toggle_cap".into(),
                label: lang.pick("切换采集状态", "Toggle Capture").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::InternalCommand {
                    command: InternalCommandKind::ToggleClipboardCapture,
                },
            }],
            preview: None,
        });
    }
    if ":clear".starts_with(&low) || low == ":" {
        out.push(ResultItem {
            id: "cb:cmd:clear".into(),
            provider_id: provider_id.into(),
            title: lang.pick("cb :clear — 清空全部非置顶剪贴板记录", "cb :clear — Clear All Unpinned History").into(),
            subtitle: lang.pick("保留已置顶（★）记录，清理其余文本与磁盘缓存图片", "Keeps pinned favorites; deletes all other entries").into(),
            category: ResultCategory::Clipboard,
            score: 9_800,
            icon: IconSpec::Monogram("CLR".into()),
            badge: Some("CLEAN".into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "clear_unpinned".into(),
                label: lang.pick("清空非置顶记录", "Clear Unpinned").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::InternalCommand {
                    command: InternalCommandKind::ClearUnpinnedClipboard,
                },
            }],
            preview: None,
        });
    }
    out
}

fn empty_clipboard_item(provider_id: &str, filter: &str, lang: Lang) -> ResultItem {
    ResultItem {
        id: "cb:empty".into(),
        provider_id: provider_id.into(),
        title: if filter.is_empty() {
            lang.pick("剪贴板历史为空", "Clipboard history is empty").into()
        } else {
            match lang {
                Lang::Zh => format!("未找到匹配「{filter}」的剪贴板记录"),
                Lang::En => format!("No clipboard entries matching `{filter}`"),
            }
        },
        subtitle: lang
            .pick(
                "支持 @pin / @img / @link / @file / @code / #标签 筛选，或回车打开工作台",
                "Try @pin / @img / @link / @file / #tag filters, or Enter to open Workbench",
            )
            .into(),
        category: ResultCategory::Clipboard,
        score: 5_000,
        icon: IconSpec::Monogram("CB".into()),
        badge: Some("0".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "open_mgr".into(),
            label: lang.pick("打开剪贴板工作台", "Open Clipboard Workbench").into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::InternalCommand {
                command: InternalCommandKind::OpenClipboardManager { focus_key: None },
            },
        }],
        preview: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn sqlite_clipboard_persists_text_images_files_tags_notes_and_backup() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_clip_v13_{}.db", std::process::id()));
        let img_tmp = std::env::temp_dir().join(format!("tomoyo_clip_img13_{}.png", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::write(&img_tmp, b"fake_png_bytes");
        let cfg = Arc::new(RwLock::new(TomoyoConfig::default()));

        {
            let cb = ClipboardProvider::with_sqlite(Some(tmp.clone()), 200, cfg.clone());
            cb.push_text_with_source("https://github.com/q871958549/Tomoyo".into(), "chrome.exe");
            cb.push_image_with_source(img_tmp.to_string_lossy().into_owned(), 640, 480, "abc123".into(), "PixPin.exe");
            cb.push_files_with_source(vec![r"C:\Temp\report.pdf".into()], "explorer.exe");
            assert!(cb.toggle_pin("https://github.com/q871958549/Tomoyo"));
            cb.store.update_item("https://github.com/q871958549/Tomoyo", None, "工作", "官方仓库").unwrap();
            assert_eq!(cb.item_count(), 3);

            let link_ctx = QueryContext::new("cb @link #工作", 1, 10).with_keyword("cb", "@link #工作");
            let link_hits = cb.query(&link_ctx);
            assert_eq!(link_hits.len(), 1);
            assert!(link_hits[0].title.contains("[#工作]"));

            let file_ctx = QueryContext::new("cb @file", 2, 10).with_keyword("cb", "@file");
            assert_eq!(cb.query(&file_ctx).len(), 1);

            let exported = cb.store.export_json();
            assert!(exported.contains("官方仓库"));
            assert_eq!(cb.clear_unpinned(), 2);
            assert_eq!(cb.item_count(), 1);
            assert!(cb.store.import_json(&exported).unwrap() >= 1);
            assert!(cb.delete_item("https://github.com/q871958549/Tomoyo"));
            assert_eq!(cb.item_count(), 0);
            cb.trim_memory();
        }

        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&img_tmp);
        let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
        let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    }

    #[test]
    fn queries_1000_clipboard_entries_under_15ms() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_clip_perf_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let store = ClipboardStore::open(Some(tmp.clone()), 1500);
        if let Some(db) = &store.db {
            let conn = db.lock().unwrap();
            let tx = conn.unchecked_transaction().unwrap();
            for i in 0..1000 {
                let txt = if i == 777 {
                    "https://github.com/q871958549/Tomoyo/releases 合同报价".to_string()
                } else {
                    format!("Sample clipboard entry #{i} with payload text")
                };
                let _ = tx.execute(
                    "INSERT INTO clipboard_history (text, captured_at, pinned, kind, subtype, tag, note, source_app)
                     VALUES (?1, ?2, 0, 'text', ?3, ?4, '', 'Code.exe')",
                    rusqlite::params![txt, 1700000000i64 + i as i64, if i == 777 { "link" } else { "text" }, if i == 777 { "工作" } else { "" }],
                );
            }
            tx.commit().unwrap();
        }
        let q = ParsedClipQuery::parse("#工作 报价");
        let started = Instant::now();
        let hits = store.query_filtered(&q, 30);
        let elapsed_ms = started.elapsed().as_millis();
        assert_eq!(hits.len(), 1);
        assert!(elapsed_ms < 100, "1000-row clipboard query too slow: {elapsed_ms}ms");
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
        let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    }
}
