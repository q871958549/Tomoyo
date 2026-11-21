//! Query-aware ranking, cross-source canonical path deduplication, and zero-result fallbacks.

use crate::{FrecencyStore, Lang, TomoyoConfig};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tomoyo_protocol::{ActionKind, ActionSpec, IconSpec, ResultCategory, ResultItem};

/// Applies query-aware frecency bonuses, sorts by descending score, and deduplicates
/// both by `item.id` and by canonical launch path (preventing Apps + Everything duplicates).
pub(crate) fn rank_and_dedup(
    frecency: &Arc<RwLock<FrecencyStore>>,
    raw_query: &str,
    mut items: Vec<ResultItem>,
    limit: usize,
) -> Vec<ResultItem> {
    if let Ok(store) = frecency.read() {
        for item in &mut items {
            item.score = item
                .score
                .saturating_add(store.bonus_for_query(raw_query, &item.id));
        }
    }

    items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));

    let mut seen_ids: HashSet<String> = HashSet::with_capacity(items.len().min(limit));
    let mut seen_paths: HashSet<String> = HashSet::with_capacity(items.len().min(limit));
    let mut deduped = Vec::with_capacity(limit.min(items.len()));

    for item in items {
        if !seen_ids.insert(item.id.clone()) {
            continue;
        }
        if let Some(canon_path) = canonical_launch_path(&item)
            && !seen_paths.insert(canon_path) {
                continue;
            }
        deduped.push(item);
        if deduped.len() >= limit {
            break;
        }
    }
    deduped
}

fn canonical_launch_path(item: &ResultItem) -> Option<String> {
    let first = item.actions.first()?;
    match &first.kind {
        ActionKind::LaunchPath { path, args, .. } if args.is_none() => {
            let norm = path
                .trim()
                .replace('/', "\\")
                .trim_end_matches('\\')
                .to_ascii_lowercase();
            // Only deduplicate concrete filesystem paths (ignore shell/UWP or bare commands)
            if norm.len() >= 4 && (norm.as_bytes()[1] == b':' || norm.starts_with("\\\\")) {
                Some(norm)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Builds helpful fallback actions when a global search yields zero local matches,
/// so the user can pivot immediately to web search or dedicated Everything search.
pub(crate) fn build_zero_result_fallbacks(
    config: Option<&Arc<RwLock<TomoyoConfig>>>,
    query: &str,
) -> Vec<ResultItem> {
    let term = query.trim();
    if term.chars().count() < 2 || term.starts_with(':') {
        return Vec::new();
    }

    let (lang, primary_engine) = config
        .and_then(|c| c.read().ok())
        .map(|cfg| (cfg.lang(), cfg.web_searches.first().cloned()))
        .unwrap_or((Lang::Zh, None));

    let (eng_name, eng_kw, url_tpl) = match primary_engine {
        Some(e) => (e.name, e.keyword, e.url),
        None => (
            "Bing".to_string(),
            "bing".to_string(),
            "https://www.bing.com/search?q={q}".to_string(),
        ),
    };
    let encoded = url_encode(term);
    let url = url_tpl.replace("{q}", &encoded);

    let mut out = vec![ResultItem {
        id: format!("fallback:web:{eng_kw}:{term}"),
        provider_id: "builtin.fallback".into(),
        title: match lang {
            Lang::Zh => format!("使用 {eng_name} 搜索 “{term}”"),
            Lang::En => format!("Search {eng_name} for \"{term}\""),
        },
        subtitle: url.clone(),
        category: ResultCategory::Web,
        score: 2_000,
        icon: IconSpec::Monogram(eng_kw.to_ascii_uppercase().chars().take(3).collect()),
        badge: Some(lang.pick("网页搜索", "WEB").into()),
        copy_text: Some(url.clone()),
        actions: vec![
            ActionSpec {
                id: "open_web".into(),
                label: lang.pick("在浏览器中搜索", "Open Web Search").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::OpenUrl { url: url.clone() },
            },
            ActionSpec {
                id: "copy_url".into(),
                label: lang.pick("复制搜索链接", "Copy Search URL").into(),
                shortcut: Some("Ctrl+C".into()),
                kind: ActionKind::CopyText {
                    text: url,
                    paste_immediately: false,
                },
            },
        ],
        preview: None,
    }];

    out.push(ResultItem {
        id: format!("fallback:ev:{term}"),
        provider_id: "builtin.fallback".into(),
        title: match lang {
            Lang::Zh => format!("用 Everything 专属模式全盘检索 “{term}”"),
            Lang::En => format!("Search all files in Everything for \"{term}\""),
        },
        subtitle: format!("f {term}"),
        category: ResultCategory::File,
        score: 1_900,
        icon: IconSpec::Monogram("EV".into()),
        badge: Some("f".into()),
        copy_text: Some(format!("f {term}")),
        actions: vec![ActionSpec {
            id: "fill_ev".into(),
            label: lang.pick("切换到 Everything 模式", "Switch to Everything Mode").into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::FillQuery {
                query: format!("f {term}"),
            },
        }],
        preview: None,
    });

    out
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            other => {
                use std::fmt::Write;
                let _ = write!(&mut out, "%{other:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_launch_item(id: &str, provider: &str, title: &str, path: &str, score: i64) -> ResultItem {
        ResultItem {
            id: id.into(),
            provider_id: provider.into(),
            title: title.into(),
            subtitle: path.into(),
            category: ResultCategory::App,
            score,
            icon: IconSpec::ShellPath(path.into()),
            badge: None,
            copy_text: Some(path.into()),
            actions: vec![ActionSpec {
                id: "launch".into(),
                label: "Open".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::LaunchPath {
                    path: path.into(),
                    args: None,
                    working_dir: None,
                },
            }],
            preview: None,
        }
    }

    #[test]
    fn deduplicates_identical_filesystem_paths_across_apps_and_everything() {
        let frecency = Arc::new(RwLock::new(FrecencyStore::default()));
        let app = mk_launch_item(
            "app:tomoyo",
            "builtin.apps",
            "Tomoyo",
            r"C:\Users\Administrator\DeskBox\应用\Tomoyo.lnk",
            9_000,
        );
        let ev = mk_launch_item(
            "ev:tomoyo",
            "builtin.everything",
            "Tomoyo.lnk",
            r"c:\users\administrator\deskbox\应用\tomoyo.lnk",
            7_500,
        );
        let ranked = rank_and_dedup(&frecency, "tomoyo", vec![ev, app], 10);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].id, "app:tomoyo");
    }
}
