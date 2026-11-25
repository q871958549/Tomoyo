//! Local Markdown notes storage, parsing, tag extraction, and safe file creation helpers.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::Value;

const MAX_SCAN_NOTES: usize = 200;
const MAX_NOTE_BYTES: u64 = 65_536;

pub(crate) struct NoteDoc {
    pub path: PathBuf,
    pub file_name: String,
    pub title: String,
    pub snippet: String,
    pub body: String,
    pub tags: Vec<String>,
    pub modified_secs: u64,
}

pub(crate) fn notes_dir_from_settings(settings: &BTreeMap<String, Value>) -> PathBuf {
    settings
        .get("notes_dir")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(notes_dir)
}

pub(crate) fn default_tag_from_settings(settings: &BTreeMap<String, Value>) -> String {
    let raw = settings
        .get("default_tag")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if raw.is_empty() {
        String::new()
    } else if raw.starts_with('#') {
        raw.to_string()
    } else {
        format!("#{raw}")
    }
}

pub(crate) fn notes_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("TOMOYO_NOTES_DIR") {
        let t = custom.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let local_root = base.join("Tomoyo");
    if let Ok(sync_raw) = fs::read_to_string(local_root.join("sync_dir.txt")) {
        let sync_t = sync_raw.trim();
        if !sync_t.is_empty() {
            let sp = PathBuf::from(sync_t);
            if sp.is_dir() {
                return sp.join("notes");
            }
        }
    }
    local_root.join("notes")
}

pub(crate) fn load_notes_from(dir: &Path) -> Vec<NoteDoc> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new(); };
    let mut docs = Vec::new();
    for ent in rd.flatten().take(MAX_SCAN_NOTES) {
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("txt"));
        if !ext_ok {
            continue;
        }
        let Ok(meta) = ent.metadata() else { continue; };
        if meta.len() > MAX_NOTE_BYTES {
            continue;
        }
        let Ok(body) = fs::read_to_string(&path) else { continue; };
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = extract_title(&body, &file_name);
        let tags = extract_tags(&body);
        let snippet = extract_snippet(&body, &title);
        let modified_secs = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        docs.push(NoteDoc { path, file_name, title, snippet, body, tags, modified_secs });
    }
    docs.sort_by_key(|a| std::cmp::Reverse(a.modified_secs));
    docs
}

pub(crate) fn extract_title(body: &str, file_name: &str) -> String {
    for line in body.lines() {
        let t = line.trim();
        if !t.is_empty() {
            return t.trim_start_matches('#').trim().chars().take(64).collect();
        }
    }
    file_name.trim_end_matches(".md").to_string()
}

pub(crate) fn extract_tags(body: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for tok in body.split_whitespace() {
        if let Some(rest) = tok.strip_prefix('#')
            && !rest.is_empty()
            && !rest.starts_with('#')
            && rest.len() <= 24
        {
            let clean = rest.trim_matches(|c: char| matches!(c, ',' | '.' | ';' | '，' | '。'));
            if !clean.is_empty() && !tags.iter().any(|x: &String| x.eq_ignore_ascii_case(clean)) {
                tags.push(format!("#{clean}"));
            }
        }
    }
    tags
}

pub(crate) fn extract_snippet(body: &str, title: &str) -> String {
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() || t.trim_start_matches('#').trim() == title {
            continue;
        }
        return t.chars().take(90).collect();
    }
    "本地 Markdown 备忘录".into()
}

fn sanitize_slug(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || ('\u{4e00}'..='\u{9fff}').contains(&c) {
                c
            } else {
                '-'
            }
        })
        .take(36)
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() { "note".into() } else { trimmed.to_string() }
}

pub(crate) fn parse_create_input(args: &str) -> Option<(String, String)> {
    let body = args
        .strip_prefix('+')
        .or_else(|| args.strip_prefix("new "))
        .or_else(|| args.strip_prefix("add "))
        .map(str::trim)?;
    if body.is_empty() {
        return None;
    }
    if let Some((t, c)) = body.split_once("::").or_else(|| body.split_once("：：")) {
        let title = t.trim();
        let content = c.trim();
        if !title.is_empty() {
            return Some((title.to_string(), content.to_string()));
        }
    }
    let title: String = body.chars().take(28).collect();
    Some((title, body.to_string()))
}

pub(crate) fn create_note_file(dir: &Path, title: &str, content: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("创建备忘录目录失败: {e}"))?;
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = dir.join(format!("{ts}-{}.md", sanitize_slug(title)));
    let md = if content.trim().is_empty() || content.trim() == title.trim() {
        format!("# {}\n", title.trim())
    } else {
        format!("# {}\n\n{}\n", title.trim(), content.trim())
    };
    fs::write(&path, md).map_err(|e| format!("写入 Markdown 失败: {e}"))?;
    Ok(path)
}
