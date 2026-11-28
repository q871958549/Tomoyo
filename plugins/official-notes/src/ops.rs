use crate::frontmatter::{local_today_ymd, parse_frontmatter, serialize_note_markdown, toggle_todo_in_body, unix_now_secs, NoteMeta};
use crate::store::{compose_body_with_title, read_single_note, safe_note_path, NoteDoc};
use std::fs;
use std::path::Path;

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
    let _ = compose_body_with_title("", "");
    read_single_note(&path, false).ok_or_else(|| "读取每日速记失败".into())
}
