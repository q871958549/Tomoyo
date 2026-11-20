use std::path::Path;
use std::sync::{Arc, RwLock};
use tomoyo_core::{score_match, IndexedText, Lang, Provider, TomoyoConfig};
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
                return vec![hint_item(lang)];
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
        let max_results = if expanded {
            ctx.limit.min(cfg_ded).clamp(1, 50) as u32
        } else {
            ctx.limit.min(cfg_global.max(8)).clamp(1, 12) as u32
        };

        let opts = EverythingQueryOptions {
            max_results,
            sort: if expanded {
                SortOrder::DateModifiedDesc
            } else {
                SortOrder::RunCountDesc
            },
            timeout_ms: if expanded { 500 } else { 200 },
            ..Default::default()
        };

        match tomoyo_everything::query(&effective_query, &opts) {
            Ok(resp) => {
                if resp.entries.is_empty() && expanded {
                    return vec![empty_result_item(self.id(), search_text, ctx.filter, lang)];
                }

                let mut items: Vec<ResultItem> = resp
                    .entries
                    .into_iter()
                    .enumerate()
                    .map(|(idx, entry)| {
                        let score = compute_ev_score(
                            search_text,
                            &entry.name,
                            entry.is_folder,
                            expanded,
                            idx,
                        );
                        build_entry_item(self.id(), entry, score, lang)
                    })
                    .collect();

                if !expanded {
                    items.sort_by(|a, b| b.score.cmp(&a.score));
                    items.truncate(cfg_global.clamp(1, 10));
                }
                items
            }
            Err(tomoyo_everything::EverythingError::NotRunning) => {
                if expanded {
                    vec![offline_item(self.id())]
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        }
    }
}

fn compute_ev_score(
    search_text: &str,
    name: &str,
    is_folder: bool,
    expanded: bool,
    idx: usize,
) -> i64 {
    if expanded {
        return 8_500 - (idx as i64 * 25);
    }
    let name_score = if search_text.is_empty() {
        4_200
    } else {
        let indexed = IndexedText::new(name);
        match score_match(search_text, &indexed) {
            Some(s) if s >= 8_500 => 6_100,
            Some(s) if s >= 6_200 => 5_100,
            Some(s) if s >= 4_400 => 4_500,
            Some(_) => 3_950,
            None => 3_800,
        }
    };
    let ext_lower = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let kind_boost = if matches!(ext_lower.as_str(), "exe" | "lnk") {
        350
    } else if is_folder {
        150
    } else {
        0
    };
    name_score + kind_boost - (idx as i64 * 20)
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

fn hint_item(lang: Lang) -> ResultItem {
    let status = match tomoyo_everything::detect_running() {
        Some(tomoyo_everything::EverythingVersion::V15Alpha) => {
            lang.pick("Everything 1.5a 已连接", "Everything 1.5a Connected")
        }
        Some(tomoyo_everything::EverythingVersion::V14) => {
            lang.pick("Everything 1.4 已连接", "Everything 1.4 Connected")
        }
        None => lang.pick("Everything 未运行", "Everything Offline"),
    };
    ResultItem {
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
                "支持语法：f Cargo.toml · f ext:rs tomoyo · 点击下方分组胶囊筛选 PDF/Excel/Word/PPT",
                "Examples: f Cargo.toml · f ext:rs · Use filter chips for PDF/Excel/Word/PPT",
            )
            .into(),
        category: ResultCategory::File,
        score: 9_000,
        icon: IconSpec::Monogram("EV".into()),
        badge: Some(status.into()),
        copy_text: None,
        actions: vec![],
        preview: Some(everything_syntax_preview()),
    }
}
