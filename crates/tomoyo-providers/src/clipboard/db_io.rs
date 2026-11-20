//! SQLite schema bootstrap, row decoding, and bounded JSON backup/restore helpers for ClipboardStore.

use super::filter::{classify_text_subtype, matches_clip_tokens, ClipboardSubtype, ParsedClipQuery};
use super::store::{ClipboardEntry, ClipboardPayload, MAX_DISK_IMAGES, MAX_ITEM_BYTES, MAX_RAM_ITEMS};
use chrono::Local;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct BackupRecord {
    pub kind: String,
    pub text: String,
    pub captured_at: i64,
    pub pinned: bool,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub source_app: String,
}

pub(super) type RawDbRow = (
    String,
    i64,
    bool,
    String,
    Option<String>,
    u32,
    u32,
    String,
    String,
    String,
    String,
    Option<String>,
);

pub(super) fn parse_db_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawDbRow> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)? != 0,
        row.get::<_, String>(3).unwrap_or_else(|_| "text".into()),
        row.get::<_, Option<String>>(4).unwrap_or(None),
        row.get::<_, i64>(5).unwrap_or(0).max(0) as u32,
        row.get::<_, i64>(6).unwrap_or(0).max(0) as u32,
        row.get::<_, String>(7).unwrap_or_default(),
        row.get::<_, String>(8).unwrap_or_default(),
        row.get::<_, String>(9).unwrap_or_default(),
        row.get::<_, String>(10).unwrap_or_default(),
        row.get::<_, Option<String>>(11).unwrap_or(None),
    ))
}

pub(super) fn open_clipboard_db(
    path: &Path,
    initial: &mut VecDeque<ClipboardEntry>,
) -> Option<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path).ok()?;
    let _ = conn.busy_timeout(Duration::from_millis(3000));
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
             img_h INTEGER NOT NULL DEFAULT 0,
             subtype TEXT NOT NULL DEFAULT 'text',
             tag TEXT NOT NULL DEFAULT '',
             note TEXT NOT NULL DEFAULT '',
             source_app TEXT NOT NULL DEFAULT '',
             file_paths TEXT
         );",
    );
    for col in [
        "ALTER TABLE clipboard_history ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE clipboard_history ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'",
        "ALTER TABLE clipboard_history ADD COLUMN image_path TEXT",
        "ALTER TABLE clipboard_history ADD COLUMN img_w INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE clipboard_history ADD COLUMN img_h INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE clipboard_history ADD COLUMN subtype TEXT NOT NULL DEFAULT 'text'",
        "ALTER TABLE clipboard_history ADD COLUMN tag TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE clipboard_history ADD COLUMN note TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE clipboard_history ADD COLUMN source_app TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE clipboard_history ADD COLUMN file_paths TEXT",
    ] {
        let _ = conn.execute(col, []);
    }
    let _ = conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_clip_pin_ts ON clipboard_history(pinned DESC, captured_at DESC);
         CREATE INDEX IF NOT EXISTS idx_clip_tag ON clipboard_history(tag);",
    );
    load_recent_rows(&conn, MAX_RAM_ITEMS, initial);
    Some(conn)
}

pub(super) fn load_recent_rows(
    conn: &Connection,
    limit: usize,
    out: &mut VecDeque<ClipboardEntry>,
) {
    if let Ok(mut stmt) = conn.prepare(
        "SELECT text, captured_at, pinned, kind, image_path, img_w, img_h, subtype, tag, note, source_app, file_paths
         FROM clipboard_history ORDER BY pinned DESC, captured_at DESC LIMIT ?1",
    ) {
        if let Ok(rows) = stmt.query_map(params![limit as i64], parse_db_row) {
            for entry in rows.flatten().filter_map(row_to_entry) {
                out.push_back(entry);
            }
        }
    }
}

pub(super) fn row_to_entry(r: RawDbRow) -> Option<ClipboardEntry> {
    let (key, ts, pinned, kind, img_path, img_w, img_h, raw_sub, tag, note, source_app, file_paths) = r;
    if kind == "image" {
        let p = img_path?;
        if !Path::new(&p).is_file() {
            return None;
        }
        return Some(ClipboardEntry {
            key,
            payload: ClipboardPayload::Image { path: p, width: img_w, height: img_h },
            preview_line: format!("[图片] {img_w}×{img_h} PNG"),
            captured_at_unix: ts,
            pinned,
            tag,
            note,
            source_app,
        });
    }
    if kind == "files" {
        let paths: Vec<String> = file_paths
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_else(|| {
                key.strip_prefix("files:")
                    .unwrap_or("")
                    .lines()
                    .map(String::from)
                    .collect()
            });
        let preview_line = make_files_preview(&paths);
        return Some(ClipboardEntry {
            key,
            payload: ClipboardPayload::Files { paths },
            preview_line,
            captured_at_unix: ts,
            pinned,
            tag,
            note,
            source_app,
        });
    }
    let subtype = if raw_sub.is_empty() || raw_sub == "text" {
        classify_text_subtype(&key)
    } else {
        ClipboardSubtype::from_db_str(&raw_sub)
    };
    let char_count = key.chars().count();
    let preview_line = make_preview_line(&key);
    Some(ClipboardEntry {
        key: key.clone(),
        payload: ClipboardPayload::Text { text: key, char_count, subtype },
        preview_line,
        captured_at_unix: ts,
        pinned,
        tag,
        note,
        source_app,
    })
}

pub(super) fn import_records_locked(conn: &Connection, raw: &str) -> Result<usize, String> {
    if raw.len() > 4 * 1024 * 1024 {
        return Err("导入文件超过 4 MB 上限".into());
    }
    let recs: Vec<BackupRecord> =
        serde_json::from_str(raw).map_err(|e| format!("JSON 格式无效: {e}"))?;
    let mut imported = 0usize;
    for rec in recs.into_iter().take(1000) {
        if rec.kind != "text" || rec.text.trim().is_empty() {
            continue;
        }
        let text = truncate_utf8(&rec.text, MAX_ITEM_BYTES);
        let sub = classify_text_subtype(&text);
        let tag = sanitize_tag(&rec.tag);
        let note = sanitize_note(&rec.note);
        let src = sanitize_source(&rec.source_app);
        let ts = if rec.captured_at > 0 { rec.captured_at } else { Local::now().timestamp() };
        if conn
            .execute(
                "INSERT INTO clipboard_history (text, captured_at, pinned, kind, subtype, tag, note, source_app)
                 VALUES (?1, ?2, ?3, 'text', ?4, ?5, ?6, ?7)
                 ON CONFLICT(text) DO UPDATE SET
                   pinned = MAX(clipboard_history.pinned, excluded.pinned),
                   tag = CASE WHEN excluded.tag != '' THEN excluded.tag ELSE clipboard_history.tag END,
                   note = CASE WHEN excluded.note != '' THEN excluded.note ELSE clipboard_history.note END",
                params![text, ts, i64::from(rec.pinned), sub.as_str(), tag, note, src],
            )
            .is_ok()
        {
            imported += 1;
        }
    }
    Ok(imported)
}

pub(super) fn prune_sqlite_locked(conn: &Connection, max_persisted: usize) {
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
        "DELETE FROM clipboard_history WHERE pinned = 0 AND kind != 'image' AND id NOT IN (
            SELECT id FROM clipboard_history ORDER BY pinned DESC, captured_at DESC LIMIT ?1
        )",
        params![max_persisted as i64],
    );
}

pub(super) fn query_sqlite_filtered(
    conn: &Connection,
    q: &ParsedClipQuery,
    cap: usize,
) -> Option<Vec<ClipboardEntry>> {
    let mut clauses: Vec<&'static str> = Vec::new();
    if q.pinned_only {
        clauses.push("pinned = 1");
    }
    if let Some(st) = q.subtype {
        clauses.push(match st {
            ClipboardSubtype::Image => "kind = 'image'",
            ClipboardSubtype::Files => "(kind = 'files' OR subtype = 'path')",
            ClipboardSubtype::Link => "subtype = 'link'",
            ClipboardSubtype::Color => "subtype = 'color'",
            ClipboardSubtype::Code => "subtype = 'code'",
            ClipboardSubtype::Path => "subtype = 'path'",
            ClipboardSubtype::Text => "kind = 'text'",
        });
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    let fetch_limit = if q.tokens.is_empty() && q.tag.is_none() { cap } else { 2000 };
    let sql = format!(
        "SELECT text, captured_at, pinned, kind, image_path, img_w, img_h, subtype, tag, note, source_app, file_paths
         FROM clipboard_history {where_sql} ORDER BY pinned DESC, captured_at DESC LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql).ok()?;
    let rows = stmt.query_map(params![fetch_limit as i64], parse_db_row).ok()?;
    let mut out = Vec::with_capacity(cap.min(64));
    for entry in rows.flatten().filter_map(row_to_entry) {
        if entry_matches_query(&entry, q) {
            out.push(entry);
            if out.len() >= cap {
                break;
            }
        }
    }
    Some(out)
}

pub(super) fn entry_matches_query(e: &ClipboardEntry, q: &ParsedClipQuery) -> bool {
    if q.pinned_only && !e.pinned {
        return false;
    }
    if let Some(st) = q.subtype {
        let ok = match st {
            ClipboardSubtype::Files => matches!(e.subtype(), ClipboardSubtype::Files | ClipboardSubtype::Path),
            ClipboardSubtype::Text => matches!(e.payload, ClipboardPayload::Text { .. }),
            other => e.subtype() == other,
        };
        if !ok {
            return false;
        }
    }
    if let Some(tag) = &q.tag {
        if !e.tag.to_lowercase().contains(tag.as_str()) {
            return false;
        }
    }
    if q.tokens.is_empty() {
        return true;
    }
    let hay = format!("{} {} {} {} {}", e.key, e.preview_line, e.note, e.tag, e.source_app).to_lowercase();
    matches_clip_tokens(&hay, &format!("{} {} {}", e.preview_line, e.note, e.tag), &q.tokens)
}

pub fn sanitize_tag(s: &str) -> String {
    s.trim()
        .trim_start_matches('#')
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .take(24)
        .collect()
}

pub fn sanitize_note(s: &str) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(160).collect()
}

pub(super) fn sanitize_source(s: &str) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(48).collect()
}

pub(super) fn make_files_preview(paths: &[String]) -> String {
    let first = paths
        .first()
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if paths.len() <= 1 {
        format!("[文件] {first}")
    } else {
        format!("[{} 个文件] {first} 等", paths.len())
    }
}

pub(super) fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

pub(super) fn make_preview_line(s: &str) -> String {
    let single: String = s
        .chars()
        .map(|c| if matches!(c, '\r' | '\n' | '\t') { ' ' } else { c })
        .take(110)
        .collect();
    let trimmed = single.trim();
    if s.chars().count() > 110 {
        format!("{trimmed}...")
    } else {
        trimmed.to_string()
    }
}
