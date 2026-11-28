//! Linux fast local file search backend (`fd` with `plocate`/`locate` fallback)
//! populating `EverythingEntry` records for unified ranking, previews, and File Action Flow.

use crate::everything_nl::split_tokens_preserving_quotes;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tomoyo_everything::{EverythingEntry, EverythingResponse};
use tomoyo_protocol::{CustomFilterTab, SearchFilter};

pub fn is_linux_file_search_available() -> bool {
    has_bin("fd") || has_bin("plocate") || has_bin("locate")
}

fn has_bin(name: &str) -> bool {
    std::env::var("PATH").is_ok_and(|v| std::env::split_paths(&v).any(|d| d.join(name).is_file()))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct LinuxFileHints {
    pub terms: String,
    pub exts: Vec<String>,
    pub folders_only: bool,
    pub changed_within: Option<&'static str>,
    pub yesterday_only: bool,
    pub min_size_bytes: Option<u64>,
    pub max_size_bytes: Option<u64>,
    pub fd_size_args: Vec<String>,
    pub path_filters: Vec<String>,
}

struct CombinedQuery {
    terms: String,
    exts: Vec<String>,
    folders_only: bool,
    files_only: bool,
    changed_within: Option<&'static str>,
    yesterday_only: bool,
    min_size_bytes: Option<u64>,
    max_size_bytes: Option<u64>,
    fd_size_args: Vec<String>,
    path_filters: Vec<String>,
}

impl CombinedQuery {
    fn from_inputs(search_text: &str, filter: SearchFilter, custom_tab: Option<&CustomFilterTab>) -> Self {
        let mut q = parse_inline_hints(search_text);
        let ce = custom_tab
            .map(|t| t.everything_expr.trim())
            .filter(|s| !s.is_empty())
            .map(parse_inline_hints)
            .unwrap_or_default();
        let terms = if q.terms.is_empty() { ce.terms } else { q.terms };
        let mut exts = filter_extensions(filter, custom_tab);
        exts.append(&mut q.exts);
        exts.extend(ce.exts);
        let mut path_filters = q.path_filters;
        for pf in ce.path_filters {
            if !path_filters.iter().any(|s| s.eq_ignore_ascii_case(&pf)) {
                path_filters.push(pf);
            }
        }
        q.fd_size_args.extend(ce.fd_size_args);
        Self {
            terms,
            exts,
            folders_only: matches!(filter, SearchFilter::Folder) || q.folders_only || ce.folders_only,
            files_only: filter.is_file_only(),
            changed_within: q.changed_within.or(ce.changed_within),
            yesterday_only: q.yesterday_only || ce.yesterday_only,
            min_size_bytes: match (q.min_size_bytes, ce.min_size_bytes) { (Some(a), Some(b)) => Some(a.max(b)), (a, b) => a.or(b) },
            max_size_bytes: match (q.max_size_bytes, ce.max_size_bytes) { (Some(a), Some(b)) => Some(a.min(b)), (a, b) => a.or(b) },
            fd_size_args: q.fd_size_args,
            path_filters,
        }
    }
}

pub(crate) fn should_prefer_plocate(cq: &CombinedQuery, has_plocate: bool, has_fd: bool) -> bool {
    if !has_plocate {
        return false;
    }
    if !has_fd {
        return true;
    }
    !cq.terms.is_empty()
        && cq.changed_within.is_none()
        && !cq.yesterday_only
        && cq.min_size_bytes.is_none()
        && cq.max_size_bytes.is_none()
        && cq.path_filters.is_empty()
}

pub(crate) fn query_linux_files(
    search_text: &str,
    filter: SearchFilter,
    custom_tab: Option<&CustomFilterTab>,
    max_results: usize,
) -> Result<EverythingResponse, &'static str> {
    let cap = max_results.clamp(1, 80);
    let cq = CombinedQuery::from_inputs(search_text, filter, custom_tab);
    let (has_fd, has_plocate, has_locate) = (has_bin("fd"), has_bin("plocate"), has_bin("locate"));
    let paths = if should_prefer_plocate(&cq, has_plocate, has_fd) {
        let fast = run_locate_search(&cq, filter != SearchFilter::All, cap);
        if fast.is_empty() && has_fd { run_fd_search(&cq, cap) } else { fast }
    } else if has_fd {
        run_fd_search(&cq, cap)
    } else if has_plocate || has_locate {
        run_locate_search(&cq, filter != SearchFilter::All, cap)
    } else {
        return Err("missing fd/plocate");
    };
    let entries: Vec<EverythingEntry> = paths.into_iter().map(path_to_entry).collect();
    let total = entries.len() as u32;
    Ok(EverythingResponse {
        total_folders: entries.iter().filter(|e| e.is_folder).count() as u32,
        total_files: entries.iter().filter(|e| !e.is_folder).count() as u32,
        total_items: total,
        entries,
    })
}

fn run_fd_search(cq: &CombinedQuery, cap: usize) -> Vec<PathBuf> {
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
    let mut search_root = home.clone();
    let mut unrooted = cq.path_filters.clone();
    if let (Some(h), 1) = (&home, cq.path_filters.len())
        && let Some(dir) = resolve_home_subdir(Path::new(h), &cq.path_filters[0])
    {
        search_root = Some(dir.to_string_lossy().into_owned());
        unrooted.clear();
    }
    let fd_cap = if unrooted.is_empty() { cap } else { (cap * 4).clamp(cap, 240) };

    let mut cmd = Command::new("fd");
    cmd.args(["--color", "never", "--max-results", &fd_cap.to_string(), "--max-depth", "7", "--ignore-case"]);
    if cq.folders_only { cmd.args(["--type", "d"]); } else if cq.files_only { cmd.args(["--type", "f"]); }
    if let Some(within) = cq.changed_within { cmd.args(["--changed-within", within]); }
    if cq.yesterday_only { cmd.args(["--changed-before", "1d"]); }
    for sz in &cq.fd_size_args { cmd.args(["--size", sz]); }
    for ext in &cq.exts { cmd.args(["-e", ext]); }
    if cq.terms.is_empty() { cmd.arg("."); } else { cmd.args(["--fixed-strings", &cq.terms]); }
    if let Some(root) = &search_root { cmd.arg(root); }

    let Ok(out) = cmd.output() else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| matches_path_filters(p, &unrooted))
        .take(cap)
        .collect()
}

fn run_locate_search(cq: &CombinedQuery, has_filter_tab: bool, cap: usize) -> Vec<PathBuf> {
    let has_constraints = has_filter_tab
        || !cq.exts.is_empty()
        || cq.changed_within.is_some()
        || cq.min_size_bytes.is_some()
        || cq.max_size_bytes.is_some()
        || !cq.path_filters.is_empty();
    let pattern = if !cq.terms.is_empty() {
        cq.terms.clone()
    } else if has_constraints {
        std::env::var("HOME").ok().filter(|h| !h.is_empty()).unwrap_or_default()
    } else {
        String::new()
    };
    if pattern.is_empty() {
        return Vec::new();
    }
    let bin = if has_bin("plocate") { "plocate" } else { "locate" };
    let Ok(out) = Command::new(bin).args(["-i", "-l", &(cap * 6).to_string(), &pattern]).output() else {
        return Vec::new();
    };
    let changed_secs = cq.changed_within.and_then(duration_secs_for_within);
    let now = SystemTime::now();
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| {
            if !matches_path_filters(p, &cq.path_filters) { return false; }
            if cq.folders_only { return p.is_dir(); }
            if cq.files_only && !p.is_file() { return false; }
            if !cq.exts.is_empty() {
                let ext = p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
                if !cq.exts.iter().any(|fe| fe == &ext) { return false; }
            }
            matches_metadata_filters(p, changed_secs, cq.yesterday_only, cq.min_size_bytes, cq.max_size_bytes, now)
        })
        .take(cap)
        .collect()
}

fn resolve_home_subdir(home: &Path, seg: &str) -> Option<PathBuf> {
    let trimmed = seg.trim().trim_matches('/');
    if trimmed.is_empty() || trimmed.contains('/') { return None; }
    let candidates: Vec<&str> = match trimmed.to_ascii_lowercase().as_str() {
        "downloads" | "下载" => vec![trimmed, "Downloads", "下载"],
        "desktop" | "桌面" => vec![trimmed, "Desktop", "桌面"],
        "documents" | "文档" => vec![trimmed, "Documents", "文档"],
        _ => vec![trimmed],
    };
    for c in &candidates {
        let p = home.join(c);
        if p.is_dir() { return Some(p); }
    }
    let rd = std::fs::read_dir(home).ok()?;
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir()
            && let Some(name) = p.file_name().and_then(|n| n.to_str())
            && candidates.iter().any(|c| name.eq_ignore_ascii_case(c))
        {
            return Some(p);
        }
    }
    None
}

fn matches_path_filters(p: &Path, filters: &[String]) -> bool {
    if filters.is_empty() { return true; }
    let low = p.to_string_lossy().to_ascii_lowercase();
    filters.iter().all(|f| low.contains(&f.to_ascii_lowercase()))
}

fn matches_metadata_filters(
    p: &Path,
    changed_within_secs: Option<u64>,
    yesterday_only: bool,
    min_bytes: Option<u64>,
    max_bytes: Option<u64>,
    now: SystemTime,
) -> bool {
    if changed_within_secs.is_none() && !yesterday_only && min_bytes.is_none() && max_bytes.is_none() {
        return true;
    }
    let Ok(meta) = std::fs::metadata(p) else { return false };
    if let Some(min_b) = min_bytes && (meta.is_dir() || meta.len() < min_b) { return false; }
    if let Some(max_b) = max_bytes && (meta.is_dir() || meta.len() > max_b) { return false; }
    if changed_within_secs.is_some() || yesterday_only {
        let Ok(mod_t) = meta.modified() else { return false };
        let age = now.duration_since(mod_t).unwrap_or(Duration::ZERO).as_secs();
        if let Some(limit) = changed_within_secs && age > limit { return false; }
        if yesterday_only && !(86_400..=172_800).contains(&age) { return false; }
    }
    true
}

fn duration_secs_for_within(within: &str) -> Option<u64> {
    match within {
        "1d" => Some(86_400),
        "2d" => Some(172_800),
        "7d" => Some(7 * 86_400),
        "30d" => Some(30 * 86_400),
        "365d" => Some(365 * 86_400),
        _ => None,
    }
}

fn parse_size_spec(spec: &str) -> Option<(bool, u64, String)> {
    let (is_min, rest) = if let Some(r) = spec.strip_prefix('>') {
        (true, r.trim_start_matches('='))
    } else {
        (false, spec.strip_prefix('<')?.trim_start_matches('='))
    };
    let split = rest.find(|c: char| !c.is_ascii_digit())?;
    let (num_s, unit_s) = rest.split_at(split);
    let num: u64 = num_s.parse().ok()?;
    let (mult, fd_arg) = match unit_s.to_ascii_lowercase().as_str() {
        "b" => (1u64, format!("{num}b")),
        "k" | "kb" => (1024u64, format!("{num}k")),
        "m" | "mb" => (1024u64 * 1024, format!("{num}m")),
        "g" | "gb" => (1024u64 * 1024 * 1024, format!("{num}g")),
        "t" | "tb" => (1024u64 * 1024 * 1024 * 1024, format!("{}g", num.saturating_mul(1024))),
        _ => return None,
    };
    Some((is_min, num.saturating_mul(mult), format!("{}{fd_arg}", if is_min { '+' } else { '-' })))
}

pub(crate) fn parse_inline_hints(raw: &str) -> LinuxFileHints {
    let mut h = LinuxFileHints::default();
    let mut terms = Vec::new();
    for tok in split_tokens_preserving_quotes(raw.trim()) {
        let low = tok.to_ascii_lowercase();
        if let Some(rest) = low.strip_prefix("ext:") {
            h.exts.extend(rest.split(';').map(|s| s.trim().trim_start_matches('.').to_string()).filter(|s| !s.is_empty()));
        } else if low == "folder:" {
            h.folders_only = true;
        } else if matches!(low.as_str(), "dm:today" | "dm:1d") { h.changed_within = Some("1d"); }
        else if low == "dm:yesterday" { h.changed_within = Some("2d"); h.yesterday_only = true; }
        else if matches!(low.as_str(), "dm:week" | "dm:thisweek" | "dm:last7days" | "dm:7d") { h.changed_within = Some("7d"); }
        else if matches!(low.as_str(), "dm:month" | "dm:thismonth" | "dm:30d") { h.changed_within = Some("30d"); }
        else if matches!(low.as_str(), "dm:year" | "dm:thisyear" | "dm:365d") { h.changed_within = Some("365d"); }
        else if let Some(rest) = low.strip_prefix("size:") {
            if let Some((is_min, bytes, fd_arg)) = parse_size_spec(rest) {
                if is_min { h.min_size_bytes = Some(h.min_size_bytes.map_or(bytes, |b| b.max(bytes))); }
                else { h.max_size_bytes = Some(h.max_size_bytes.map_or(bytes, |b| b.min(bytes))); }
                h.fd_size_args.push(fd_arg);
            }
        } else if let Some(rest) = tok.strip_prefix("path:").or_else(|| tok.strip_prefix("PATH:")) {
            let clean = rest.trim_matches('"').trim();
            if !clean.is_empty() { h.path_filters.push(clean.to_string()); }
        } else if !low.starts_with("dm:") {
            let clean = tok.trim_matches('"').trim();
            if !clean.is_empty() { terms.push(clean.to_string()); }
        }
    }
    h.terms = terms.join(" ");
    h
}

fn filter_extensions(filter: SearchFilter, custom: Option<&CustomFilterTab>) -> Vec<String> {
    if let Some(tab) = custom {
        return tab.extensions.iter().map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|e| !e.is_empty()).collect();
    }
    let list: &[&str] = match filter {
        SearchFilter::Pdf => &["pdf"],
        SearchFilter::Excel => &["xls", "xlsx", "xlsm", "csv"],
        SearchFilter::Word => &["doc", "docx", "rtf", "odt"],
        SearchFilter::Ppt => &["ppt", "pptx", "odp"],
        SearchFilter::Image => &["png", "jpg", "jpeg", "webp", "bmp", "gif", "svg", "ico"],
        SearchFilter::Archive => &["zip", "7z", "rar", "tar", "gz", "xz", "zst"],
        _ => &[],
    };
    list.iter().map(|s| (*s).to_string()).collect()
}

fn path_to_entry(p: PathBuf) -> EverythingEntry {
    let meta = std::fs::metadata(&p).ok();
    let is_folder = meta.as_ref().is_some_and(|m| m.is_dir());
    let size = (!is_folder).then(|| meta.as_ref().map(|m| m.len())).flatten();
    let date_modified_filetime = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|st| st.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() * 10_000_000 + 116_444_736_000_000_000);
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string_lossy().into_owned());
    let parent = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into());
    let full_path = Path::new(&p).to_string_lossy().into_owned();
    EverythingEntry { name, path: parent, full_path, is_folder, is_drive: false, size, date_modified_filetime, attributes: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nl_size_date_path_and_quoted_terms_for_linux() {
        let h = parse_inline_hints("dm:today size:>10mb ext:pdf \"2026 报告\"");
        assert_eq!(h.changed_within, Some("1d"));
        assert!(!h.yesterday_only);
        assert_eq!(h.min_size_bytes, Some(10 * 1024 * 1024));
        assert_eq!(h.fd_size_args, vec!["+10m"]);
        assert_eq!(h.exts, vec!["pdf"]);
        assert_eq!(h.terms, "2026 报告");

        let dl = parse_inline_hints("path:Downloads size:<500kb ext:zip;tar dm:last7days");
        assert_eq!(dl.path_filters, vec!["Downloads"]);
        assert_eq!(dl.max_size_bytes, Some(500 * 1024));
        assert_eq!(dl.fd_size_args, vec!["-500k"]);
        assert_eq!(dl.exts, vec!["zip", "tar"]);
        assert_eq!(dl.changed_within, Some("7d"));

        let y = parse_inline_hints("dm:yesterday folder:");
        assert_eq!(y.changed_within, Some("2d"));
        assert!(y.yesterday_only);
        assert!(y.folders_only);
        assert_eq!(parse_inline_hints("dm:thismonth").changed_within, Some("30d"));
        assert_eq!(parse_inline_hints("dm:thisyear").changed_within, Some("365d"));
    }

    #[test]
    fn prefers_plocate_for_plain_queries_and_fd_for_rich_filters() {
        let plain = CombinedQuery::from_inputs("Cargo.toml", SearchFilter::All, None);
        assert!(should_prefer_plocate(&plain, true, true));
        let dated = CombinedQuery::from_inputs("dm:today report", SearchFilter::Pdf, None);
        assert!(!should_prefer_plocate(&dated, true, true));
        let sized = CombinedQuery::from_inputs("size:>5mb video", SearchFilter::All, None);
        assert!(!should_prefer_plocate(&sized, true, true));
        let scoped = CombinedQuery::from_inputs("path:Downloads iso", SearchFilter::All, None);
        assert!(!should_prefer_plocate(&scoped, true, true));
        assert!(should_prefer_plocate(&scoped, true, false));
    }
}
