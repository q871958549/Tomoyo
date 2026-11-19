//! Strictly bounded SQLite WAL + RAM ring-buffer storage for Text & Disk-Backed PNG Clipboard History.

use chrono::Local;
use rusqlite::{params, Connection};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

pub const MAX_RAM_ITEMS: usize = 64;
pub const MAX_ITEM_BYTES: usize = 16 * 1024;
pub const MAX_DISK_IMAGES: usize = 50;

#[derive(Debug, Clone)]
pub enum ClipboardPayload {
    Text { text: String, char_count: usize },
    Image { path: String, width: u32, height: u32 },
}

#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    pub key: String,
    pub payload: ClipboardPayload,
    pub preview_line: String,
    pub captured_at_unix: i64,
    pub pinned: bool,
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

    pub fn push_text(&self, raw: String) {
        if raw.trim().is_empty() {
            return;
        }
        let truncated = truncate_utf8(&raw, MAX_ITEM_BYTES);
        let char_count = truncated.chars().count();
        let preview_line = make_preview_line(&truncated);
        let now = Local::now().timestamp();

        self.insert_ram_entry(ClipboardEntry {
            key: truncated.clone(),
            payload: ClipboardPayload::Text {
                text: truncated.clone(),
                char_count,
            },
            preview_line,
            captured_at_unix: now,
            pinned: false,
        });

        if let Some(db_lock) = &self.db {
            if let Ok(conn) = db_lock.lock() {
                let _ = conn.execute(
                    "INSERT INTO clipboard_history (text, captured_at, pinned, kind) VALUES (?1, ?2, 0, 'text')
                     ON CONFLICT(text) DO UPDATE SET captured_at = excluded.captured_at",
                    params![truncated, now],
                );
                self.prune_sqlite_locked(&conn);
            }
        }
    }

    pub fn push_image(&self, path: String, width: u32, height: u32, digest_key: String) {
        let key = format!("img:{digest_key}");
        let preview_line = format!("[图片] {width}×{height} PNG");
        let now = Local::now().timestamp();

        self.insert_ram_entry(ClipboardEntry {
            key: key.clone(),
            payload: ClipboardPayload::Image {
                path: path.clone(),
                width,
                height,
            },
            preview_line,
            captured_at_unix: now,
            pinned: false,
        });

        if let Some(db_lock) = &self.db {
            if let Ok(conn) = db_lock.lock() {
                let _ = conn.execute(
                    "INSERT INTO clipboard_history (text, captured_at, pinned, kind, image_path, img_w, img_h)
                     VALUES (?1, ?2, 0, 'image', ?3, ?4, ?5)
                     ON CONFLICT(text) DO UPDATE SET captured_at = excluded.captured_at, image_path = excluded.image_path",
                    params![key, now, path, width as i64, height as i64],
                );
                self.prune_sqlite_locked(&conn);
            }
        }
    }

    pub fn toggle_pin(&self, id_key: &str) -> bool {
        let mut new_pinned = false;
        if let Ok(mut guard) = self.history.write() {
            if let Some(item) = guard.iter_mut().find(|e| e.key == id_key) {
                item.pinned = !item.pinned;
                new_pinned = item.pinned;
            }
        }
        if let Some(db_lock) = &self.db {
            if let Ok(conn) = db_lock.lock() {
                if let Ok(cur) = conn.query_row(
                    "SELECT pinned FROM clipboard_history WHERE text = ?1",
                    params![id_key],
                    |r| r.get::<_, i64>(0),
                ) {
                    let target = if cur == 0 { 1i64 } else { 0i64 };
                    new_pinned = target != 0;
                    let _ = conn.execute(
                        "UPDATE clipboard_history SET pinned = ?1 WHERE text = ?2",
                        params![target, id_key],
                    );
                }
            }
        }
        new_pinned
    }

    pub fn item_count(&self) -> usize {
        if let Some(db_lock) = &self.db {
            if let Ok(conn) = db_lock.lock() {
                if let Ok(n) = conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |r| {
                    r.get::<_, i64>(0)
                }) {
                    return n.max(0) as usize;
                }
            }
        }
        self.history.read().map(|g| g.len()).unwrap_or(0)
    }

    pub fn query_entries(&self, filter: &str, cap: usize) -> Vec<ClipboardEntry> {
        if let Some(db_items) = self.query_sqlite(filter, cap) {
            return db_items;
        }
        let Ok(guard) = self.history.read() else {
            return Vec::new();
        };
        let mut v: Vec<ClipboardEntry> = guard
            .iter()
            .filter(|e| {
                filter.is_empty()
                    || e.preview_line.to_lowercase().contains(filter)
                    || e.key.to_lowercase().contains(filter)
            })
            .cloned()
            .collect();
        v.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.captured_at_unix.cmp(&a.captured_at_unix))
        });
        v.truncate(cap);
        v
    }

    pub fn trim_memory(&self) {
        if let Ok(mut guard) = self.history.write() {
            guard.shrink_to_fit();
        }
        if let Some(db_lock) = &self.db {
            if let Ok(conn) = db_lock.lock() {
                let _ = conn.execute_batch("PRAGMA shrink_memory;");
            }
        }
    }

    fn insert_ram_entry(&self, mut entry: ClipboardEntry) {
        if let Ok(mut guard) = self.history.write() {
            if let Some(pos) = guard.iter().position(|e| e.key == entry.key) {
                entry.pinned = guard[pos].pinned;
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

    fn prune_sqlite_locked(&self, conn: &Connection) {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, image_path FROM clipboard_history
             WHERE kind = 'image' AND pinned = 0 AND id NOT IN (
                 SELECT id FROM clipboard_history WHERE kind = 'image'
                 ORDER BY pinned DESC, captured_at DESC LIMIT ?1
             )",
        ) {
            if let Ok(rows) = stmt.query_map(params![MAX_DISK_IMAGES as i64], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
            }) {
                for (id, path_opt) in rows.flatten() {
                    if let Some(p) = path_opt {
                        let _ = std::fs::remove_file(p);
                    }
                    let _ = conn.execute("DELETE FROM clipboard_history WHERE id = ?1", params![id]);
                }
            }
        }

        let _ = conn.execute(
            "DELETE FROM clipboard_history WHERE pinned = 0 AND kind = 'text' AND id NOT IN (
                SELECT id FROM clipboard_history ORDER BY pinned DESC, captured_at DESC LIMIT ?1
            )",
            params![self.max_persisted as i64],
        );
    }

    fn query_sqlite(&self, filter: &str, limit: usize) -> Option<Vec<ClipboardEntry>> {
        let conn = self.db.as_ref()?.lock().ok()?;
        let sql = if filter.is_empty() {
            "SELECT text, captured_at, pinned, kind, image_path, img_w, img_h
             FROM clipboard_history ORDER BY pinned DESC, captured_at DESC LIMIT ?1"
        } else {
            "SELECT text, captured_at, pinned, kind, image_path, img_w, img_h
             FROM clipboard_history
             WHERE text LIKE ?1 ESCAPE '\\' OR (kind = 'image' AND ('img 图片 png' LIKE ?1 ESCAPE '\\'))
             ORDER BY pinned DESC, captured_at DESC LIMIT ?2"
        };
        let mut stmt = conn.prepare(sql).ok()?;
        let map_row = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)? != 0,
                row.get::<_, String>(3).unwrap_or_else(|_| "text".into()),
                row.get::<_, Option<String>>(4).unwrap_or(None),
                row.get::<_, i64>(5).unwrap_or(0).max(0) as u32,
                row.get::<_, i64>(6).unwrap_or(0).max(0) as u32,
            ))
        };
        let rows: Vec<_> = if filter.is_empty() {
            stmt.query_map(params![limit as i64], map_row).ok()?.flatten().collect()
        } else {
            let pat = format!("%{}%", filter.replace('%', "\\%").replace('_', "\\_"));
            stmt.query_map(params![pat, limit as i64], map_row).ok()?.flatten().collect()
        };
        Some(rows.into_iter().filter_map(row_to_entry).collect())
    }
}

fn open_clipboard_db(path: &PathBuf, initial: &mut VecDeque<ClipboardEntry>) -> Option<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path).ok()?;
    let _ = conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -256;
         CREATE TABLE IF NOT EXISTS clipboard_history (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             text TEXT NOT NULL UNIQUE,
             captured_at INTEGER NOT NULL,
             pinned INTEGER NOT NULL DEFAULT 0,
             kind TEXT NOT NULL DEFAULT 'text',
             image_path TEXT,
             img_w INTEGER NOT NULL DEFAULT 0,
             img_h INTEGER NOT NULL DEFAULT 0
         );",
    );
    let _ = conn.execute("ALTER TABLE clipboard_history ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE clipboard_history ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'", []);
    let _ = conn.execute("ALTER TABLE clipboard_history ADD COLUMN image_path TEXT", []);
    let _ = conn.execute("ALTER TABLE clipboard_history ADD COLUMN img_w INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE clipboard_history ADD COLUMN img_h INTEGER NOT NULL DEFAULT 0", []);

    if let Ok(mut stmt) = conn.prepare(
        "SELECT text, captured_at, pinned, kind, image_path, img_w, img_h
         FROM clipboard_history ORDER BY pinned DESC, captured_at DESC LIMIT ?1",
    ) {
        if let Ok(rows) = stmt.query_map(params![MAX_RAM_ITEMS as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)? != 0,
                row.get::<_, String>(3).unwrap_or_else(|_| "text".into()),
                row.get::<_, Option<String>>(4).unwrap_or(None),
                row.get::<_, i64>(5).unwrap_or(0).max(0) as u32,
                row.get::<_, i64>(6).unwrap_or(0).max(0) as u32,
            ))
        }) {
            for entry in rows.flatten().filter_map(row_to_entry) {
                initial.push_back(entry);
            }
        }
    }
    Some(conn)
}

fn row_to_entry(
    (key, ts, pinned, kind, img_path, img_w, img_h): (String, i64, bool, String, Option<String>, u32, u32),
) -> Option<ClipboardEntry> {
    if kind == "image" {
        let p = img_path?;
        if !std::path::Path::new(&p).is_file() {
            return None;
        }
        return Some(ClipboardEntry {
            key,
            payload: ClipboardPayload::Image {
                path: p,
                width: img_w,
                height: img_h,
            },
            preview_line: format!("[图片] {img_w}×{img_h} PNG"),
            captured_at_unix: ts,
            pinned,
        });
    }
    let char_count = key.chars().count();
    let preview_line = make_preview_line(&key);
    Some(ClipboardEntry {
        key: key.clone(),
        payload: ClipboardPayload::Text { text: key, char_count },
        preview_line,
        captured_at_unix: ts,
        pinned,
    })
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn make_preview_line(s: &str) -> String {
    let single: String = s
        .chars()
        .map(|c| if c == '\r' || c == '\n' || c == '\t' { ' ' } else { c })
        .take(110)
        .collect();
    let trimmed = single.trim();
    if s.chars().count() > 110 {
        format!("{trimmed}...")
    } else {
        trimmed.to_string()
    }
}
