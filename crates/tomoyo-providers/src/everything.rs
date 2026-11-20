use crate::everything_history::recent_everything_queries;
use crate::everything_rank::{build_prefix_query, compute_ev_score, merge_unique_entries};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tomoyo_core::{Lang, Provider, TomoyoConfig};
use tomoyo_everything::{EverythingQueryOptions, SortOrder};
use tomoyo_protocol::{
    extension_short_tag, ActionKind, ActionSpec, IconSpec, PreviewSpec, QueryContext,
    ResultCategory, ResultItem, SearchFilter,
};

/// First-class native Everything 1.4 & 1.5a file search provider (zero external DLLs)
/// with live file preview support, group filter pushdown, i18n, and customizable filters.
pub struct EverythingProvider {
    config: Arc<RwLock<TomoyoConfig>>,
}

impl EverythingProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        Self { config }
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }
}

impl Provider for EverythingProvider {
    fn id(&self) -> &'static str {
        "builtin.everything"
    }

    fn name(&self) -> &'static str {
        "Everything File Search"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["f", "ev"]
    }

    fn supports_global(&self) -> bool {
        true
    }

    fn is_slow_ipc(&self) -> bool {
        true
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let lang = self.lang();
        let dedicated = ctx.keyword.is_some();
        let filtered_file_mode = ctx.filter.is_file_only();
        let search_text = if dedicated {
            ctx.args.trim()
        } else {
            ctx.trimmed.as_str()
        };

        if search_text.is_empty() && !filtered_file_mode {
            if dedicated {
                return hint_items(lang, &self.config);
            }
            return Vec::new();
        }

        if !dedicated && !filtered_file_mode && (search_text.chars().count() < 2 || search_text.starts_with("http")) {
            return Vec::new();
        }
        if search_text.starts_with("http") {
            return Vec::new();
        }

        let (cfg_global, cfg_ded, extra_filter) = self
            .config
            .read()
            .map(|c| {
                (
                    c.everything.max_global_results,
                    c.everything.max_dedicated_results,
                    c.everything.extra_filter.clone(),
                )
            })
            .unwrap_or((6, 30, String::new()));

        let mut parts: Vec<String> = Vec::with_capacity(3);
        if !search_text.is_empty() {
            parts.push(search_text.to_string());
        }
        if let Some(filter_expr) = ctx.filter.everything_expr() {
            parts.push(filter_expr.to_string());
        }
        if (dedicated || filtered_file_mode) && !extra_filter.trim().is_empty() {
            parts.push(extra_filter.trim().to_string());
        }
        let effective_query = parts.join(" ");

        let expanded = dedicated || filtered_file_mode;
        if !expanded && cfg_global == 0 {
            return Vec::new();
        }
        let target_cap = if expanded {
            ctx.limit.min(cfg_ded).clamp(1, 50)
        } else {
            ctx.limit.min(cfg_global).clamp(1, 10)
        };
        let fetch_limit = if search_text.is_empty() {
            target_cap as u32
        } else {
            (target_cap * 6).clamp(48, 80) as u32
        };

        let opts = EverythingQueryOptions {
            max_results: fetch_limit,
            sort: if search_text.is_empty() {
                SortOrder::DateModifiedDesc
            } else {
                SortOrder::NameAsc
            },
            timeout_ms: if expanded { 800 } else { 450 },
            ..Default::default()
        };

        match tomoyo_everything::query(&effective_query, &opts) {
            Ok(mut resp) => {
                // When many files match a simple token, also pull instant prefix hits (`startwith:`)
                // so exact/prefix user folders & files (e.g. `1118`) are never buried behind `000_...`.
                let is_plain_token = !search_text.is_empty()
                    && !search_text.contains(char::is_whitespace)
                    && !search_text.contains(':');
                if is_plain_token && resp.total_items > fetch_limit {
                    let prefix_q = build_prefix_query(search_text, ctx.filter, &extra_filter, expanded);
                    let prefix_opts = EverythingQueryOptions {
                        max_results: 32,
                        sort: SortOrder::NameAsc,
                        timeout_ms: 250,
                        ..Default::default()
                    };
                    if let Ok(pref_resp) = tomoyo_everything::query(&prefix_q, &prefix_opts) {
                        merge_unique_entries(&mut resp.entries, pref_resp.entries);
                    }
                }

                if resp.entries.is_empty() && expanded {
                    return vec![empty_result_item(self.id(), search_text, ctx.filter, lang)];
                }

                let mut items: Vec<ResultItem> = resp
                    .entries
                    .into_iter()
                    .enumerate()
                    .map(|(idx, entry)| {
                        let score = compute_ev_score(search_text, &entry, expanded, idx);
                        build_entry_item(self.id(), entry, score, lang)
                    })
                    .collect();

                items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
                items.truncate(target_cap);
                items
            }
            Err(tomoyo_everything::EverythingError::NotRunning) => {
                if expanded {
                    vec![offline_item(self.id())]
                } else {
                    Vec::new()
                }
            }
            Err(e) => {
                tracing::warn!("Everything query `{effective_query}` failed: {e}");
                Vec::new()
            }
        }
    }
}

fn build_entry_item(
    provider_id: &str,
    entry: tomoyo_everything::EverythingEntry,
    score: i64,
    lang: Lang,
) -> ResultItem {
    let parent_dir = if entry.is_folder {
        entry.full_path.clone()
    } else {
        Path::new(&entry.full_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.path.clone())
    };

    let mod_str = entry
        .modified_local()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string());
    let subtitle = match &mod_str {
        Some(m) => format!("{}  ·  {m}", entry.path),
        None => entry.path.clone(),
    };

    let badge = if entry.is_folder {
        Some(lang.pick("文件夹", "Folder").into())
    } else {
        let ext = Path::new(&entry.name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let tag = extension_short_tag(&ext);
        match entry.formatted_size() {
            Some(sz) if tag != "FILE" => Some(format!("{tag} · {sz}")),
            Some(sz) => Some(sz),
            None if tag != "FILE" => Some(tag.into()),
            None => None,
        }
    };

    ResultItem {
        id: format!("ev:{}", entry.full_path),
        provider_id: provider_id.into(),
        title: entry.name.clone(),
        subtitle,
        category: if entry.is_folder {
            ResultCategory::Folder
        } else {
            ResultCategory::File
        },
        score,
        icon: IconSpec::ShellPath(entry.full_path.clone()),
        badge,
        copy_text: Some(entry.full_path.clone()),
        actions: vec![
            ActionSpec {
                id: "open".into(),
                label: if entry.is_folder {
                    lang.pick("打开文件夹", "Open Folder").into()
                } else {
                    lang.pick("打开文件", "Open File").into()
                },
                shortcut: Some("Enter".into()),
                kind: ActionKind::LaunchPath {
                    path: entry.full_path.clone(),
                    args: None,
                    working_dir: Some(parent_dir.clone()),
                },
            },
            ActionSpec {
                id: "reveal".into(),
                label: lang.pick("在资源管理器中定位", "Reveal in Explorer").into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::RevealInExplorer {
                    path: entry.full_path.clone(),
                },
            },
            ActionSpec {
                id: "copy_path".into(),
                label: lang.pick("复制完整路径", "Copy Full Path").into(),
                shortcut: Some("Ctrl+C".into()),
                kind: ActionKind::CopyText {
                    text: entry.full_path.clone(),
                    paste_immediately: false,
                },
            },
            ActionSpec {
                id: "terminal".into(),
                label: lang.pick("在此处打开终端", "Open Terminal Here").into(),
                shortcut: None,
                kind: ActionKind::OpenTerminalAt { dir: parent_dir },
            },
            ActionSpec {
                id: "copy_name".into(),
                label: lang.pick("复制文件名", "Copy File Name").into(),
                shortcut: None,
                kind: ActionKind::CopyText {
                    text: entry.name,
                    paste_immediately: false,
                },
            },
        ],
        preview: Some(PreviewSpec::FilePreview {
            path: entry.full_path,
        }),
    }
}

fn empty_result_item(
    provider_id: &str,
    search_text: &str,
    filter: SearchFilter,
    lang: Lang,
) -> ResultItem {
    let scope_label = filter.label(lang == Lang::Zh);
    ResultItem {
        id: "everything.empty".into(),
        provider_id: provider_id.into(),
        title: if search_text.is_empty() {
            format!("No matching {scope_label} items")
        } else {
            format!("[{scope_label}] 未找到匹配 `{search_text}` 的文件")
        },
        subtitle: "支持 Everything 语法：ext:rs · dm:today · size:>10mb · folder:".into(),
        category: ResultCategory::File,
        score: 100,
        icon: IconSpec::Monogram("EV".into()),
        badge: Some("0 hits".into()),
        copy_text: None,
        actions: vec![],
        preview: Some(everything_syntax_preview()),
    }
}

fn offline_item(provider_id: &str) -> ResultItem {
    ResultItem {
        id: "everything.not_running".into(),
        provider_id: provider_id.into(),
        title: "Everything is not running".into(),
        subtitle: "Start Voidtools Everything (1.4 or 1.5a) to unlock instant sub-ms file search"
            .into(),
        category: ResultCategory::System,
        score: 9_000,
        icon: IconSpec::Monogram("EV".into()),
        badge: Some("Offline".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "download".into(),
            label: "Open voidtools.com".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::OpenUrl {
                url: "https://www.voidtools.com/".into(),
            },
        }],
        preview: Some(everything_syntax_preview()),
    }
}

fn everything_syntax_preview() -> PreviewSpec {
    PreviewSpec::KeyValue {
        title: "Everything Power Filters".into(),
        entries: vec![
            ("ext:rs;toml".into(), "Match extensions".into()),
            ("dm:today".into(), "Modified today".into()),
            ("size:>50mb".into(), "Larger than 50 MB".into()),
            ("folder: docs".into(), "Folders only".into()),
            ("@pdf / @excel".into(), "Quick group filter tags".into()),
            ("Drag & Drop".into(), "Drag any result out to Explorer/IDE".into()),
        ],
    }
}

fn hint_items(lang: Lang, config: &Arc<RwLock<TomoyoConfig>>) -> Vec<ResultItem> {
    let status = match tomoyo_everything::detect_running() {
        Some(tomoyo_everything::EverythingVersion::V15Alpha) => {
            lang.pick("Everything 1.5a 已连接", "Everything 1.5a Connected")
        }
        Some(tomoyo_everything::EverythingVersion::V14) => {
            lang.pick("Everything 1.4 已连接", "Everything 1.4 Connected")
        }
        None => lang.pick("Everything 未运行", "Everything Offline"),
    };
    let mut items = vec![ResultItem {
        id: "everything.hint".into(),
        provider_id: "builtin.everything".into(),
        title: lang
            .pick(
                "输入关键词通过 Everything 毫秒级搜文件...",
                "Type to search files via Everything...",
            )
            .into(),
        subtitle: lang
            .pick(
                "支持语法：f Cargo.toml · f ext:rs tomoyo · 下方为收藏筛选器与历史记录",
                "Examples: f Cargo.toml · f ext:rs · Favorites & history listed below",
            )
            .into(),
        category: ResultCategory::File,
        score: 9_500,
        icon: IconSpec::Monogram("EV".into()),
        badge: Some(status.into()),
        copy_text: None,
        actions: vec![],
        preview: Some(everything_syntax_preview()),
    }];

    if let Ok(cfg) = config.read() {
        for (idx, fav) in cfg.everything.favorites.iter().take(6).enumerate() {
            items.push(ResultItem {
                id: format!("ev:fav:{}", fav.tag),
                provider_id: "builtin.everything".into(),
                title: format!("★ {} · {}", fav.tag, fav.label),
                subtitle: format!("f {}", fav.expr),
                category: ResultCategory::File,
                score: 9_200 - idx as i64 * 10,
                icon: IconSpec::Monogram("★".into()),
                badge: Some(fav.tag.clone()),
                copy_text: Some(fav.expr.clone()),
                actions: vec![ActionSpec {
                    id: "fill_fav".into(),
                    label: lang.pick("一键填入收藏筛选", "Apply Favorite Filter").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::FillQuery {
                        query: format!("f {} ", fav.expr),
                    },
                }],
                preview: Some(everything_syntax_preview()),
            });
        }
    }

    for (idx, prev) in recent_everything_queries(6).into_iter().enumerate() {
        items.push(ResultItem {
            id: format!("ev:hist:{idx}"),
            provider_id: "builtin.everything".into(),
            title: format!("↺ {prev}"),
            subtitle: lang.pick("最近 Everything 搜索记录 · 回车直接复用", "Recent Everything search").into(),
            category: ResultCategory::File,
            score: 8_800 - idx as i64 * 10,
            icon: IconSpec::Monogram("HIS".into()),
            badge: Some(lang.pick("历史", "HIST").into()),
            copy_text: Some(prev.clone()),
            actions: vec![ActionSpec {
                id: "fill_hist".into(),
                label: lang.pick("重新搜索该记录", "Re-run Query").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::FillQuery {
                    query: format!("f {prev}"),
                },
            }],
            preview: None,
        });
    }
    items
}
