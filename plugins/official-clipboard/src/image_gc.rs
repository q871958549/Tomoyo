//! Total disk byte-budget LRU eviction & orphan PNG sweeping for `clipboard-images/`.

use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub(crate) const MAX_DISK_IMAGE_BYTES: u64 = 48 * 1024 * 1024;

/// Enforces `max_bytes` across unpinned clipboard image files (evicts oldest unpinned first).
pub(crate) fn enforce_image_byte_budget(conn: &Connection, max_bytes: u64) -> usize {
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, pinned, image_path FROM clipboard_history
         WHERE kind = 'image' ORDER BY captured_at ASC, id ASC",
    ) else {
        return 0;
    };
    let rows: Vec<(i64, bool, String, u64)> = stmt
        .query_map([], |r| {
            let id: i64 = r.get(0)?;
            let pinned = r.get::<_, i64>(1).unwrap_or(0) != 0;
            let path: String = r.get::<_, Option<String>>(2)?.unwrap_or_default();
            let sz = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            Ok((id, pinned, path, sz))
        })
        .ok()
        .map(|it| it.flatten().collect())
        .unwrap_or_default();

    let mut total_bytes: u64 = rows.iter().map(|(_, _, _, sz)| *sz).sum();
    if total_bytes <= max_bytes {
        return 0;
    }

    let mut evicted = 0usize;
    for (id, pinned, path, sz) in rows {
        if total_bytes <= max_bytes {
            break;
        }
        if pinned {
            continue;
        }
        let _ = std::fs::remove_file(&path);
        let _ = conn.execute("DELETE FROM clipboard_history WHERE id = ?1", params![id]);
        total_bytes = total_bytes.saturating_sub(sz);
        evicted += 1;
    }
    evicted
}

/// Removes unreferenced `img_*.png` / `capture_*.png` orphans in `images_dir` older than `min_age_secs`.
pub(crate) fn sweep_orphan_images(conn: &Connection, images_dir: &Path, min_age_secs: u64) -> usize {
    if !images_dir.is_dir() {
        return 0;
    }
    let mut tracked: HashSet<String> = HashSet::new();
    if let Ok(mut stmt) = conn.prepare("SELECT image_path FROM clipboard_history WHERE kind = 'image'")
        && let Ok(rows) = stmt.query_map([], |r| r.get::<_, Option<String>>(0))
    {
        for raw in rows.flatten().flatten() {
            tracked.insert(raw.clone());
            if let Some(name) = Path::new(&raw).file_name().and_then(|n| n.to_str()) {
                tracked.insert(name.to_string());
            }
        }
    }

    let cutoff = SystemTime::now().checked_sub(Duration::from_secs(min_age_secs));
    let Ok(entries) = std::fs::read_dir(images_dir) else {
        return 0;
    };
    let mut removed = 0usize;
    for ent in entries.flatten() {
        let path = ent.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !(name.ends_with(".png") && (name.starts_with("img_") || name.starts_with("capture_"))) {
            continue;
        }
        let full = path.to_string_lossy().into_owned();
        if tracked.contains(&full) || tracked.contains(name) {
            continue;
        }
        if let Some(limit) = cutoff
            && let Ok(meta) = ent.metadata()
            && let Ok(mtime) = meta.modified()
            && mtime > limit
        {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_byte_budget_keeping_pinned_and_sweeping_orphans() {
        let dir = std::env::temp_dir().join(format!("tomoyo-clip-gc-{}", std::process::id()));
        let img_dir = dir.join("clipboard-images");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&img_dir).unwrap();

        let p_pinned = img_dir.join("img_pinned.png");
        let p_old = img_dir.join("img_old.png");
        let p_new = img_dir.join("img_new.png");
        let p_orphan = img_dir.join("img_orphan.png");
        std::fs::write(&p_pinned, vec![1u8; 60]).unwrap();
        std::fs::write(&p_old, vec![2u8; 60]).unwrap();
        std::fs::write(&p_new, vec![3u8; 60]).unwrap();
        std::fs::write(&p_orphan, vec![4u8; 60]).unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE clipboard_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL,
                captured_at INTEGER NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                kind TEXT NOT NULL DEFAULT 'text',
                image_path TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO clipboard_history (text, captured_at, pinned, kind, image_path) VALUES ('k1', 100, 1, 'image', ?1)",
            params![p_pinned.to_string_lossy().as_ref()],
        ).unwrap();
        conn.execute(
            "INSERT INTO clipboard_history (text, captured_at, pinned, kind, image_path) VALUES ('k2', 200, 0, 'image', ?1)",
            params![p_old.to_string_lossy().as_ref()],
        ).unwrap();
        conn.execute(
            "INSERT INTO clipboard_history (text, captured_at, pinned, kind, image_path) VALUES ('k3', 300, 0, 'image', ?1)",
            params![p_new.to_string_lossy().as_ref()],
        ).unwrap();

        let evicted = enforce_image_byte_budget(&conn, 120);
        assert_eq!(evicted, 1);
        assert!(p_pinned.is_file());
        assert!(!p_old.is_file());
        assert!(p_new.is_file());

        let orphans = sweep_orphan_images(&conn, &img_dir, 0);
        assert_eq!(orphans, 1);
        assert!(!p_orphan.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
