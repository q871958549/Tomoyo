use std::path::Path;
use std::sync::{Arc, RwLock};
use tomoyo_core::{Lang, Provider, TomoyoConfig};
use tomoyo_everything::{EverythingQueryOptions, SortOrder};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, PreviewSpec, QueryContext, ResultCategory, ResultItem,
};

/// First-class native Everything 1.4 & 1.5a file search provider (zero external DLLs)
/// with live file preview support, i18n, and customizable filters.
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
        let search_text = if dedicated {
            ctx.args.trim()
        } else {
            ctx.trimmed.as_str()
        };

        if search_text.is_empty() {
            if dedicated {
                return vec![hint_item(lang)];
            }
            return Vec::new();
        }

        if !dedicated && (search_text.chars().count() < 2 || search_text.starts_with("http")) {
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

        let effective_query = if dedicated && !extra_filter.trim().is_empty() {
            format!("{search_text} {extra_filter}")
        } else {
            search_text.to_string()
        };

        let max_results = if dedicated {
            ctx.limit.min(cfg_ded).clamp(1, 50) as u32
        } else {
            ctx.limit.min(cfg_global).clamp(1, 10) as u32
        };

        let opts = EverythingQueryOptions {
            max_results,
            sort: if dedicated {
                SortOrder::DateModifiedDesc
            } else {
                SortOrder::RunCountDesc
            },
            timeout_ms: if dedicated { 500 } else { 200 },
            ..Default::default()
        };

        match tomoyo_everything::query(&effective_query, &opts) {
            Ok(resp) => {
                if resp.entries.is_empty() && dedicated {
                    return vec![ResultItem {
                        id: "everything.empty".into(),
                        provider_id: self.id().into(),
                        title: format!("No files matching `{search_text}`"),
                        subtitle:
                            "Everything syntax supported: ext:rs · dm:today · size:>10mb · folder:"
                                .into(),
                        category: ResultCategory::File,
                        score: 100,
                        icon: IconSpec::Monogram("EV".into()),
                        badge: Some("0 hits".into()),
                        copy_text: None,
                        actions: vec![],
                        preview: Some(everything_syntax_preview()),
                    }];
                }

                let base_score: i64 = if dedicated { 8_500 } else { 4_200 };
                resp.entries
                    .into_iter()
                    .enumerate()
                    .map(|(idx, entry)| {
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
                            Some("Folder".into())
                        } else {
                            entry.formatted_size()
                        };

                        ResultItem {
                            id: format!("ev:{}", entry.full_path),
                            provider_id: self.id().into(),
                            title: entry.name.clone(),
                            subtitle,
                            category: if entry.is_folder {
                                ResultCategory::Folder
                            } else {
                                ResultCategory::File
                            },
                            score: base_score - (idx as i64 * 25),
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
                    })
                    .collect()
            }
            Err(tomoyo_everything::EverythingError::NotRunning) => {
                if dedicated {
                    vec![ResultItem {
                        id: "everything.not_running".into(),
                        provider_id: self.id().into(),
                        title: "Everything is not running".into(),
                        subtitle:
                            "Start Voidtools Everything (1.4 or 1.5a) to unlock instant sub-ms file search"
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
                    }]
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        }
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
            ("regex:^foo.*".into(), "Regular expression".into()),
            ("Drag & Drop".into(), "Drag any result out to Explorer/IDE".into()),
        ],
    }
}

fn hint_item(lang: Lang) -> ResultItem {
    let status = match tomoyo_everything::detect_running() {
        Some(tomoyo_everything::EverythingVersion::V15Alpha) => lang.pick("Everything 1.5a 已连接", "Everything 1.5a Connected"),
        Some(tomoyo_everything::EverythingVersion::V14) => lang.pick("Everything 1.4 已连接", "Everything 1.4 Connected"),
        None => lang.pick("Everything 未运行", "Everything Offline"),
    };
    ResultItem {
        id: "everything.hint".into(),
        provider_id: "builtin.everything".into(),
        title: lang
            .pick("输入关键词通过 Everything 毫秒级搜文件...", "Type to search files via Everything...")
            .into(),
        subtitle: lang
            .pick(
                "支持语法：f Cargo.toml · f ext:rs tomoyo · f dm:today · 支持左键拖拽文件到其他窗口",
                "Examples: f Cargo.toml · f ext:rs tomoyo · f dm:today · Drag rows to drop files",
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
