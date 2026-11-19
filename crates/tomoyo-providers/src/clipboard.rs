use chrono::{Local, TimeZone};
use rusqlite::{params, Connection};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use tomoyo_core::{Lang, Provider, TomoyoConfig};
use tomoyo_protocol::{
    ActionKind, ActionSpec, ClipboardSnapshot, IconSpec, InternalCommandKind, PreviewSpec,
    QueryContext, ResultCategory, ResultItem,
};

/// Hard memory bounds for in-RAM hot cache (< 1 MB) + bounded SQLite + max 50 disk images.
const MAX_RAM_ITEMS: usize = 64;
const MAX_ITEM_BYTES: usize = 16 * 1024;
const MAX_DISK_IMAGES: usize = 50;

#[derive(Debug, Clone)]
enum ClipboardPayload {
    Text {
        text: String,
        char_count: usize,
    },
    Image {
        path: String,
        width: u32,
        height: u32,
    },
}

#[derive(Debug, Clone)]
struct ClipboardEntry {
    /// Unique dedup/pin key (`text` for text items, `img:<hash>` for images).
    key: String,
    payload: ClipboardPayload,
    preview_line: String,
    captured_at_unix: i64,
    pinned: bool,
}

/// Event-driven, strictly memory-bounded SQLite text + image clipboard history provider.
/// Images are stored as PNG files on disk with strict LRU eviction (ZERO raw image RAM!).
pub struct ClipboardProvider {
    history: RwLock<VecDeque<ClipboardEntry>>,
    db: Option<Mutex<Connection>>,
    max_persisted: usize,
    config: Arc<RwLock<TomoyoConfig>>,
}

impl ClipboardProvider {
    pub fn with_sqlite(
        db_path: Option<PathBuf>,
        max_persisted: usize,
        config: Arc<RwLock<TomoyoConfig>>,
    ) -> Self {
        let mut initial = VecDeque::with_capacity(32);
        let db = db_path.and_then(|path| open_clipboard_db(&path, &mut initial).map(Mutex::new));
        Self {
            history: RwLock::new(initial),
            db,
            max_persisted: max_persisted.clamp(50, 2000),
            config,
        }
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }

    /// Pushes a text clipboard entry, deduplicating while preserving pinned status.
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

    /// Pushes a disk-backed screenshot/image clipboard entry (ZERO pixel bytes held in RAM!).
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
        // Evict excess unpinned disk images first and delete their PNG files from disk!
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

        // Enforce overall max unpinned records
        let _ = conn.execute(
            "DELETE FROM clipboard_history WHERE pinned = 0 AND kind = 'text' AND id NOT IN (
                SELECT id FROM clipboard_history ORDER BY pinned DESC, captured_at DESC LIMIT ?1
            )",
            params![self.max_persisted as i64],
        );
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

    pub fn latest_snapshot(&self) -> Option<ClipboardSnapshot> {
        let guard = self.history.read().ok()?;
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

    fn query_sqlite(&self, filter: &str, limit: usize) -> Option<Vec<ClipboardEntry>> {
        let db_lock = self.db.as_ref()?;
        let conn = db_lock.lock().ok()?;
        let mut out = Vec::new();
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
            let key: String = row.get(0)?;
            let ts: i64 = row.get(1)?;
            let pinned: bool = row.get::<_, i64>(2)? != 0;
            let kind: String = row.get(3).unwrap_or_else(|_| "text".into());
            let img_path: Option<String> = row.get(4).unwrap_or(None);
            let img_w: u32 = row.get::<_, i64>(5).unwrap_or(0).max(0) as u32;
            let img_h: u32 = row.get::<_, i64>(6).unwrap_or(0).max(0) as u32;
            Ok((key, ts, pinned, kind, img_path, img_w, img_h))
        };

        let collected: Vec<_> = if filter.is_empty() {
            stmt.query_map(params![limit as i64], map_row)
                .ok()?
                .flatten()
                .collect()
        } else {
            let pattern = format!("%{}%", filter.replace('%', "\\%").replace('_', "\\_"));
            stmt.query_map(params![pattern, limit as i64], map_row)
                .ok()?
                .flatten()
                .collect()
        };

        for (key, ts, pinned, kind, img_path, img_w, img_h) in collected {
            if kind == "image" {
                if let Some(p) = img_path {
                    if std::path::Path::new(&p).is_file() {
                        out.push(ClipboardEntry {
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
                }
            } else {
                let char_count = key.chars().count();
                let preview_line = make_preview_line(&key);
                out.push(ClipboardEntry {
                    key: key.clone(),
                    payload: ClipboardPayload::Text {
                        text: key,
                        char_count,
                    },
                    preview_line,
                    captured_at_unix: ts,
                    pinned,
                });
            }
        }
        Some(out)
    }
}

impl Provider for ClipboardProvider {
    fn id(&self) -> &'static str {
        "builtin.clipboard"
    }

    fn name(&self) -> &'static str {
        "Clipboard History (SQLite Text + Image)"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["cb"]
    }

    fn supports_global(&self) -> bool {
        false
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let lang = self.lang();
        let filter = ctx.args.trim().to_lowercase();
        let cap = ctx.limit.min(30);

        let entries: Vec<ClipboardEntry> = if let Some(db_items) = self.query_sqlite(&filter, cap) {
            db_items
        } else if let Ok(guard) = self.history.read() {
            let mut v: Vec<ClipboardEntry> = guard
                .iter()
                .filter(|e| {
                    filter.is_empty()
                        || e.preview_line.to_lowercase().contains(&filter)
                        || e.key.to_lowercase().contains(&filter)
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
        } else {
            Vec::new()
        };

        if entries.is_empty() {
            return vec![ResultItem {
                id: "cb:empty".into(),
                provider_id: self.id().into(),
                title: if filter.is_empty() {
                    lang.pick("剪贴板历史为空", "Clipboard history is empty").into()
                } else {
                    match lang {
                        Lang::Zh => format!("未找到包含「{filter}」的剪贴板记录"),
                        Lang::En => format!("No clipboard entries matching `{filter}`"),
                    }
                },
                subtitle: lang
                    .pick(
                        "支持文本与截图图片 · 磁盘缩略图 LRU（零图片常驻内存）",
                        "Supports text & images · Disk thumbnail LRU (0 image RAM)",
                    )
                    .into(),
                category: ResultCategory::Clipboard,
                score: 5_000,
                icon: IconSpec::Monogram("CB".into()),
                badge: Some("0".into()),
                copy_text: None,
                actions: vec![],
                preview: None,
            }];
        }

        entries
            .into_iter()
            .enumerate()
            .map(|(idx, e)| {
                let time_str = Local
                    .timestamp_opt(e.captured_at_unix, 0)
                    .single()
                    .map(|dt| dt.format("%m-%d %H:%M:%S").to_string())
                    .unwrap_or_default();
                let pin_prefix = if e.pinned { "★ " } else { "" };

                match e.payload {
                    ClipboardPayload::Text { text, char_count } => {
                        let line_count = text.lines().count().max(1);
                        let badge = if e.pinned {
                            lang.pick("置顶", "PIN").to_string()
                        } else {
                            format!("#{}", idx + 1)
                        };
                        let subtitle = match lang {
                            Lang::Zh => format!(
                                "{time_str} · {char_count} 字符 · {line_count} 行 · 回车直贴原窗口"
                            ),
                            Lang::En => format!(
                                "{time_str} · {char_count} chars · {line_count} lines · Enter to paste"
                            ),
                        };
                        ResultItem {
                            id: format!("cb:{idx}:{}", e.captured_at_unix),
                            provider_id: self.id().into(),
                            title: format!("{pin_prefix}{}", e.preview_line),
                            subtitle,
                            category: ResultCategory::Clipboard,
                            score: if e.pinned { 9_400 } else { 8_500 } - (idx as i64 * 20),
                            icon: IconSpec::Monogram(if e.pinned { "★".into() } else { "CB".into() }),
                            badge: Some(badge),
                            copy_text: Some(text.clone()),
                            actions: vec![
                                ActionSpec {
                                    id: "paste".into(),
                                    label: lang.pick("粘贴到原焦点窗口", "Paste into Active App").into(),
                                    shortcut: Some("Enter".into()),
                                    kind: ActionKind::CopyText {
                                        text: text.clone(),
                                        paste_immediately: true,
                                    },
                                },
                                ActionSpec {
                                    id: "copy".into(),
                                    label: lang.pick("仅复制到剪贴板", "Copy Only").into(),
                                    shortcut: Some("Ctrl+Enter".into()),
                                    kind: ActionKind::CopyText {
                                        text: text.clone(),
                                        paste_immediately: false,
                                    },
                                },
                                ActionSpec {
                                    id: "pin".into(),
                                    label: if e.pinned {
                                        lang.pick("取消置顶", "Unpin").into()
                                    } else {
                                        lang.pick("置顶收藏", "Pin Favorite").into()
                                    },
                                    shortcut: None,
                                    kind: ActionKind::InternalCommand {
                                        command: InternalCommandKind::ToggleClipboardPin {
                                            id_key: e.key,
                                        },
                                    },
                                },
                            ],
                            preview: Some(PreviewSpec::Text {
                                title: match lang {
                                    Lang::Zh => format!("文本剪贴板 #{} ({time_str})", idx + 1),
                                    Lang::En => format!("Clipboard #{} ({time_str})", idx + 1),
                                },
                                body: text,
                            }),
                        }
                    }
                    ClipboardPayload::Image {
                        path,
                        width,
                        height,
                    } => {
                        let badge = if e.pinned {
                            lang.pick("置顶图", "PIN IMG").to_string()
                        } else {
                            lang.pick("图片", "IMG").to_string()
                        };
                        let subtitle = match lang {
                            Lang::Zh => format!(
                                "{time_str} · {width}×{height} 截图/图片 · 回车直接贴图到原窗口"
                            ),
                            Lang::En => format!(
                                "{time_str} · {width}×{height} Image · Enter to paste image"
                            ),
                        };
                        ResultItem {
                            id: format!("cb:img:{idx}:{}", e.captured_at_unix),
                            provider_id: self.id().into(),
                            title: format!("{pin_prefix}{}", e.preview_line),
                            subtitle,
                            category: ResultCategory::Clipboard,
                            score: if e.pinned { 9_450 } else { 8_520 } - (idx as i64 * 20),
                            icon: IconSpec::Monogram("IMG".into()),
                            badge: Some(badge),
                            copy_text: Some(path.clone()),
                            actions: vec![
                                ActionSpec {
                                    id: "paste_img".into(),
                                    label: lang.pick("粘贴图片到原焦点窗口", "Paste Image into Active App").into(),
                                    shortcut: Some("Enter".into()),
                                    kind: ActionKind::CopyImage {
                                        path: path.clone(),
                                        paste_immediately: true,
                                    },
                                },
                                ActionSpec {
                                    id: "copy_img".into(),
                                    label: lang.pick("仅复制图片到剪贴板", "Copy Image Only").into(),
                                    shortcut: Some("Ctrl+Enter".into()),
                                    kind: ActionKind::CopyImage {
                                        path: path.clone(),
                                        paste_immediately: false,
                                    },
                                },
                                ActionSpec {
                                    id: "open_img".into(),
                                    label: lang.pick("用系统看图打开", "Open Image").into(),
                                    shortcut: None,
                                    kind: ActionKind::LaunchPath {
                                        path: path.clone(),
                                        args: None,
                                        working_dir: None,
                                    },
                                },
                                ActionSpec {
                                    id: "reveal_img".into(),
                                    label: lang.pick("在资源管理器定位图片", "Reveal PNG in Explorer").into(),
                                    shortcut: None,
                                    kind: ActionKind::RevealInExplorer { path: path.clone() },
                                },
                                ActionSpec {
                                    id: "pin_img".into(),
                                    label: if e.pinned {
                                        lang.pick("取消置顶", "Unpin").into()
                                    } else {
                                        lang.pick("置顶收藏", "Pin Favorite").into()
                                    },
                                    shortcut: None,
                                    kind: ActionKind::InternalCommand {
                                        command: InternalCommandKind::ToggleClipboardPin {
                                            id_key: e.key,
                                        },
                                    },
                                },
                            ],
                            preview: Some(PreviewSpec::ImagePreview {
                                path,
                                width,
                                height,
                            }),
                        }
                    }
                }
            })
            .collect()
    }

    fn trim_memory(&self) {
        if let Ok(mut guard) = self.history.write() {
            guard.shrink_to_fit();
        }
        if let Some(db_lock) = &self.db {
            if let Ok(conn) = db_lock.lock() {
                let _ = conn.execute_batch("PRAGMA shrink_memory;");
            }
        }
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
    // Non-destructive schema migrations for v0.2/v0.3 databases
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
            for (key, ts, pinned, kind, img_path, img_w, img_h) in rows.flatten() {
                if kind == "image" {
                    if let Some(p) = img_path {
                        if std::path::Path::new(&p).is_file() {
                            initial.push_back(ClipboardEntry {
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
                    }
                } else {
                    let char_count = key.chars().count();
                    let preview_line = make_preview_line(&key);
                    initial.push_back(ClipboardEntry {
                        key: key.clone(),
                        payload: ClipboardPayload::Text {
                            text: key,
                            char_count,
                        },
                        preview_line,
                        captured_at_unix: ts,
                        pinned,
                    });
                }
            }
        }
    }
    Some(conn)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_clipboard_persists_text_and_images_and_pins() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_clip_v4_{}.db", std::process::id()));
        let img_tmp = std::env::temp_dir().join(format!("tomoyo_clip_img_{}.png", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::write(&img_tmp, b"fake_png_bytes");
        let cfg = Arc::new(RwLock::new(TomoyoConfig::default()));

        {
            let cb = ClipboardProvider::with_sqlite(Some(tmp.clone()), 100, cfg.clone());
            cb.push_text("hello tomoyo rust".into());
            cb.push_image(img_tmp.to_string_lossy().into_owned(), 640, 480, "abc123".into());
            assert!(cb.toggle_pin("hello tomoyo rust"));
            assert_eq!(cb.item_count(), 2);
            let ctx = QueryContext::new("cb", 1, 10).with_keyword("cb", "");
            let items = cb.query(&ctx);
            assert_eq!(items.len(), 2);
            assert!(items[0].title.contains("★"));
            assert!(items[1].title.contains("640×480"));
            cb.trim_memory();
        }

        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&img_tmp);
        let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
        let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    }
}
