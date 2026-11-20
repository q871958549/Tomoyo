//! Relevance & path-aware ranking for Everything search candidates.

use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tomoyo_core::{score_match, IndexedText};
use tomoyo_everything::EverythingEntry;
use tomoyo_protocol::SearchFilter;

pub(crate) fn build_prefix_query(
    search_text: &str,
    filter: SearchFilter,
    extra_filter: &str,
    expanded: bool,
) -> String {
    let mut parts = vec![format!("startwith:{search_text}")];
    if let Some(expr) = filter.everything_expr() {
        parts.push(expr.to_string());
    }
    if expanded && !extra_filter.trim().is_empty() {
        parts.push(extra_filter.trim().to_string());
    }
    parts.join(" ")
}

pub(crate) fn merge_unique_entries(base: &mut Vec<EverythingEntry>, extra: Vec<EverythingEntry>) {
    let mut seen: HashSet<String> = base
        .iter()
        .map(|e| e.full_path.to_ascii_lowercase())
        .collect();
    for entry in extra {
        if seen.insert(entry.full_path.to_ascii_lowercase()) {
            base.push(entry);
        }
    }
}

/// Computes a balanced relevance score that surfaces exact/prefix matches and user
/// folders (Desktop, DeskBox, Documents, Downloads) above internal dot-caches.
pub(crate) fn compute_ev_score(
    search_text: &str,
    entry: &EverythingEntry,
    expanded: bool,
    idx: usize,
) -> i64 {
    let path_adj = path_relevance_adjustment(&entry.full_path);
    if search_text.is_empty() {
        let base = if expanded { 8_500 } else { 4_200 };
        return base + path_adj - (idx as i64 * 25);
    }

    let name = &entry.name;
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    let q_low = search_text.trim().to_lowercase();
    let name_low = name.to_lowercase();
    let stem_low = stem.to_lowercase();

    let name_score = if name_low == q_low || stem_low == q_low {
        7_600
    } else if name_low.starts_with(&q_low) || stem_low.starts_with(&q_low) {
        6_500
    } else {
        let indexed = IndexedText::new(name);
        match score_match(search_text, &indexed) {
            Some(s) if s >= 8_500 => 7_200,
            Some(s) if s >= 6_200 => 6_100,
            Some(s) if s >= 4_400 => 5_100,
            Some(_) => 4_300,
            None => 3_900,
        }
    };

    let ext_lower = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let kind_boost = if matches!(ext_lower.as_str(), "exe" | "lnk") {
        300
    } else if entry.is_folder {
        220
    } else if matches!(
        ext_lower.as_str(),
        "pdf" | "xlsx" | "xls" | "docx" | "doc" | "pptx" | "ppt" | "png" | "jpg" | "zip"
    ) {
        120
    } else {
        0
    };

    let recency = recency_bonus(entry.date_modified_filetime);
    let expanded_bias = if expanded { 600 } else { 0 };
    name_score + path_adj + kind_boost + recency + expanded_bias - (idx as i64 * 12)
}

pub(crate) fn path_relevance_adjustment(full_path: &str) -> i64 {
    let low = full_path.to_ascii_lowercase().replace('/', "\\");
    let has_dot_segment = low
        .split('\\')
        .any(|seg| seg.starts_with('.') && seg.len() > 1);
    let is_noisy_internal = has_dot_segment
        || low.contains("\\appdata\\local\\temp\\")
        || low.contains("\\appdata\\local\\microsoft\\")
        || low.contains("\\node_modules\\")
        || low.contains("\\__pycache__\\")
        || low.contains("\\target\\debug\\")
        || low.contains("\\target\\release\\")
        || low.contains("\\$recycle.bin\\");
    if is_noisy_internal {
        return -2_600;
    }
    let is_app_runtime_dir = low.starts_with("c:\\windows\\")
        || low.starts_with("c:\\program files\\")
        || low.starts_with("c:\\program files (x86)\\")
        || low.starts_with("c:\\programdata\\")
        || low.contains("\\appdata\\")
        || low.contains("\\locale\\")
        || low.contains("\\locales\\")
        || low.contains("\\intl\\");
    if is_app_runtime_dir {
        return -1_100;
    }
    if low.contains("\\deskbox\\") || low.contains("\\desktop\\") {
        return 1_400;
    }
    if low.contains("\\documents\\") || low.contains("\\downloads\\") {
        return 550;
    }
    0
}

fn recency_bonus(filetime: Option<u64>) -> i64 {
    const WINDOWS_TO_UNIX_TICKS: u64 = 116_444_736_000_000_000;
    let Some(ft) = filetime.filter(|&v| v > WINDOWS_TO_UNIX_TICKS && v != u64::MAX) else {
        return 0;
    };
    let file_secs = (ft - WINDOWS_TO_UNIX_TICKS) / 10_000_000;
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age_days = now_secs.saturating_sub(file_secs) / 86_400;
    match age_days {
        0..=1 => 240,
        2..=7 => 160,
        8..=30 => 80,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(name: &str, path: &str, is_folder: bool) -> EverythingEntry {
        EverythingEntry {
            name: name.into(),
            path: path.into(),
            full_path: format!("{path}\\{name}"),
            is_folder,
            is_drive: false,
            size: None,
            date_modified_filetime: None,
            attributes: None,
        }
    }

    #[test]
    fn deskbox_exact_folder_outranks_internal_dot_cache_and_migrations() {
        let deskbox = mk("1118", r"C:\Users\Administrator\DeskBox\其他", true);
        let hidden = mk(
            "1118",
            r"C:\Users\Administrator\.gemini\antigravity\brain\abc\.system_generated\steps",
            true,
        );
        let migration = mk(
            "20261118104755_Add_Badges.cs",
            r"E:\WorkSpace\App\Migrations",
            false,
        );
        let s_desk = compute_ev_score("1118", &deskbox, false, 6);
        let s_hid = compute_ev_score("1118", &hidden, false, 0);
        let s_mig = compute_ev_score("1118", &migration, false, 0);
        assert!(s_desk > s_mig + 2000, "DeskBox exact folder ({s_desk}) must outrank migration ({s_mig})");
        assert!(s_desk > s_hid + 3000, "DeskBox exact folder ({s_desk}) must outrank hidden cache ({s_hid})");
    }

    #[test]
    fn live_everything_provider_if_running() {
        use crate::EverythingProvider;
        use std::sync::{Arc, RwLock};
        use tomoyo_core::{Provider, TomoyoConfig};
        use tomoyo_protocol::{QueryContext, SearchFilter};

        if tomoyo_everything::detect_running().is_none() {
            return;
        }
        let cfg = Arc::new(RwLock::new(TomoyoConfig::default()));
        let prov = EverythingProvider::new(cfg);

        for (label, raw, filter) in [
            ("global_1118", "1118", SearchFilter::All),
            ("global_as", "as", SearchFilter::All),
            ("filter_pdf", "", SearchFilter::Pdf),
        ] {
            let ctx = QueryContext::new(raw, 1, 10).with_filter(filter);
            let items = prov.query(&ctx);
            println!("=== {label} ({} hits) ===", items.len());
            for (i, it) in items.iter().take(6).enumerate() {
                println!("  #{} [{}] {} :: {}", i + 1, it.score, it.title, it.subtitle);
            }
            assert!(!items.is_empty(), "{label} should return live Everything hits");
        }
    }
}
