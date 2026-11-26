//! Local-first Markdown notes repository: YAML frontmatter, archive box, daily scratchpad & backlinks.

use crate::frontmatter::{
    count_todos, extract_tags, extract_wikilinks, local_today_ymd, normalize_tag, parse_frontmatter,
    serialize_note_markdown, toggle_todo_in_body, unix_now_secs, NoteMeta,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const MAX_SCAN_NOTES: usize = 300;
const MAX_NOTE_BYTES: u64 = 128 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct NoteDoc {
    pub path: PathBuf,
    pub file_name: String,
    pub title: String,
    pub snippet: String,
    pub body: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub archived: bool,
    pub created_secs: u64,
    pub modified_secs: u64,
    pub source: String,
    pub todo_open: usize,
    pub todo_done: usize,
    pub wikilinks: Vec<String>,
    pub backlinks: Vec<(String, String)>,
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
    normalize_tag(raw).unwrap_or_default()
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

/// Validates note filename strictly to prevent directory traversal (`..`, `/`, `\`).
pub(crate) fn safe_note_path(dir: &Path, file_name: &str, archived: bool) -> Result<PathBuf, String> {
    let clean = file_name.trim();
    if clean.is_empty()
        || clean.contains("..")
        || clean.contains('/')
        || clean.contains('\\')
        || clean.contains(':')
    {
        return Err("无效的备忘录文件名".into());
    }
    let ext_ok = Path::new(clean)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("txt"));
    if !ext_ok {
        return Err("仅支持 .md / .txt 备忘录文件".into());
    }
    let base = if archived { dir.join(".archive") } else { dir.to_path_buf() };
    Ok(base.join(clean))
}

fn read_single_note(path: &Path, archived: bool) -> Option<NoteDoc> {
    let ext_ok = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("txt"));
    if !path.is_file() || !ext_ok {
        return None;
    }
    let meta_fs = fs::metadata(path).ok()?;
    if meta_fs.len() > MAX_NOTE_BYTES {
        return None;
    }
    let raw = fs::read_to_string(path).ok()?;
    let (fm, body) = parse_frontmatter(&raw);
    let file_name = path.file_name()?.to_string_lossy().into_owned();
    let title = extract_title(&body, &file_name);
    let tags = extract_tags(&fm.tags, &body);
    let snippet = extract_snippet(&body, &title);
    let (todo_open, todo_done) = count_todos(&body);
    let wikilinks = extract_wikilinks(&body);
    let fs_mtime = meta_fs
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let modified_secs = if fm.updated_secs > 0 { fm.updated_secs.max(fs_mtime) } else { fs_mtime };
    let created_secs = if fm.created_secs > 0 { fm.created_secs } else { modified_secs };
    Some(NoteDoc {
        path: path.to_path_buf(),
        file_name,
        title,
        snippet,
        body,
        tags,
        pinned: fm.pinned,
        archived,
        created_secs,
        modified_secs,
        source: fm.source,
        todo_open,
        todo_done,
        wikilinks,
        backlinks: Vec::new(),
    })
}

fn scan_folder(folder: &Path, archived: bool) -> Vec<NoteDoc> {
    let Ok(rd) = fs::read_dir(folder) else { return Vec::new(); };
    let mut docs = Vec::new();
    for ent in rd.flatten().take(MAX_SCAN_NOTES) {
        if let Some(doc) = read_single_note(&ent.path(), archived) {
            docs.push(doc);
        }
    }
    docs
}

fn populate_backlinks(docs: &mut [NoteDoc]) {
    let index: Vec<(String, String, String)> = docs
        .iter()
        .map(|d| {
            let stem = Path::new(&d.file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            (d.file_name.clone(), d.title.clone(), stem)
        })
        .collect();

    for i in 0..docs.len() {
        let my_links = docs[i].wikilinks.clone();
        if my_links.is_empty() {
            continue;
        }
        let src_file = docs[i].file_name.clone();
        let src_title = docs[i].title.clone();
        for link in &my_links {
            for (j, (_f_name, t_name, stem)) in index.iter().enumerate() {
                if i == j {
                    continue;
                }
                if (t_name.eq_ignore_ascii_case(link) || stem.eq_ignore_ascii_case(link))
                    && !docs[j].backlinks.iter().any(|(f, _)| f == &src_file)
                {
                    docs[j].backlinks.push((src_file.clone(), src_title.clone()));
                }
            }
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn load_notes_from(dir: &Path) -> Vec<NoteDoc> {
    load_notes_with_archive(dir, false)
}

pub(crate) fn load_notes_with_archive(dir: &Path, include_archived: bool) -> Vec<NoteDoc> {
    let mut active = scan_folder(dir, false);
    populate_backlinks(&mut active);
    if include_archived {
        active.extend(scan_folder(&dir.join(".archive"), true));
    }
    active.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| b.modified_secs.cmp(&a.modified_secs))
    });
    active
}

pub(crate) fn collect_tag_counts(docs: &[NoteDoc]) -> Vec<(String, usize)> {
    let mut map: BTreeMap<String, usize> = BTreeMap::new();
    for doc in docs.iter().filter(|d| !d.archived) {
        for tag in &doc.tags {
            *map.entry(tag.clone()).or_insert(0) += 1;
        }
    }
    let mut items: Vec<(String, usize)> = map.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items
}

pub(crate) fn extract_title(body: &str, file_name: &str) -> String {
    for line in body.lines() {
        let t = line.trim();
        if !t.is_empty() {
            return t.trim_start_matches('#').trim().chars().take(64).collect();
        }
    }
    file_name.trim_end_matches(".md").trim_end_matches(".txt").to_string()
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

fn compose_body_with_title(title: &str, body: &str) -> String {
    let clean_title = if title.trim().is_empty() { "未命名备忘" } else { title.trim() };
    let trimmed_body = body.trim_start_matches('\n');
    let mut lines = trimmed_body.lines();
    if let Some(first) = lines.next()
        && first.trim_start().starts_with("# ")
    {
        let rest: Vec<&str> = lines.collect();
        let rest_str = rest.join("\n").trim_start_matches('\n').to_string();
        if rest_str.trim().is_empty() {
            return format!("# {clean_title}\n");
        }
        return format!("# {clean_title}\n\n{rest_str}\n");
    }
    if trimmed_body.trim().is_empty() || trimmed_body.trim() == clean_title {
        format!("# {clean_title}\n")
    } else {
        format!("# {clean_title}\n\n{}\n", trimmed_body.trim_end())
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn create_note_file(dir: &Path, title: &str, content: &str) -> Result<PathBuf, String> {
    let doc = create_note_file_ex(dir, title, content, &[], false, "")?;
    Ok(doc.path)
}

pub(crate) fn create_note_file_ex(
    dir: &Path,
    title: &str,
    content: &str,
    tags: &[String],
    pinned: bool,
    source: &str,
) -> Result<NoteDoc, String> {
    fs::create_dir_all(dir).map_err(|e| format!("创建备忘录目录失败: {e}"))?;
    let now = unix_now_secs();
    let mut candidate = dir.join(format!("{now}-{}.md", sanitize_slug(title)));
    let mut seq = 1u32;
    while candidate.exists() {
        candidate = dir.join(format!("{now}-{seq}-{}.md", sanitize_slug(title)));
        seq += 1;
    }
    let body = compose_body_with_title(title, content);
    let merged_tags = extract_tags(tags, &body);
    let meta = NoteMeta {
        pinned,
        tags: merged_tags,
        created_secs: now,
        updated_secs: now,
        source: source.to_string(),
    };
    let md = serialize_note_markdown(&meta, &body);
    fs::write(&candidate, md).map_err(|e| format!("写入 Markdown 失败: {e}"))?;
    read_single_note(&candidate, false).ok_or_else(|| "读取新建备忘录失败".into())
}

pub(crate) fn save_note_file(
    dir: &Path,
    file_name: &str,
    title: &str,
    body: &str,
    tags: &[String],
    pinned: bool,
) -> Result<NoteDoc, String> {
    let path = safe_note_path(dir, file_name, false)?;
    let existing = fs::read_to_string(&path)
        .ok()
        .map(|raw| parse_frontmatter(&raw).0)
        .unwrap_or_default();
    let now = unix_now_secs();
    let full_body = compose_body_with_title(title, body);
    let meta = NoteMeta {
        pinned,
        tags: extract_tags(tags, &full_body),
        created_secs: if existing.created_secs > 0 { existing.created_secs } else { now },
        updated_secs: now,
        source: existing.source,
    };
    fs::write(&path, serialize_note_markdown(&meta, &full_body))
        .map_err(|e| format!("保存备忘录失败: {e}"))?;
    read_single_note(&path, false).ok_or_else(|| "保存后读取备忘录失败".into())
}

pub(crate) fn toggle_pin_note(dir: &Path, file_name: &str) -> Result<bool, String> {
    let path = safe_note_path(dir, file_name, false)?;
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取备忘录失败: {e}"))?;
    let (mut meta, body) = parse_frontmatter(&raw);
    meta.pinned = !meta.pinned;
    meta.updated_secs = unix_now_secs();
    if meta.created_secs == 0 {
        meta.created_secs = meta.updated_secs;
    }
    fs::write(&path, serialize_note_markdown(&meta, &body))
        .map_err(|e| format!("更新置顶状态失败: {e}"))?;
    Ok(meta.pinned)
}

pub(crate) fn toggle_todo_note(dir: &Path, file_name: &str, line_idx: usize) -> Result<NoteDoc, String> {
    let path = safe_note_path(dir, file_name, false)?;
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取备忘录失败: {e}"))?;
    let (mut meta, body) = parse_frontmatter(&raw);
    let updated_body =
        toggle_todo_in_body(&body, line_idx).ok_or_else(|| "未找到对应待办行".to_string())?;
    meta.updated_secs = unix_now_secs();
    fs::write(&path, serialize_note_markdown(&meta, &updated_body))
        .map_err(|e| format!("更新待办状态失败: {e}"))?;
    read_single_note(&path, false).ok_or_else(|| "读取更新后备忘录失败".into())
}

pub(crate) fn archive_note(dir: &Path, file_name: &str) -> Result<(), String> {
    let src = safe_note_path(dir, file_name, false)?;
    let dst_dir = dir.join(".archive");
    fs::create_dir_all(&dst_dir).map_err(|e| format!("创建归档目录失败: {e}"))?;
    let dst = safe_note_path(dir, file_name, true)?;
    fs::rename(&src, &dst).map_err(|e| format!("归档备忘录失败: {e}"))
}

pub(crate) fn restore_note(dir: &Path, file_name: &str) -> Result<(), String> {
    let src = safe_note_path(dir, file_name, true)?;
    let dst = safe_note_path(dir, file_name, false)?;
    fs::rename(&src, &dst).map_err(|e| format!("恢复备忘录失败: {e}"))
}

pub(crate) fn delete_note_file(dir: &Path, file_name: &str, archived: bool) -> Result<(), String> {
    let path = safe_note_path(dir, file_name, archived)?;
    fs::remove_file(&path).map_err(|e| format!("删除备忘录失败: {e}"))
}

pub(crate) fn open_or_create_daily_note(dir: &Path, default_tag: &str) -> Result<NoteDoc, String> {
    fs::create_dir_all(dir).map_err(|e| format!("创建备忘录目录失败: {e}"))?;
    let ymd = local_today_ymd();
    let file_name = format!("{ymd}.md");
    let path = safe_note_path(dir, &file_name, false)?;
    if path.is_file()
        && let Some(doc) = read_single_note(&path, false)
    {
        return Ok(doc);
    }
    let now = unix_now_secs();
    let mut tags = vec!["#daily".to_string()];
    if !default_tag.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(default_tag)) {
        tags.push(default_tag.to_string());
    }
    let meta = NoteMeta {
        pinned: false,
        tags,
        created_secs: now,
        updated_secs: now,
        source: "daily".into(),
    };
    let body = format!("# {ymd} 每日速记\n\n- [ ] 今日重点\n\n");
    fs::write(&path, serialize_note_markdown(&meta, &body))
        .map_err(|e| format!("创建每日速记失败: {e}"))?;
    read_single_note(&path, false).ok_or_else(|| "读取每日速记失败".into())
}
