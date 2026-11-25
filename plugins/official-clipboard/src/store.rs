//! Strictly bounded SQLite WAL + RAM ring-buffer storage for Clipboard Center.

use crate::db_io::{
    entry_matches_query, import_records_locked, load_recent_rows, make_files_preview,
    make_preview_line, open_clipboard_db, prune_sqlite_locked, query_sqlite_filtered,
    sanitize_note, sanitize_source, sanitize_tag, truncate_utf8, BackupRecord,
};
use crate::filter::{classify_text_subtype, ClipboardSubtype, ParsedClipQuery};
use chrono::Local;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

pub const MAX_RAM_ITEMS: usize = 64;
pub const MAX_ITEM_BYTES: usize = 16 * 1024;
pub const MAX_DISK_IMAGES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipboardPayload {
    Text {
        text: String,
        char_count: usize,
        subtype: ClipboardSubtype,
    },
    Image {
        path: String,
        width: u32,
        height: u32,
    },
    Files {
        paths: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub key: String,
    pub payload: ClipboardPayload,
    pub preview_line: String,
    pub captured_at_unix: i64,
    pub pinned: bool,
    pub tag: String,
    pub note: String,
    pub source_app: String,
}

impl ClipboardEntry {
    pub fn subtype(&self) -> ClipboardSubtype {
        match &self.payload {
            ClipboardPayload::Text { subtype, .. } => *subtype,
            ClipboardPayload::Image { .. } => ClipboardSubtype::Image,
            ClipboardPayload::Files { .. } => ClipboardSubtype::Files,
        }
    }
}

pub struct ClipboardStore {
    pub history: RwLock<VecDeque<ClipboardEntry>>,
    pub db: Option<Mutex<Connection>>,
    pub max_persisted: usize,
}

impl ClipboardStore {
    pub fn open(db_path: Option<PathBuf>, max_persisted: usize) -> Self {
        let mut initial = VecDeque::with_capacity(32);
        let db = db_path.and_then(|path| open_clipboard_db(&path, &mut initial).map(Mutex::new));
        Self {
            history: RwLock::new(initial),
            db,
            max_persisted: max_persisted.clamp(50, 2000),
        }
    }

    pub fn push_text_with_source(&self, raw: String, source_app: &str) {
        if raw.trim().is_empty() {
            return;
        }
        let truncated = truncate_utf8(&raw, MAX_ITEM_BYTES);
        let char_count = truncated.chars().count();
        let subtype = classify_text_subtype(&truncated);
        let preview_line = make_preview_line(&truncated);
        let now = Local::now().timestamp();
        let src = sanitize_source(source_app);

        self.insert_ram_entry(ClipboardEntry {
            key: truncated.clone(),
            payload: ClipboardPayload::Text {
                text: truncated.clone(),
                char_count,
                subtype,
            },
            preview_line,
            captured_at_unix: now,
            pinned: false,
            tag: String::new(),
            note: String::new(),
            source_app: src.clone(),
        });

        if let Some(db_lock) = &self.db
            && let Ok(conn) = db_lock.lock()
        {
            let _ = conn.execute(
                "INSERT INTO clipboard_history (text, captured_at, pinned, kind, subtype, source_app)
                 VALUES (?1, ?2, 0, 'text', ?3, ?4)
                 ON CONFLICT(text) DO UPDATE SET
                   captured_at = excluded.captured_at,
                   subtype = excluded.subtype,
                   source_app = CASE WHEN excluded.source_app != '' THEN excluded.source_app ELSE clipboard_history.source_app END",
                params![truncated, now, subtype.as_str(), src],
            );
            prune_sqlite_locked(&conn, self.max_persisted);
        }
    }

    pub fn push_and_pin_text(&self, raw: String, source_app: &str) -> bool {
        if raw.trim().is_empty() {
            return false;
        }
        let key = truncate_utf8(&raw, MAX_ITEM_BYTES);
        self.push_text_with_source(key.clone(), source_app);
        if let Ok(mut guard) = self.history.write()
            && let Some(item) = guard.iter_mut().find(|e| e.key == key)
        {
            item.pinned = true;
        }
        if let Some(db_lock) = &self.db
            && let Ok(conn) = db_lock.lock()
        {
            let _ = conn.execute("UPDATE clipboard_history SET pinned = 1 WHERE text = ?1", params![key]);
        }
        true
    }

    pub fn push_image_with_source(
        &self,
        path: String,
        width: u32,
        height: u32,
        digest_key: String,
        source_app: &str,
    ) {
        let key = format!("img:{digest_key}");
        let preview_line = format!("[图片] {width}×{height} PNG");
        let now = Local::now().timestamp();
        let src = sanitize_source(source_app);

        self.insert_ram_entry(ClipboardEntry {
            key: key.clone(),
            payload: ClipboardPayload::Image { path: path.clone(), width, height },
            preview_line,
            captured_at_unix: now,
            pinned: false,
            tag: String::new(),
            note: String::new(),
            source_app: src.clone(),
        });

        if let Some(db_lock) = &self.db
            && let Ok(conn) = db_lock.lock()
        {
            let _ = conn.execute(
                "INSERT INTO clipboard_history (text, captured_at, pinned, kind, subtype, image_path, img_w, img_h, source_app)
                 VALUES (?1, ?2, 0, 'image', 'image', ?3, ?4, ?5, ?6)
                 ON CONFLICT(text) DO UPDATE SET
                   captured_at = excluded.captured_at,
                   image_path = excluded.image_path,
                   source_app = CASE WHEN excluded.source_app != '' THEN excluded.source_app ELSE clipboard_history.source_app END",
                params![key, now, path, width as i64, height as i64, src],
            );
            prune_sqlite_locked(&conn, self.max_persisted);
        }
    }

    pub fn push_files_with_source(&self, mut paths: Vec<String>, source_app: &str) {
        paths.retain(|p| !p.trim().is_empty());
        paths.truncate(32);
        if paths.is_empty() {
            return;
        }
        let key = format!("files:{}", paths.join("\n"));
        let preview_line = make_files_preview(&paths);
        let paths_json = serde_json::to_string(&paths).unwrap_or_else(|_| "[]".into());
        let now = Local::now().timestamp();
        let src = sanitize_source(source_app);

        self.insert_ram_entry(ClipboardEntry {
            key: key.clone(),
            payload: ClipboardPayload::Files { paths },
            preview_line,
            captured_at_unix: now,
            pinned: false,
            tag: String::new(),
            note: String::new(),
            source_app: src.clone(),
        });

        if let Some(db_lock) = &self.db
            && let Ok(conn) = db_lock.lock()
        {
            let _ = conn.execute(
                "INSERT INTO clipboard_history (text, captured_at, pinned, kind, subtype, file_paths, source_app)
                 VALUES (?1, ?2, 0, 'files', 'files', ?3, ?4)
                 ON CONFLICT(text) DO UPDATE SET captured_at = excluded.captured_at, file_paths = excluded.file_paths",
                params![key, now, paths_json, src],
            );
            prune_sqlite_locked(&conn, self.max_persisted);
        }
    }

    pub fn toggle_pin(&self, id_key: &str) -> bool {
        let mut new_pinned = false;
        if let Ok(mut guard) = self.history.write()
            && let Some(item) = guard.iter_mut().find(|e| e.key == id_key)
        {
            item.pinned = !item.pinned;
            new_pinned = item.pinned;
        }
        if let Some(db_lock) = &self.db
            && let Ok(conn) = db_lock.lock()
            && let Ok(cur) = conn.query_row(
                "SELECT pinned FROM clipboard_history WHERE text = ?1",
                params![id_key],
                |r| r.get::<_, i64>(0),
            )
        {
            let target = if cur == 0 { 1i64 } else { 0i64 };
            new_pinned = target != 0;
            let _ = conn.execute(
                "UPDATE clipboard_history SET pinned = ?1 WHERE text = ?2",
                params![target, id_key],
            );
        }
        new_pinned
    }

    pub fn delete_item(&self, id_key: &str) -> bool {
        let mut deleted = false;
        if let Ok(mut guard) = self.history.write()
            && let Some(pos) = guard.iter().position(|e| e.key == id_key)
            && let Some(removed) = guard.remove(pos)
        {
            deleted = true;
            if let ClipboardPayload::Image { path, .. } = removed.payload {
                let _ = std::fs::remove_file(path);
            }
        }
        if let Some(db_lock) = &self.db
            && let Ok(conn) = db_lock.lock()
        {
            if let Ok(Some(img_path)) = conn.query_row(
                "SELECT image_path FROM clipboard_history WHERE text = ?1 AND kind = 'image'",
                params![id_key],
                |r| r.get::<_, Option<String>>(0),
            ) {
                let _ = std::fs::remove_file(img_path);
            }
            if let Ok(n) = conn.execute("DELETE FROM clipboard_history WHERE text = ?1", params![id_key]) {
                deleted = deleted || n > 0;
            }
        }
        deleted
    }

    pub fn clear_unpinned(&self) -> usize {
        if let Ok(mut guard) = self.history.write() {
            guard.retain(|e| e.pinned);
        }
        let Some(db_lock) = &self.db else { return 0 };
        let Ok(conn) = db_lock.lock() else { return 0 };
        if let Ok(mut stmt) = conn.prepare(
            "SELECT image_path FROM clipboard_history WHERE pinned = 0 AND kind = 'image' AND image_path IS NOT NULL",
        ) && let Ok(rows) = stmt.query_map([], |r| r.get::<_, Option<String>>(0))
        {
            for path in rows.flatten().flatten() {
                let _ = std::fs::remove_file(path);
            }
        }
        conn.execute("DELETE FROM clipboard_history WHERE pinned = 0", []).unwrap_or(0)
    }

    pub fn update_item(
        &self,
        id_key: &str,
        new_text: Option<String>,
        tag: &str,
        note: &str,
    ) -> Result<String, String> {
        let clean_tag = sanitize_tag(tag);
        let clean_note = sanitize_note(note);
        let next_key = match new_text {
            Some(t) if !id_key.starts_with("img:") && !id_key.starts_with("files:") => {
                if t.trim().is_empty() {
                    return Err("文本内容不能为空".into());
                }
                truncate_utf8(&t, MAX_ITEM_BYTES)
            }
            _ => id_key.to_string(),
        };

        if let Some(db_lock) = &self.db {
            let conn = db_lock.lock().map_err(|_| "数据库锁定失败".to_string())?;
            if next_key != id_key {
                let sub = classify_text_subtype(&next_key);
                let _ = conn.execute(
                    "DELETE FROM clipboard_history WHERE text = ?1 AND text != ?2",
                    params![next_key, id_key],
                );
                conn.execute(
                    "UPDATE clipboard_history SET text = ?1, subtype = ?2, tag = ?3, note = ?4 WHERE text = ?5",
                    params![next_key, sub.as_str(), clean_tag, clean_note, id_key],
                )
                .map_err(|e| format!("更新剪贴板记录失败: {e}"))?;
            } else {
                conn.execute(
                    "UPDATE clipboard_history SET tag = ?1, note = ?2 WHERE text = ?3",
                    params![clean_tag, clean_note, id_key],
                )
                .map_err(|e| format!("更新标签/备注失败: {e}"))?;
            }
        }
        self.reload_from_db();
        Ok(next_key)
    }

    pub fn list_tags(&self) -> Vec<(String, usize)> {
        if let Some(db_lock) = &self.db
            && let Ok(conn) = db_lock.lock()
            && let Ok(mut stmt) = conn.prepare(
                "SELECT tag, COUNT(*) FROM clipboard_history WHERE tag != '' GROUP BY tag ORDER BY COUNT(*) DESC, tag ASC LIMIT 32",
            )
            && let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))
        {
            return rows.flatten().collect();
        }
        let mut map: HashMap<String, usize> = HashMap::new();
        if let Ok(g) = self.history.read() {
            for e in g.iter().filter(|e| !e.tag.is_empty()) {
                *map.entry(e.tag.clone()).or_insert(0) += 1;
            }
        }
        let mut v: Vec<_> = map.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }

    pub fn reload_from_db(&self) {
        let Some(db_lock) = &self.db else { return };
        let Ok(conn) = db_lock.lock() else { return };
        let mut fresh = VecDeque::with_capacity(MAX_RAM_ITEMS);
        load_recent_rows(&conn, MAX_RAM_ITEMS, &mut fresh);
        if let Ok(mut guard) = self.history.write() {
            *guard = fresh;
        }
    }

    pub fn export_json(&self) -> String {
        let entries = self.query_filtered(&ParsedClipQuery::default(), 500);
        let recs: Vec<BackupRecord> = entries
            .into_iter()
            .filter_map(|e| match e.payload {
                ClipboardPayload::Text { text, .. } => Some(BackupRecord {
                    kind: "text".into(),
                    text,
                    captured_at: e.captured_at_unix,
                    pinned: e.pinned,
                    tag: e.tag,
                    note: e.note,
                    source_app: e.source_app,
                }),
                _ => None,
            })
            .collect();
        serde_json::to_string_pretty(&recs).unwrap_or_else(|_| "[]".into())
    }

    pub fn import_json(&self, raw: &str) -> Result<usize, String> {
        let imported = if let Some(db_lock) = &self.db {
            let conn = db_lock.lock().map_err(|_| "数据库锁定失败".to_string())?;
            let n = import_records_locked(&conn, raw)?;
            prune_sqlite_locked(&conn, self.max_persisted);
            n
        } else {
            0
        };
        self.reload_from_db();
        Ok(imported)
    }

    pub fn item_count(&self) -> usize {
        if let Some(db_lock) = &self.db
            && let Ok(conn) = db_lock.lock()
            && let Ok(n) = conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |r| r.get::<_, i64>(0))
        {
            return n.max(0) as usize;
        }
        self.history.read().map(|g| g.len()).unwrap_or(0)
    }

    pub fn query_filtered(&self, q: &ParsedClipQuery, cap: usize) -> Vec<ClipboardEntry> {
        let cap = cap.clamp(1, 500);
        if let Some(db_lock) = &self.db
            && let Ok(conn) = db_lock.lock()
            && let Some(db_items) = query_sqlite_filtered(&conn, q, cap)
        {
            return db_items;
        }
        let Ok(guard) = self.history.read() else { return Vec::new() };
        let mut v: Vec<ClipboardEntry> = guard.iter().filter(|e| entry_matches_query(e, q)).cloned().collect();
        v.sort_by(|a, b| b.pinned.cmp(&a.pinned).then_with(|| b.captured_at_unix.cmp(&a.captured_at_unix)));
        v.truncate(cap);
        v
    }

    fn insert_ram_entry(&self, mut entry: ClipboardEntry) {
        if let Ok(mut guard) = self.history.write() {
            if let Some(pos) = guard.iter().position(|e| e.key == entry.key) {
                entry.pinned = guard[pos].pinned;
                if entry.tag.is_empty() { entry.tag = guard[pos].tag.clone(); }
                if entry.note.is_empty() { entry.note = guard[pos].note.clone(); }
                guard.remove(pos);
            }
            guard.push_front(entry);
            while guard.len() > MAX_RAM_ITEMS {
                if let Some(pos) = guard.iter().rposition(|e| !e.pinned) {
                    guard.remove(pos);
                } else {
                    guard.pop_back();
                }
            }
        }
    }
}
