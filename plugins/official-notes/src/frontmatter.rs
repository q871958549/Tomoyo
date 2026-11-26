//! Lightweight YAML frontmatter, `#tag`, checkbox `- [ ]`, and `[[WikiLink]]` parser
//! for human-readable local-first Markdown notes (zero external dependencies).

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NoteMeta {
    pub pinned: bool,
    pub tags: Vec<String>,
    pub created_secs: u64,
    pub updated_secs: u64,
    pub source: String,
}

pub(crate) fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Pure-Rust UTC+8 calendar date (`YYYY-MM-DD`) via Howard Hinnant's civil-from-days algorithm.
pub(crate) fn local_today_ymd() -> String {
    let days = (unix_now_secs() as i64 + 8 * 3600).div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    format!("{year:04}-{m:02}-{d:02}")
}

pub(crate) fn normalize_tag(raw: &str) -> Option<String> {
    let clean = raw
        .trim()
        .trim_matches(|c: char| matches!(c, ',' | '.' | ';' | ':' | '"' | '\'' | '，' | '。'));
    let body = clean.trim_start_matches('#').trim();
    if body.is_empty() || body.chars().count() > 28 {
        return None;
    }
    Some(format!("#{body}"))
}

fn push_unique_tag(dst: &mut Vec<String>, candidate: &str) {
    if let Some(norm) = normalize_tag(candidate)
        && !dst.iter().any(|t| t.eq_ignore_ascii_case(&norm))
    {
        dst.push(norm);
    }
}

/// Parses optional leading YAML frontmatter (`---\n...\n---`), preserving 100% compatibility
/// with plain `.md` / `.txt` files that have no frontmatter header.
pub(crate) fn parse_frontmatter(raw: &str) -> (NoteMeta, String) {
    let normalized = raw.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return (NoteMeta::default(), normalized);
    };
    let mut lines = rest.lines().peekable();
    let mut header_lines = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        header_lines.push(line);
    }
    if !closed {
        return (NoteMeta::default(), normalized);
    }

    let mut meta = NoteMeta::default();
    let mut in_tags_list = false;
    for line in header_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if in_tags_list && let Some(item) = trimmed.strip_prefix("- ") {
            push_unique_tag(&mut meta.tags, item);
            continue;
        }
        in_tags_list = false;
        let Some((key, val)) = trimmed.split_once(':') else { continue };
        let key = key.trim();
        let val = val.trim();
        match key {
            "pinned" => meta.pinned = val.eq_ignore_ascii_case("true"),
            "created" => meta.created_secs = val.parse().unwrap_or(0),
            "updated" => meta.updated_secs = val.parse().unwrap_or(0),
            "source" => meta.source = val.trim_matches(|c| c == '"' || c == '\'').to_string(),
            "tags" => {
                if val.is_empty() {
                    in_tags_list = true;
                } else {
                    let inner = val.trim_start_matches('[').trim_end_matches(']');
                    for part in inner.split(',') {
                        push_unique_tag(&mut meta.tags, part);
                    }
                }
            }
            _ => {}
        }
    }

    let remainder: Vec<&str> = lines.collect();
    let body = remainder.join("\n").trim_start_matches('\n').to_string();
    (meta, body)
}

/// Serializes metadata and Markdown body cleanly (omits frontmatter only if all defaults & empty? No,
/// keeps compact YAML header for edited/managed notes while leaving existing files readable everywhere).
pub(crate) fn serialize_note_markdown(meta: &NoteMeta, body: &str) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("pinned: {}\n", meta.pinned));
    if meta.tags.is_empty() {
        out.push_str("tags: []\n");
    } else {
        let items: Vec<String> = meta
            .tags
            .iter()
            .map(|t| t.trim_start_matches('#').to_string())
            .collect();
        out.push_str(&format!("tags: [{}]\n", items.join(", ")));
    }
    if meta.created_secs > 0 {
        out.push_str(&format!("created: {}\n", meta.created_secs));
    }
    if meta.updated_secs > 0 {
        out.push_str(&format!("updated: {}\n", meta.updated_secs));
    }
    if !meta.source.trim().is_empty() {
        out.push_str(&format!("source: {}\n", meta.source.trim()));
    }
    out.push_str("---\n\n");
    out.push_str(body.trim_end());
    out.push('\n');
    out
}

/// Merges frontmatter tags and inline `#tag` tokens (skipping markdown headings).
pub(crate) fn extract_tags(meta_tags: &[String], body: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for t in meta_tags {
        push_unique_tag(&mut tags, t);
    }
    for tok in body.split_whitespace() {
        if let Some(rest) = tok.strip_prefix('#')
            && !rest.is_empty()
            && !rest.starts_with('#')
        {
            push_unique_tag(&mut tags, rest);
        }
    }
    tags
}

pub(crate) fn count_todos(body: &str) -> (usize, usize) {
    let mut open = 0usize;
    let mut done = 0usize;
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("- [ ] ") || t.starts_with("* [ ] ") || t == "- [ ]" || t == "* [ ]" {
            open += 1;
        } else if t.starts_with("- [x] ")
            || t.starts_with("- [X] ")
            || t.starts_with("* [x] ")
            || t.starts_with("* [X] ")
            || t.eq_ignore_ascii_case("- [x]")
            || t.eq_ignore_ascii_case("* [x]")
        {
            done += 1;
        }
    }
    (open, done)
}

pub(crate) fn toggle_todo_in_body(body: &str, line_idx: usize) -> Option<String> {
    let mut lines: Vec<String> = body.lines().map(String::from).collect();
    let target = lines.get_mut(line_idx)?;
    let indent_len = target.len() - target.trim_start().len();
    let (indent, rest) = target.split_at(indent_len);
    let updated = if let Some(tail) = rest.strip_prefix("- [ ]") {
        format!("{indent}- [x]{tail}")
    } else if let Some(tail) = rest.strip_prefix("* [ ]") {
        format!("{indent}* [x]{tail}")
    } else if let Some(tail) = rest.strip_prefix("- [x]").or_else(|| rest.strip_prefix("- [X]")) {
        format!("{indent}- [ ]{tail}")
    } else {
        let tail = rest.strip_prefix("* [x]").or_else(|| rest.strip_prefix("* [X]"))?;
        format!("{indent}* [ ]{tail}")
    };
    *target = updated;
    let mut joined = lines.join("\n");
    if body.ends_with('\n') {
        joined.push('\n');
    }
    Some(joined)
}

pub(crate) fn extract_wikilinks(body: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("]]") else { break };
        let raw_inner = &after_start[..end];
        let target = raw_inner.split('|').next().unwrap_or("").trim();
        if !target.is_empty()
            && !target.contains('\n')
            && !links.iter().any(|l: &String| l.eq_ignore_ascii_case(target))
        {
            links.push(target.to_string());
            if links.len() >= 32 {
                break;
            }
        }
        rest = &after_start[end + 2..];
    }
    links
}
