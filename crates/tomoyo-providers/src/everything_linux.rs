//! Linux fast local file search backend (`fd` with `plocate`/`locate` fallback)
//! populating `EverythingEntry` records for unified ranking, previews, and File Action Flow.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;
use tomoyo_everything::{EverythingEntry, EverythingResponse};
use tomoyo_protocol::{CustomFilterTab, SearchFilter};

pub fn is_linux_file_search_available() -> bool {
    has_bin("fd") || has_bin("plocate") || has_bin("locate")
}

fn has_bin(name: &str) -> bool {
    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|d| d.join(name).is_file())
}

pub(crate) fn query_linux_files(
    search_text: &str,
    filter: SearchFilter,
    custom_tab: Option<&CustomFilterTab>,
    max_results: usize,
) -> Result<EverythingResponse, &'static str> {
    let cap = max_results.clamp(1, 80);
    let paths = if has_bin("fd") {
        run_fd_search(search_text, filter, custom_tab, cap)
    } else if has_bin("plocate") || has_bin("locate") {
        run_locate_search(search_text, filter, custom_tab, cap)
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

fn run_fd_search(
    search_text: &str,
    filter: SearchFilter,
    custom_tab: Option<&CustomFilterTab>,
    cap: usize,
) -> Vec<PathBuf> {
    let mut cmd = Command::new("fd");
    cmd.args([
        "--color",
        "never",
        "--max-results",
        &cap.to_string(),
        "--max-depth",
        "7",
        "--ignore-case",
    ]);

    let (cleaned_q, q_exts, q_folders, q_changed) = parse_inline_hints(search_text);
    let custom_expr = custom_tab.map(|t| t.everything_expr.trim()).filter(|s| !s.is_empty());
    let (ce_q, ce_exts, ce_folders, ce_changed) = custom_expr.map(parse_inline_hints).unwrap_or_default();
    if matches!(filter, SearchFilter::Folder) || q_folders || ce_folders {
        cmd.args(["--type", "d"]);
    } else if filter.is_file_only() {
        cmd.args(["--type", "f"]);
    }

    if let Some(within) = q_changed.or(ce_changed) {
        cmd.args(["--changed-within", within]);
    }
    for ext in filter_extensions(filter, custom_tab).into_iter().chain(q_exts).chain(ce_exts) {
        cmd.args(["-e", &ext]);
    }

    let final_q = match (cleaned_q.is_empty(), ce_q.is_empty()) {
        (false, _) => cleaned_q,
        (true, false) => ce_q,
        (true, true) => String::new(),
    };
    if final_q.is_empty() {
        cmd.arg(".");
    } else {
        cmd.args(["--fixed-strings", &final_q]);
    }

    if let Ok(home) = std::env::var("HOME") {
        cmd.arg(&home);
    }

    let Ok(out) = cmd.output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(cap)
        .map(PathBuf::from)
        .collect()
}

fn run_locate_search(
    search_text: &str,
    filter: SearchFilter,
    custom_tab: Option<&CustomFilterTab>,
    cap: usize,
) -> Vec<PathBuf> {
    let (cleaned, inline_exts, want_folders, _) = parse_inline_hints(search_text);
    let ce_hints = custom_tab
        .map(|t| t.everything_expr.trim())
        .filter(|s| !s.is_empty())
        .map(parse_inline_hints)
        .unwrap_or_default();
    let eff_q = if cleaned.is_empty() { ce_hints.0 } else { cleaned };
    if eff_q.is_empty() {
        return Vec::new();
    }
    let bin = if has_bin("plocate") { "plocate" } else { "locate" };
    let Ok(out) = Command::new(bin)
        .args(["-i", "-l", &(cap * 3).to_string(), &eff_q])
        .output()
    else {
        return Vec::new();
    };
    let filter_exts: Vec<String> = filter_extensions(filter, custom_tab)
        .into_iter()
        .chain(inline_exts)
        .chain(ce_hints.1)
        .collect();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| {
            if want_folders || ce_hints.2 || filter == SearchFilter::Folder {
                return p.is_dir();
            }
            if !filter_exts.is_empty() {
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                return filter_exts.iter().any(|fe| fe == &ext);
            }
            true
        })
        .take(cap)
        .collect()
}

fn parse_inline_hints(raw: &str) -> (String, Vec<String>, bool, Option<&'static str>) {
    let mut terms = Vec::new();
    let mut exts = Vec::new();
    let mut folders_only = false;
    let mut changed_within = None;
    for tok in raw.split_whitespace() {
        let low = tok.to_ascii_lowercase();
        if let Some(rest) = low.strip_prefix("ext:") {
            for e in rest.split(';').map(str::trim).filter(|s| !s.is_empty()) {
                exts.push(e.trim_start_matches('.').to_string());
            }
        } else if low == "folder:" {
            folders_only = true;
        } else if matches!(low.as_str(), "dm:today" | "dm:1d") {
            changed_within = Some("1d");
        } else if matches!(low.as_str(), "dm:week" | "dm:thisweek" | "dm:7d") {
            changed_within = Some("7d");
        } else if !low.starts_with("dm:") && !low.starts_with("size:") {
            terms.push(tok);
        }
    }
    (terms.join(" "), exts, folders_only, changed_within)
}

fn filter_extensions(filter: SearchFilter, custom: Option<&CustomFilterTab>) -> Vec<String> {
    if let Some(tab) = custom {
        return tab
            .extensions
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
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
    let date_modified_filetime = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|st| st.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() * 10_000_000 + 116_444_736_000_000_000);
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned());
    let parent = p
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".into());
    let full_path = Path::new(&p).to_string_lossy().into_owned();
    EverythingEntry {
        name,
        path: parent,
        full_path,
        is_folder,
        is_drive: false,
        size,
        date_modified_filetime,
        attributes: None,
    }
}
