//! `:update` GitHub Releases status card builder.

use tomoyo_core::{
    build_diagnostic_bundle, count_recent_log_issues, logs_dir, read_log_tail, Lang, UpdateManager,
    UpdateStatus, CURRENT_VERSION,
};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, PreviewSpec, ResultCategory, ResultItem,
};

pub fn build_update_item(
    provider_id: &str,
    updater: &UpdateManager,
    repo: &str,
    lang: Lang,
) -> ResultItem {
    let status = updater.snapshot();
    let (title, subtitle, badge, actions, preview) = match status {
        UpdateStatus::Available { release, .. } => {
            let dl = release
                .download_url
                .clone()
                .unwrap_or_else(|| release.html_url.clone());
            (
                match lang {
                    Lang::Zh => format!(":update — 发现新版本 v{}（当前 v{}）", release.version, CURRENT_VERSION),
                    Lang::En => format!(":update — New Release v{} available (current v{})", release.version, CURRENT_VERSION),
                },
                match lang {
                    Lang::Zh => format!("{} · 回车自动下载并校验 SHA-256 安装升级", release.name),
                    Lang::En => format!("{} · Enter to download, verify SHA-256 & install", release.name),
                },
                format!("v{}", release.version),
                vec![
                    ActionSpec {
                        id: "install_update".into(),
                        label: lang.pick("自动下载校验并安装新版本", "Download, Verify & Install Update").into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::InternalCommand {
                            command: InternalCommandKind::InstallUpdate,
                        },
                    },
                    ActionSpec {
                        id: "open_release".into(),
                        label: lang.pick("在浏览器打开 GitHub Release", "Open GitHub Release").into(),
                        shortcut: Some("Ctrl+Enter".into()),
                        kind: ActionKind::OpenUrl { url: dl },
                    },
                    ActionSpec {
                        id: "recheck".into(),
                        label: lang.pick("重新检查更新", "Re-check Now").into(),
                        shortcut: None,
                        kind: ActionKind::InternalCommand {
                            command: InternalCommandKind::CheckForUpdates,
                        },
                    },
                ],
                Some(PreviewSpec::Text {
                    title: format!("Release {} ({})", release.tag, release.published_at),
                    body: if release.notes.is_empty() {
                        release.html_url
                    } else {
                        release.notes
                    },
                }),
            )
        }
        UpdateStatus::UpToDate { latest, note, .. } => (
            match lang {
                Lang::Zh => format!(":update — 当前已是最新版本 v{CURRENT_VERSION}"),
                Lang::En => format!(":update — Up to date (v{CURRENT_VERSION})"),
            },
            if note.is_empty() {
                format!("GitHub ({repo}) 最新版本: v{latest} · 回车立即重新检查")
            } else {
                format!("{note} · 回车重新检查")
            },
            format!("v{CURRENT_VERSION}"),
            vec![
                ActionSpec {
                    id: "check".into(),
                    label: lang.pick("立即联网检查更新", "Check GitHub Now").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::CheckForUpdates,
                    },
                },
                ActionSpec {
                    id: "open_repo".into(),
                    label: lang.pick("打开 GitHub Releases", "Open GitHub Releases").into(),
                    shortcut: None,
                    kind: ActionKind::OpenUrl {
                        url: format!("https://github.com/{repo}/releases"),
                    },
                },
            ],
            None,
        ),
        UpdateStatus::Error { message, .. } => (
            format!(":update — {}", lang.pick("检查更新失败", "Update Check Failed")),
            format!("{message} · {}", lang.pick("回车重试 · Ctrl+Enter 打开设置中心", "Enter retry · Ctrl+Enter Settings")),
            "ERR".into(),
            vec![
                ActionSpec {
                    id: "retry".into(),
                    label: lang.pick("重试检查更新", "Retry Check").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::CheckForUpdates,
                    },
                },
                ActionSpec {
                    id: "open_settings".into(),
                    label: lang.pick("打开设置中心", "Open Settings").into(),
                    shortcut: Some("Ctrl+Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::OpenSettingsWindow,
                    },
                },
                ActionSpec {
                    id: "open_repo".into(),
                    label: lang.pick("打开 GitHub Releases", "Open GitHub Releases").into(),
                    shortcut: None,
                    kind: ActionKind::OpenUrl {
                        url: format!("https://github.com/{repo}/releases"),
                    },
                },
            ],
            Some(PreviewSpec::Text {
                title: lang.pick("检查更新诊断详情", "Update Diagnostic Details").into(),
                body: format!("{message}\n\nhttps://github.com/{repo}/releases"),
            }),
        ),
        UpdateStatus::Checking => (
            lang.pick(":update — 正在连接 GitHub 检查最新版本...", ":update — Checking GitHub Releases...").into(),
            format!("Repository: {repo}"),
            "...".into(),
            vec![],
            None,
        ),
        UpdateStatus::Idle => (
            match lang {
                Lang::Zh => format!(":update — 检查软件更新（当前 v{CURRENT_VERSION}）"),
                Lang::En => format!(":update — Check for Updates (v{CURRENT_VERSION})"),
            },
            format!("GitHub: https://github.com/{repo}/releases · 回车立即检查"),
            format!("v{CURRENT_VERSION}"),
            vec![
                ActionSpec {
                    id: "check".into(),
                    label: lang.pick("立即检查更新", "Check Now").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::CheckForUpdates,
                    },
                },
                ActionSpec {
                    id: "open_repo".into(),
                    label: lang.pick("打开 GitHub Releases", "Open GitHub Releases").into(),
                    shortcut: None,
                    kind: ActionKind::OpenUrl {
                        url: format!("https://github.com/{repo}/releases"),
                    },
                },
            ],
            None,
        ),
    };

    ResultItem {
        id: "diag:update".into(),
        provider_id: provider_id.into(),
        title,
        subtitle,
        category: ResultCategory::System,
        score: 9_930,
        icon: IconSpec::Monogram("UPD".into()),
        badge: Some(badge),
        copy_text: None,
        actions,
        preview,
    }
}

pub fn build_logs_item(provider_id: &str, lang: Lang) -> ResultItem {
    let (warns, errs, has_crash) = count_recent_log_issues();
    let dir_str = logs_dir().to_string_lossy().into_owned();
    let tail = read_log_tail("tomoyo.log", 28, false);
    let bundle = build_diagnostic_bundle();
    let badge = if has_crash {
        "CRASH".into()
    } else if errs > 0 {
        format!("{errs} ERR")
    } else if warns > 0 {
        format!("{warns} WARN")
    } else {
        "OK".into()
    };
    ResultItem {
        id: "diag:logs".into(),
        provider_id: provider_id.into(),
        title: match lang {
            Lang::Zh => format!(":logs — 运行日志与异常溯源（{errs} 错误 · {warns} 警告）"),
            Lang::En => format!(":logs — Runtime Logs & Diagnostics ({errs} err · {warns} warn)"),
        },
        subtitle: match lang {
            Lang::Zh => format!("Enter 打开日志目录 · Ctrl+Enter 复制完整诊断报告 · {dir_str}"),
            Lang::En => format!("Enter open logs folder · Ctrl+Enter copy diagnostic bundle · {dir_str}"),
        },
        category: ResultCategory::System,
        score: 9_660,
        icon: IconSpec::Monogram("LOG".into()),
        badge: Some(badge),
        copy_text: Some(bundle.clone()),
        actions: vec![
            ActionSpec {
                id: "open_logs_dir".into(),
                label: lang.pick("打开日志目录", "Open Logs Folder").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::LaunchPath {
                    path: dir_str,
                    args: None,
                    working_dir: None,
                },
            },
            ActionSpec {
                id: "copy_diag".into(),
                label: lang.pick("复制完整诊断报告", "Copy Diagnostic Report").into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText {
                    text: bundle,
                    paste_immediately: false,
                },
            },
        ],
        preview: Some(PreviewSpec::Text {
            title: lang
                .pick("最近运行日志 (tomoyo.log)", "Recent Runtime Logs (tomoyo.log)")
                .into(),
            body: if tail.is_empty() {
                "(日志为空)".into()
            } else {
                tail
            },
        }),
    }
}
