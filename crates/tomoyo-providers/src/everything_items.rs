use crate::everything_history::recent_everything_queries;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tomoyo_core::{Lang, TomoyoConfig};
use tomoyo_protocol::{
    extension_short_tag, ActionKind, ActionSpec, IconSpec, PreviewSpec, ResultCategory,
    ResultItem, SearchFilter,
};

pub(super) fn build_entry_item(
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

    let mut actions = vec![
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
            label: lang.pick("在资源管理器中定位", "Reveal in File Manager").into(),
            shortcut: Some("Ctrl+Enter".into()),
            kind: ActionKind::RevealInExplorer {
                path: entry.full_path.clone(),
            },
        },
        ActionSpec {
            id: "terminal".into(),
            label: lang.pick("在此处打开终端", "Open Terminal Here").into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::OpenTerminalAt { dir: parent_dir.clone() },
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
            id: "copy_name".into(),
            label: lang.pick("复制文件名", "Copy File Name").into(),
            shortcut: None,
            kind: ActionKind::CopyText {
                text: entry.name.clone(),
                paste_immediately: false,
            },
        },
    ];
    if !entry.is_folder {
        crate::file_actions::append_contextual_file_actions(
            &mut actions,
            &entry.full_path,
            &parent_dir,
            lang,
        );
    }
    ResultItem {
        id: format!("ev:{}", entry.full_path),
        provider_id: provider_id.into(),
        title: entry.name,
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
        actions,
        preview: Some(PreviewSpec::FilePreview {
            path: entry.full_path,
        }),
    }
}

pub(super) fn empty_result_item(
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
        subtitle: lang
            .pick(
                "试试自然语言：f 今天 pdf · f 大文件 · f 下载目录 · @pdf",
                "Try: f today pdf · f large · f downloads · @pdf",
            )
            .into(),
        category: ResultCategory::File,
        score: 100,
        icon: IconSpec::Monogram("FILE".into()),
        badge: Some("0 hits".into()),
        copy_text: None,
        actions: vec![],
        preview: Some(everything_syntax_preview(lang)),
    }
}

pub(super) fn offline_item(provider_id: &str) -> ResultItem {
    if cfg!(target_os = "linux") {
        return ResultItem {
            id: "everything.not_running".into(),
            provider_id: provider_id.into(),
            title: "未检测到文件检索工具 (fd / plocate)".into(),
            subtitle: "安装 `fd` 或 `plocate` 可解锁本地极速文件检索".into(),
            category: ResultCategory::System,
            score: 200,
            icon: IconSpec::Monogram("FD".into()),
            badge: Some("Offline".into()),
            copy_text: None,
            actions: vec![],
            preview: Some(everything_syntax_preview(Lang::Zh)),
        };
    }
    ResultItem {
        id: "everything.not_running".into(),
        provider_id: provider_id.into(),
        title: "Everything is not running".into(),
        subtitle: "Start Voidtools Everything (1.4 or 1.5a) to unlock instant sub-ms file search"
            .into(),
        category: ResultCategory::System,
        score: 200,
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
        preview: Some(everything_syntax_preview(Lang::En)),
    }
}

fn everything_syntax_preview(lang: Lang) -> PreviewSpec {
    let zh = lang == Lang::Zh;
    PreviewSpec::KeyValue {
        title: if cfg!(target_os = "linux") {
            if zh { "文件快捷筛选" } else { "File Search Filters (fd)" }
        } else if zh {
            "自然语言与快捷筛选"
        } else {
            "Natural & Power Filters"
        }
        .into(),
        entries: if zh {
            vec![
                ("f 今天 pdf".into(), "今天修改的 PDF 文档".into()),
                ("f 大文件".into(), "大于 50MB 的大文件".into()),
                ("f 近7天 合同".into(), "近 7 天内含“合同”的文件".into()),
                ("f 下载目录".into(), "仅在 Downloads 目录查找".into()),
                ("@pdf / @excel".into(), "一键切换分类过滤标签".into()),
                ("Tab".into(), "打开定位 / 终端 / 校验等动作".into()),
            ]
        } else {
            vec![
                ("f today pdf".into(), "PDFs modified today".into()),
                ("f large".into(), "Files larger than 50MB".into()),
                ("f last7days".into(), "Modified within 7 days".into()),
                ("f downloads".into(), "Files inside Downloads".into()),
                ("ext:rs;toml".into(), "Raw extension filter".into()),
                ("Tab".into(), "Open reveal / terminal / SHA-256".into()),
            ]
        },
    }
}

pub(super) fn hint_items(lang: Lang, config: &Arc<RwLock<TomoyoConfig>>) -> Vec<ResultItem> {
    let status = if cfg!(target_os = "linux") {
        #[cfg(target_os = "linux")]
        let avail = crate::everything_linux::is_linux_file_search_available();
        #[cfg(not(target_os = "linux"))]
        let avail = false;
        if avail {
            lang.pick("文件检索已就绪 (fd)", "File Search Ready (fd)")
        } else {
            lang.pick("未安装 fd/plocate", "fd/plocate Not Found")
        }
    } else {
        match tomoyo_everything::detect_running() {
            Some(tomoyo_everything::EverythingVersion::V15Alpha) => {
                lang.pick("Everything 1.5a 已连接", "Everything 1.5a Connected")
            }
            Some(tomoyo_everything::EverythingVersion::V14) => {
                lang.pick("Everything 1.4 已连接", "Everything 1.4 Connected")
            }
            None => lang.pick("Everything 未运行", "Everything Offline"),
        }
    };
    let mut items = vec![ResultItem {
        id: "everything.hint".into(),
        provider_id: "builtin.everything".into(),
        title: lang
            .pick(
                "直接说人话搜索本地文件，例如 `f 今天 pdf`...",
                "Search files naturally, e.g. `f today pdf`...",
            )
            .into(),
        subtitle: lang
            .pick(
                "支持自然筛选：f 今天 · f 大文件 · f 近7天 合同 · f 下载目录（也兼容 ext: / dm:）",
                "Natural filters: f today · f large · f last7days · f downloads (plus ext:/dm:)",
            )
            .into(),
        category: ResultCategory::File,
        score: 9_500,
        icon: IconSpec::Monogram("FD".into()),
        badge: Some(status.into()),
        copy_text: None,
        actions: vec![],
        preview: Some(everything_syntax_preview(lang)),
    }];

    let starters: [(&str, &str, &str, &str, &str); 4] = [
        ("nl:today", "今日修改的文件", "Files modified today", "f 今天 ", "TODAY"),
        ("nl:large", "查找大文件（>50MB）", "Large files (>50MB)", "f 大文件 ", ">50M"),
        ("nl:pdf7", "近 7 天 PDF 文档", "Recent 7-day PDFs", "f 近7天 pdf ", "PDF"),
        ("nl:dl", "下载目录里的文件", "Files in Downloads", "f 下载目录 ", "DL"),
    ];
    for (idx, (sid, tz, te, q, bdg)) in starters.into_iter().enumerate() {
        items.push(ResultItem {
            id: format!("ev:{sid}"),
            provider_id: "builtin.everything".into(),
            title: if lang == Lang::Zh { tz } else { te }.into(),
            subtitle: format!("{q}· {}", lang.pick("回车一键填入并筛选", "Enter to apply starter")),
            category: ResultCategory::File,
            score: 9_400 - (idx as i64 * 15),
            icon: IconSpec::Monogram(bdg.into()),
            badge: Some(bdg.into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "fill_starter".into(),
                label: lang.pick("填入此筛选条件", "Apply Starter").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::FillQuery { query: q.into() },
            }],
            preview: Some(everything_syntax_preview(lang)),
        });
    }

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
                preview: Some(everything_syntax_preview(lang)),
            });
        }
    }

    for (idx, prev) in recent_everything_queries(6).into_iter().enumerate() {
        items.push(ResultItem {
            id: format!("ev:hist:{idx}"),
            provider_id: "builtin.everything".into(),
            title: format!("↺ {prev}"),
            subtitle: lang
                .pick("最近文件搜索记录 · 回车直接复用", "Recent file search")
                .into(),
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
