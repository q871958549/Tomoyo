use base64::Engine;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tomoyo_core::{
    current_process_stats, is_autostart_enabled, score_match, ExternalPluginBridge, IndexedText,
    Lang, Provider, TomoyoConfig, UpdateManager, UpdateStatus, CURRENT_VERSION,
};
use tomoyo_protocol::{
    ActionKind, ActionSpec, AttachedContext, IconSpec, InternalCommandKind, PreviewSpec,
    QueryContext, ResultCategory, ResultItem, SystemCommandKind,
};

struct SysEntry {
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    badge: &'static str,
    indexed: IndexedText,
    kind: ActionKind,
}

/// System commands, Super Match attached-capsule actions, Auto-Update (`:update`), and Diagnostics.
pub struct SystemProvider {
    commands: Vec<SysEntry>,
    plugin_bridge: Option<Arc<dyn ExternalPluginBridge>>,
    config: Arc<RwLock<TomoyoConfig>>,
    updater: Arc<UpdateManager>,
}

impl SystemProvider {
    pub fn new(
        plugin_bridge: Option<Arc<dyn ExternalPluginBridge>>,
        config: Arc<RwLock<TomoyoConfig>>,
        updater: Arc<UpdateManager>,
    ) -> Self {
        let specs: [(&str, &str, &str, &str, ActionKind); 8] = [
            (
                "sys:tomoyo_settings",
                "Tomoyo 设置中心 Settings",
                "打开图形化设置中心（主题、快捷键自动录入、语言、Everything、插件市场、检查更新）",
                "SET",
                ActionKind::InternalCommand {
                    command: InternalCommandKind::OpenSettingsWindow,
                },
            ),
            (
                "sys:check_update",
                "检查更新 Check for Updates",
                "联网检查 GitHub 最新 Release 版本并查看更新日志",
                "UPD",
                ActionKind::InternalCommand {
                    command: InternalCommandKind::CheckForUpdates,
                },
            ),
            (
                "sys:lock",
                "Lock Screen 锁屏",
                "Lock your Windows workstation immediately",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::LockWorkstation,
                },
            ),
            (
                "sys:sleep",
                "Sleep 睡眠",
                "Put computer into low-power sleep mode",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::Sleep,
                },
            ),
            (
                "sys:recycle",
                "Empty Recycle Bin 清空回收站",
                "Permanently clear Windows Recycle Bin",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::EmptyRecycleBin,
                },
            ),
            (
                "sys:settings",
                "Windows Settings 系统设置",
                "Open Windows Settings (ms-settings:)",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::OpenSettings,
                },
            ),
            (
                "sys:taskmgr",
                "Task Manager 任务管理器",
                "Open Windows Task Manager",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::OpenTaskManager,
                },
            ),
            (
                "sys:terminal",
                "Terminal 终端",
                "Open Windows Terminal / PowerShell",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::OpenTerminal,
                },
            ),
        ];

        let commands = specs
            .into_iter()
            .map(|(id, title, subtitle, badge, kind)| SysEntry {
                id,
                title,
                subtitle,
                badge,
                indexed: IndexedText::new(title),
                kind,
            })
            .collect();

        Self {
            commands,
            plugin_bridge,
            config,
            updater,
        }
    }

    fn build_update_item(&self, lang: Lang) -> ResultItem {
        let repo = self
            .config
            .read()
            .map(|c| c.update.repo.clone())
            .unwrap_or_else(|_| "q871958549/Tomoyo".into());
        let status = self.updater.snapshot();
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
                    format!("{} · 回车直达下载页面", release.name),
                    format!("v{}", release.version),
                    vec![
                        ActionSpec {
                            id: "open_release".into(),
                            label: lang.pick("前往 GitHub 下载新版本", "Download New Release").into(),
                            shortcut: Some("Enter".into()),
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
                        label: lang.pick("打开 GitHub 仓库", "Open GitHub Repository").into(),
                        shortcut: None,
                        kind: ActionKind::OpenUrl {
                            url: format!("https://github.com/{repo}/releases"),
                        },
                    },
                ],
                None,
            ),
            UpdateStatus::Error { message, .. } => (
                format!(":update — 检查失败 ({message})"),
                lang.pick("回车重试检查 GitHub Releases", "Enter to retry GitHub Releases check").into(),
                "ERR".into(),
                vec![ActionSpec {
                    id: "retry".into(),
                    label: lang.pick("重试检查更新", "Retry Check").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::CheckForUpdates,
                    },
                }],
                None,
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
            provider_id: self.id().into(),
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

    fn build_super_match_items(&self, attached: &AttachedContext, filter: &str, lang: Lang) -> Vec<ResultItem> {
        let mut items = Vec::new();
        let f = filter.to_lowercase();

        match attached {
            AttachedContext::Image {
                path,
                width,
                height,
            } => {
                let b64_preview = std::fs::read(path)
                    .ok()
                    .filter(|b| b.len() <= 2 * 1024 * 1024)
                    .map(|b| {
                        format!(
                            "data:image/png;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(&b)
                        )
                    });

                items.push(ResultItem {
                    id: "super:img:open".into(),
                    provider_id: self.id().into(),
                    title: lang.pick("打开粘贴的图片", "Open Pasted Image").into(),
                    subtitle: format!("{width}×{height} PNG · {path}"),
                    category: ResultCategory::File,
                    score: 10_500,
                    icon: IconSpec::Monogram("IMG".into()),
                    badge: Some(format!("{width}×{height}")),
                    copy_text: Some(path.clone()),
                    actions: vec![
                        ActionSpec {
                            id: "open".into(),
                            label: lang.pick("用系统看图打开", "Open Image").into(),
                            shortcut: Some("Enter".into()),
                            kind: ActionKind::LaunchPath {
                                path: path.clone(),
                                args: None,
                                working_dir: None,
                            },
                        },
                        ActionSpec {
                            id: "reveal".into(),
                            label: lang.pick("在资源管理器定位", "Reveal in Explorer").into(),
                            shortcut: None,
                            kind: ActionKind::RevealInExplorer { path: path.clone() },
                        },
                    ],
                    preview: Some(PreviewSpec::ImagePreview {
                        path: path.clone(),
                        width: *width,
                        height: *height,
                    }),
                });

                if let Some(uri) = b64_preview {
                    let uri_len = uri.len();
                    items.push(ResultItem {
                        id: "super:img:b64".into(),
                        provider_id: self.id().into(),
                        title: lang.pick("复制图片为 Base64 Data URI", "Copy Image as Base64 Data URI").into(),
                        subtitle: format!("data:image/png;base64,... ({uri_len} 字符)"),
                        category: ResultCategory::Calc,
                        score: 10_400,
                        icon: IconSpec::Monogram("B64".into()),
                        badge: Some("BASE64".into()),
                        copy_text: Some(uri.clone()),
                        actions: vec![ActionSpec {
                            id: "copy_b64".into(),
                            label: lang.pick("复制 Base64 Data URI", "Copy Base64").into(),
                            shortcut: Some("Enter".into()),
                            kind: ActionKind::CopyText {
                                text: uri,
                                paste_immediately: false,
                            },
                        }],
                        preview: Some(PreviewSpec::ImagePreview {
                            path: path.clone(),
                            width: *width,
                            height: *height,
                        }),
                    });
                }

                items.push(ResultItem {
                    id: "super:img:reveal".into(),
                    provider_id: self.id().into(),
                    title: lang.pick("在资源管理器中定位该图片文件", "Reveal Image File in Explorer").into(),
                    subtitle: path.clone(),
                    category: ResultCategory::Folder,
                    score: 10_300,
                    icon: IconSpec::Monogram("DIR".into()),
                    badge: Some("PNG".into()),
                    copy_text: Some(path.clone()),
                    actions: vec![ActionSpec {
                        id: "reveal".into(),
                        label: lang.pick("在资源管理器定位", "Reveal in Explorer").into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::RevealInExplorer { path: path.clone() },
                    }],
                    preview: Some(PreviewSpec::ImagePreview {
                        path: path.clone(),
                        width: *width,
                        height: *height,
                    }),
                });
            }
            AttachedContext::Files { paths } => {
                if let Some(first) = paths.first() {
                    let joined = paths.join("\n");
                    let parent = Path::new(first)
                        .parent()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| first.clone());

                    items.push(ResultItem {
                        id: "super:files:copy_paths".into(),
                        provider_id: self.id().into(),
                        title: match lang {
                            Lang::Zh => format!("复制已粘贴的 {} 个文件路径", paths.len()),
                            Lang::En => format!("Copy {} Pasted File Path(s)", paths.len()),
                        },
                        subtitle: first.clone(),
                        category: ResultCategory::File,
                        score: 10_500,
                        icon: IconSpec::ShellPath(first.clone()),
                        badge: Some(format!("{} files", paths.len())),
                        copy_text: Some(joined.clone()),
                        actions: vec![ActionSpec {
                            id: "copy".into(),
                            label: lang.pick("复制全部路径", "Copy All Paths").into(),
                            shortcut: Some("Enter".into()),
                            kind: ActionKind::CopyText {
                                text: joined,
                                paste_immediately: false,
                            },
                        }],
                        preview: Some(PreviewSpec::FilePreview { path: first.clone() }),
                    });

                    // Fast SHA-256 hash for files <= 32 MB
                    if Path::new(first).is_file() {
                        if let Ok(meta) = std::fs::metadata(first) {
                            if meta.len() <= 32 * 1024 * 1024 {
                                if let Ok(data) = std::fs::read(first) {
                                    let digest: String = Sha256::digest(&data)
                                        .iter()
                                        .map(|b| format!("{b:02x}"))
                                        .collect();
                                    items.push(ResultItem {
                                        id: "super:files:sha256".into(),
                                        provider_id: self.id().into(),
                                        title: format!("SHA-256: {digest}"),
                                        subtitle: lang.pick("已自动计算文件校验和 · 回车复制", "File SHA-256 checksum · Enter to copy").into(),
                                        category: ResultCategory::Calc,
                                        score: 10_400,
                                        icon: IconSpec::Monogram("SHA".into()),
                                        badge: Some("SHA256".into()),
                                        copy_text: Some(digest.clone()),
                                        actions: vec![ActionSpec {
                                            id: "copy_sha".into(),
                                            label: lang.pick("复制 SHA-256", "Copy SHA-256").into(),
                                            shortcut: Some("Enter".into()),
                                            kind: ActionKind::CopyText {
                                                text: digest.clone(),
                                                paste_immediately: false,
                                            },
                                        }],
                                        preview: Some(PreviewSpec::KeyValue {
                                            title: "文件校验信息".into(),
                                            entries: vec![
                                                ("文件".into(), first.clone()),
                                                ("大小".into(), format!("{} B", meta.len())),
                                                ("SHA-256".into(), digest),
                                            ],
                                        }),
                                    });
                                }
                            }
                        }
                    }

                    items.push(ResultItem {
                        id: "super:files:terminal".into(),
                        provider_id: self.id().into(),
                        title: lang.pick("在文件所在目录打开终端", "Open Terminal at File Directory").into(),
                        subtitle: parent.clone(),
                        category: ResultCategory::Folder,
                        score: 10_300,
                        icon: IconSpec::Monogram("CMD".into()),
                        badge: Some("TERM".into()),
                        copy_text: Some(parent.clone()),
                        actions: vec![ActionSpec {
                            id: "term".into(),
                            label: lang.pick("打开终端", "Open Terminal").into(),
                            shortcut: Some("Enter".into()),
                            kind: ActionKind::OpenTerminalAt { dir: parent },
                        }],
                        preview: None,
                    });
                }
            }
        }

        items.push(ResultItem {
            id: "super:clear".into(),
            provider_id: self.id().into(),
            title: lang.pick("清除当前粘贴胶囊（或按 Backspace）", "Clear Attached Capsule (or press Backspace)").into(),
            subtitle: lang.pick("返回普通搜索模式", "Return to normal search mode").into(),
            category: ResultCategory::System,
            score: 9_900,
            icon: IconSpec::Monogram("CLR".into()),
            badge: Some("ESC".into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "clear".into(),
                label: lang.pick("清除粘贴胶囊", "Clear Capsule").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::InternalCommand {
                    command: InternalCommandKind::ClearAttachedContext,
                },
            }],
            preview: None,
        });

        if !f.is_empty() {
            items.retain(|it| {
                it.title.to_lowercase().contains(&f) || it.subtitle.to_lowercase().contains(&f)
            });
        }
        items
    }

    fn build_diagnostics_items(&self, cmd: &str) -> Vec<ResultItem> {
        let stats = current_process_stats();
        let ev_status = match tomoyo_everything::detect_running() {
            Some(tomoyo_everything::EverythingVersion::V15Alpha) => "Everything 1.5a LIVE",
            Some(tomoyo_everything::EverythingVersion::V14) => "Everything 1.4 LIVE",
            None => "Everything Offline",
        };

        let plugins = self
            .plugin_bridge
            .as_ref()
            .map(|b| b.summary_lines())
            .unwrap_or_default();
        let active_plugins = plugins.iter().filter(|p| p.running).count();
        let plugin_ram_mb: f64 = plugins
            .iter()
            .filter_map(|p| p.working_set_bytes)
            .sum::<u64>() as f64
            / (1024.0 * 1024.0);

        let cur_theme = self
            .config
            .read()
            .map(|c| c.theme.clone())
            .unwrap_or_else(|_| "dark".into());
        let cur_hotkey = self
            .config
            .read()
            .map(|c| c.hotkey.clone())
            .unwrap_or_else(|_| "Alt+Space".into());

        let lang = self
            .config
            .read()
            .map(|c| c.lang())
            .unwrap_or(tomoyo_core::Lang::Zh);

        let mut items = Vec::new();

        if cmd == ":stats" || cmd == ":mem" || cmd == ":" || ":stats".starts_with(cmd) {
            let summary = format!(
                "WS: {:.1} MB · Private: {:.1} MB · GDI: {} · USER: {} · 句柄: {}",
                stats.working_set_mb(),
                stats.private_mb(),
                stats.gdi_objects,
                stats.user_objects,
                stats.handle_count
            );
            items.push(ResultItem {
                id: "diag:stats".into(),
                provider_id: self.id().into(),
                title: format!(":stats — {summary}"),
                subtitle: match lang {
                    tomoyo_core::Lang::Zh => format!(
                        "运行 {}s · 休眠整理 {} 次 · {} · 活跃插件 {}/{} ({:.1} MB)",
                        stats.uptime_secs,
                        stats.hibernation_cycles,
                        ev_status,
                        active_plugins,
                        plugins.len(),
                        plugin_ram_mb
                    ),
                    tomoyo_core::Lang::En => format!(
                        "Uptime {}s · Hibernations: {} · {} · Plugins: {}/{} active ({:.1} MB)",
                        stats.uptime_secs,
                        stats.hibernation_cycles,
                        ev_status,
                        active_plugins,
                        plugins.len(),
                        plugin_ram_mb
                    ),
                },
                category: ResultCategory::System,
                score: 10_000,
                icon: IconSpec::Monogram("MEM".into()),
                badge: Some(format!("{:.1} MB", stats.working_set_mb())),
                copy_text: Some(summary),
                actions: vec![ActionSpec {
                    id: "refresh".into(),
                    label: lang.pick("刷新实时统计", "Refresh Live Stats").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::RefreshQuery,
                    },
                }],
                preview: Some(PreviewSpec::KeyValue {
                    title: lang.pick("Tomoyo 实时资源遥测", "Tomoyo Live Telemetry").into(),
                    entries: vec![
                        (lang.pick("工作集 WS", "Working Set").into(), format!("{:.2} MB", stats.working_set_mb())),
                        (lang.pick("私有内存", "Private Commit").into(), format!("{:.2} MB", stats.private_mb())),
                        ("GDI".into(), stats.gdi_objects.to_string()),
                        ("USER".into(), stats.user_objects.to_string()),
                        (lang.pick("内核句柄", "Kernel Handles").into(), stats.handle_count.to_string()),
                        (lang.pick("休眠次数", "Hibernations").into(), stats.hibernation_cycles.to_string()),
                        ("Everything".into(), ev_status.into()),
                        (lang.pick("当前主题", "Active Theme").into(), cur_theme.clone()),
                        (lang.pick("全局热键", "Global Hotkey").into(), cur_hotkey.clone()),
                    ],
                }),
            });
        }

        if cmd == ":lang" || cmd.starts_with(":lang ") {
            let arg = cmd.strip_prefix(":lang").unwrap_or("").trim();
            for (code, label) in [("zh", "简体中文 (Chinese)"), ("en", "English (英文)")] {
                if arg.is_empty() || code.starts_with(arg) {
                    items.push(ResultItem {
                        id: format!("diag:lang:{code}"),
                        provider_id: self.id().into(),
                        title: format!(":lang {code} — {label}"),
                        subtitle: lang.pick("回车立即切换界面语言并保存", "Enter to switch UI language immediately").into(),
                        category: ResultCategory::System,
                        score: 9_920,
                        icon: IconSpec::Monogram(code.to_uppercase()),
                        badge: Some(if lang.code() == code { "ACTIVE" } else { "LANG" }.into()),
                        copy_text: None,
                        actions: vec![ActionSpec {
                            id: "set_lang".into(),
                            label: label.into(),
                            shortcut: Some("Enter".into()),
                            kind: ActionKind::InternalCommand {
                                command: InternalCommandKind::SetLanguage { lang: code.into() },
                            },
                        }],
                        preview: None,
                    });
                }
            }
        }

        if cmd == ":plugins" || cmd == ":" || ":plugins".starts_with(cmd) {
            if plugins.is_empty() && cmd == ":plugins" {
                items.push(ResultItem {
                    id: "diag:plugins:none".into(),
                    provider_id: self.id().into(),
                    title: "No external plugins discovered".into(),
                    subtitle: "Use `tomoyo-cli new` or drop folders into ./plugins".into(),
                    category: ResultCategory::Plugin,
                    score: 9_800,
                    icon: IconSpec::Monogram("EXT".into()),
                    badge: Some("0".into()),
                    copy_text: None,
                    actions: vec![],
                    preview: None,
                });
            } else if cmd == ":plugins" {
                for (idx, p) in plugins.iter().enumerate() {
                    let state = if p.running {
                        match p.working_set_bytes {
                            Some(b) => format!("LIVE · {:.1} MB", b as f64 / (1024.0 * 1024.0)),
                            None => "LIVE (In-Sandbox)".into(),
                        }
                    } else {
                        "SLEEPING (0 MB)".into()
                    };
                    let fill_kw = p
                        .keywords
                        .split(',')
                        .next()
                        .map(|k| format!("{} ", k.trim()))
                        .unwrap_or_default();
                    items.push(ResultItem {
                        id: format!("diag:plugin:{idx}:{}", p.id),
                        provider_id: self.id().into(),
                        title: format!("{}  [{}]", p.name, p.keywords),
                        subtitle: format!(
                            "Runtime: {} · {} · Cap: {} MB",
                            p.runtime_badge, state, p.max_memory_mb
                        ),
                        category: ResultCategory::Plugin,
                        score: 9_800 - idx as i64 * 10,
                        icon: IconSpec::Monogram(p.runtime_badge.chars().take(3).collect()),
                        badge: Some(p.runtime_badge.clone()),
                        copy_text: Some(fill_kw.clone()),
                        actions: vec![ActionSpec {
                            id: "try_plugin".into(),
                            label: "Trigger Plugin".into(),
                            shortcut: Some("Enter".into()),
                            kind: ActionKind::FillQuery { query: fill_kw },
                        }],
                        preview: Some(PreviewSpec::KeyValue {
                            title: p.name.clone(),
                            entries: vec![
                                ("Plugin ID".into(), p.id.clone()),
                                ("Runtime".into(), p.runtime_badge.clone()),
                                ("Triggers".into(), p.keywords.clone()),
                                ("Status".into(), state),
                                ("Memory Cap".into(), format!("{} MB", p.max_memory_mb)),
                            ],
                        }),
                    });
                }
            } else {
                items.push(ResultItem {
                    id: "diag:plugins:cmd".into(),
                    provider_id: self.id().into(),
                    title: format!(":plugins — Manage Extensions ({} installed)", plugins.len()),
                    subtitle: "Inspect STDIO, WASM sandbox, and on-demand WebView plugins".into(),
                    category: ResultCategory::Plugin,
                    score: 9_750,
                    icon: IconSpec::Monogram("EXT".into()),
                    badge: Some(format!("{}/{}", active_plugins, plugins.len())),
                    copy_text: None,
                    actions: vec![ActionSpec {
                        id: "open_plugins".into(),
                        label: "Inspect Plugins".into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::FillQuery {
                            query: ":plugins".into(),
                        },
                    }],
                    preview: None,
                });
            }
        }

        // Theme live switcher: `:theme`, `:theme dark`, `:theme light`, `:theme midnight`
        if cmd.starts_with(":theme") || cmd == ":" || ":theme".starts_with(cmd) {
            let arg = cmd.strip_prefix(":theme").unwrap_or("").trim();
            if cmd.starts_with(":theme") {
                for (name, desc, score) in [
                    ("dark", "Fluent Obsidian Dark (default)", 9_700),
                    ("light", "Fluent Crisp Daylight", 9_680),
                    ("midnight", "Deep Indigo Midnight", 9_660),
                ] {
                    if arg.is_empty() || name.starts_with(arg) {
                        let active = cur_theme.eq_ignore_ascii_case(name);
                        items.push(ResultItem {
                            id: format!("diag:theme:{name}"),
                            provider_id: self.id().into(),
                            title: format!(":theme {name} — {desc}"),
                            subtitle: if active {
                                "Currently active theme".into()
                            } else {
                                "Enter to apply immediately & save to config.toml".into()
                            },
                            category: ResultCategory::System,
                            score,
                            icon: IconSpec::Monogram("THM".into()),
                            badge: Some(if active { "ACTIVE" } else { "THEME" }.into()),
                            copy_text: None,
                            actions: vec![ActionSpec {
                                id: "set_theme".into(),
                                label: format!("Switch to `{name}` theme"),
                                shortcut: Some("Enter".into()),
                                kind: ActionKind::InternalCommand {
                                    command: InternalCommandKind::SetTheme {
                                        theme: name.into(),
                                    },
                                },
                            }],
                            preview: None,
                        });
                    }
                }
            } else {
                items.push(ResultItem {
                    id: "diag:theme:hint".into(),
                    provider_id: self.id().into(),
                    title: format!(":theme — Switch Theme (current: {cur_theme})"),
                    subtitle: "Live switch between dark, light, and midnight presets".into(),
                    category: ResultCategory::System,
                    score: 9_680,
                    icon: IconSpec::Monogram("THM".into()),
                    badge: Some(cur_theme.to_uppercase()),
                    copy_text: None,
                    actions: vec![ActionSpec {
                        id: "fill_theme".into(),
                        label: "Choose Theme".into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::FillQuery {
                            query: ":theme ".into(),
                        },
                    }],
                    preview: None,
                });
            }
        }

        if cmd == ":update" || cmd == ":" || ":update".starts_with(cmd) {
            items.push(self.build_update_item(lang));
        }

        if cmd == ":settings" || cmd == ":" || ":settings".starts_with(cmd) {
            items.push(ResultItem {
                id: "diag:settings".into(),
                provider_id: self.id().into(),
                title: ":settings — Open Graphical Settings Center 设置中心".into(),
                subtitle: "Configure Hotkey, Modern White / Dark Themes, Window Size, Everything & Web Search (0 idle RAM)".into(),
                category: ResultCategory::System,
                score: 9_950,
                icon: IconSpec::Monogram("SET".into()),
                badge: Some("UI".into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "open_settings".into(),
                    label: "Open Settings Center".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::OpenSettingsWindow,
                    },
                }],
                preview: None,
            });
        }

        if cmd == ":config" || cmd == ":" || ":config".starts_with(cmd) {
            let cfg_path = TomoyoConfig::config_path().to_string_lossy().into_owned();
            items.push(ResultItem {
                id: "diag:config".into(),
                provider_id: self.id().into(),
                title: ":config — Open config.toml".into(),
                subtitle: format!("Edit hotkey, theme, search engines ({cfg_path})"),
                category: ResultCategory::System,
                score: 9_640,
                icon: IconSpec::Monogram("CFG".into()),
                badge: Some("TOML".into()),
                copy_text: Some(cfg_path.clone()),
                actions: vec![ActionSpec {
                    id: "open_cfg".into(),
                    label: "Open config.toml".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::OpenConfigFile,
                    },
                }],
                preview: Some(PreviewSpec::FilePreview { path: cfg_path }),
            });
        }

        if cmd == ":autostart" || cmd == ":" || ":autostart".starts_with(cmd) {
            let enabled = is_autostart_enabled();
            items.push(ResultItem {
                id: "diag:autostart".into(),
                provider_id: self.id().into(),
                title: format!(
                    ":autostart — Launch at Windows Login ({})",
                    if enabled { "Enabled" } else { "Disabled" }
                ),
                subtitle: "Toggles HKCU\\...\\Run\\Tomoyo (--silent background start)".into(),
                category: ResultCategory::System,
                score: 9_620,
                icon: IconSpec::Monogram("RUN".into()),
                badge: Some(if enabled { "ON" } else { "OFF" }.into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "toggle_autostart".into(),
                    label: if enabled {
                        "Disable Launch at Login".into()
                    } else {
                        "Enable Launch at Login".into()
                    },
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::ToggleAutostart,
                    },
                }],
                preview: None,
            });
        }

        if cmd == ":preview" || ":preview".starts_with(cmd) && cmd.len() > 1 {
            items.push(ResultItem {
                id: "diag:preview".into(),
                provider_id: self.id().into(),
                title: ":preview — Toggle Right-Side Live Preview Pane (Ctrl+P)".into(),
                subtitle: "Show or hide bounded file, clipboard, and plugin preview cards".into(),
                category: ResultCategory::System,
                score: 9_610,
                icon: IconSpec::Monogram("PRV".into()),
                badge: Some("Ctrl+P".into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "toggle_preview".into(),
                    label: "Toggle Preview Pane".into(),
                    shortcut: Some("Ctrl+P".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::TogglePreviewPane,
                    },
                }],
                preview: None,
            });
        }

        if cmd == ":gc" || cmd == ":" || ":gc".starts_with(cmd) {
            items.push(ResultItem {
                id: "diag:gc".into(),
                provider_id: self.id().into(),
                title: ":gc — Hibernate & Trim Working Set Now".into(),
                subtitle: "Release DIB surfaces, shrink SQLite & icon caches, reap idle plugins"
                    .into(),
                category: ResultCategory::System,
                score: 9_600,
                icon: IconSpec::Monogram("GC".into()),
                badge: Some("ANTI-LEAK".into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "gc".into(),
                    label: "Run Memory Trim".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::TriggerHibernateGc,
                    },
                }],
                preview: None,
            });
        }

        if cmd == ":reload" || cmd == ":" || ":reload".starts_with(cmd) {
            items.push(ResultItem {
                id: "diag:reload".into(),
                provider_id: self.id().into(),
                title: ":reload — Hot-Reload config.toml, Apps & Plugins".into(),
                subtitle: "Re-read configuration, Start Menu apps, and all STDIO/WASM/WebView manifests".into(),
                category: ResultCategory::System,
                score: 9_500,
                icon: IconSpec::Monogram("RE".into()),
                badge: Some("INDEX".into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "reload".into(),
                    label: "Reload All".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::ReloadAll,
                    },
                }],
                preview: None,
            });
        }

        if cmd == ":quit" || cmd == ":exit" || cmd == ":" || ":quit".starts_with(cmd) {
            items.push(ResultItem {
                id: "diag:quit".into(),
                provider_id: self.id().into(),
                title: ":quit — Exit Tomoyo Completely".into(),
                subtitle: "Gracefully stop all plugin Job Objects & WASM sandboxes and exit".into(),
                category: ResultCategory::System,
                score: 9_000,
                icon: IconSpec::Monogram("BYE".into()),
                badge: Some("EXIT".into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "quit".into(),
                    label: "Quit Tomoyo".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::QuitApp,
                    },
                }],
                preview: None,
            });
        }

        items
    }
}

impl Provider for SystemProvider {
    fn id(&self) -> &'static str {
        "builtin.system"
    }

    fn name(&self) -> &'static str {
        "System Commands & Diagnostics"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &[":"]
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let lang = self
            .config
            .read()
            .map(|c| c.lang())
            .unwrap_or(Lang::Zh);

        if let Some(attached) = &ctx.attached {
            return self.build_super_match_items(attached, &ctx.trimmed, lang);
        }

        let trimmed = ctx.trimmed.as_str();
        if trimmed.starts_with(':') {
            return self.build_diagnostics_items(&trimmed.to_lowercase());
        }
        if trimmed.is_empty() {
            return Vec::new();
        }

        self.commands
            .iter()
            .filter_map(|cmd| {
                let s = score_match(trimmed, &cmd.indexed)?;
                Some(ResultItem {
                    id: cmd.id.into(),
                    provider_id: self.id().into(),
                    title: cmd.title.into(),
                    subtitle: cmd.subtitle.into(),
                    category: ResultCategory::System,
                    score: s - 200,
                    icon: IconSpec::Monogram(cmd.badge.into()),
                    badge: Some(cmd.badge.into()),
                    copy_text: None,
                    actions: vec![ActionSpec {
                        id: "exec".into(),
                        label: cmd.title.into(),
                        shortcut: Some("Enter".into()),
                        kind: cmd.kind.clone(),
                    }],
                    preview: None,
                })
            })
            .collect()
    }
}
