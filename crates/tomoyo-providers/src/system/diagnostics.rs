//! Built-in `:` diagnostics & administration commands (`:stats`, `:plugins`, `:theme`, `:lang`, `:settings`, `:gc`, `:reload`, `:update`, `:quit`).

use super::updates::build_update_item;
use std::sync::{Arc, RwLock};
use tomoyo_core::{
    current_process_stats, is_autostart_enabled, ExternalPluginBridge, Lang, TomoyoConfig,
    UpdateManager,
};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, PreviewSpec, ResultCategory, ResultItem,
};

pub fn build_diagnostics_items(
    provider_id: &str,
    cmd: &str,
    plugin_bridge: Option<&Arc<dyn ExternalPluginBridge>>,
    config: &Arc<RwLock<TomoyoConfig>>,
    updater: &UpdateManager,
) -> Vec<ResultItem> {
    let stats = current_process_stats();
    let ev_status = match tomoyo_everything::detect_running() {
        Some(tomoyo_everything::EverythingVersion::V15Alpha) => "Everything 1.5a LIVE",
        Some(tomoyo_everything::EverythingVersion::V14) => "Everything 1.4 LIVE",
        None => "Everything Offline",
    };

    let plugins = plugin_bridge.map(|b| b.summary_lines()).unwrap_or_default();
    let active_plugins = plugins.iter().filter(|p| p.running).count();
    let plugin_ram_mb: f64 = plugins
        .iter()
        .filter_map(|p| p.working_set_bytes)
        .sum::<u64>() as f64
        / (1024.0 * 1024.0);

    let (cur_theme, cur_hotkey, lang, update_repo) = config
        .read()
        .map(|c| {
            (
                c.theme.clone(),
                c.hotkey.clone(),
                c.lang(),
                c.update.repo.clone(),
            )
        })
        .unwrap_or_else(|_| {
            (
                "dark".into(),
                "Alt+Space".into(),
                Lang::Zh,
                "q871958549/Tomoyo".into(),
            )
        });

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
            provider_id: provider_id.into(),
            title: format!(":stats — {summary}"),
            subtitle: match lang {
                Lang::Zh => format!(
                    "运行 {}s · 休眠整理 {} 次 · {} · 活跃插件 {}/{} ({:.1} MB)",
                    stats.uptime_secs, stats.hibernation_cycles, ev_status, active_plugins, plugins.len(), plugin_ram_mb
                ),
                Lang::En => format!(
                    "Uptime {}s · Hibernations: {} · {} · Plugins: {}/{} active ({:.1} MB)",
                    stats.uptime_secs, stats.hibernation_cycles, ev_status, active_plugins, plugins.len(), plugin_ram_mb
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
                items.push(diag_action(
                    provider_id,
                    &format!("diag:lang:{code}"),
                    format!(":lang {code} — {label}"),
                    lang.pick("回车立即切换界面语言并保存", "Enter to switch UI language immediately").into(),
                    9_920,
                    &code.to_uppercase(),
                    if lang.code() == code { "ACTIVE" } else { "LANG" },
                    label,
                    InternalCommandKind::SetLanguage { lang: code.into() },
                ));
            }
        }
    }

    if cmd == ":plugins" || cmd == ":" || ":plugins".starts_with(cmd) {
        if cmd == ":plugins" {
            if plugins.is_empty() {
                items.push(ResultItem {
                    id: "diag:plugins:none".into(),
                    provider_id: provider_id.into(),
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
            } else {
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
                        provider_id: provider_id.into(),
                        title: format!("{}  [{}]", p.name, p.keywords),
                        subtitle: format!("Runtime: {} · {} · Cap: {} MB", p.runtime_badge, state, p.max_memory_mb),
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
            }
        } else {
            items.push(ResultItem {
                id: "diag:plugins:cmd".into(),
                provider_id: provider_id.into(),
                title: format!(":plugins — Manage Extensions ({} installed)", plugins.len()),
                subtitle: "Inspect STDIO, WASM sandbox, and on-demand WebView plugins".into(),
                category: ResultCategory::Plugin,
                score: 9_750,
                icon: IconSpec::Monogram("EXT".into()),
                badge: Some(format!("{active_plugins}/{}", plugins.len())),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "open_plugins".into(),
                    label: "Inspect Plugins".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::FillQuery { query: ":plugins".into() },
                }],
                preview: None,
            });
        }
    }

    if cmd.starts_with(":theme") || cmd == ":" || ":theme".starts_with(cmd) {
        let arg = cmd.strip_prefix(":theme").unwrap_or("").trim();
        if cmd.starts_with(":theme") {
            for (name, desc, score) in [
                ("dark", "Fluent Obsidian Dark (default)", 9_700),
                ("light", "Modern White Porcelain 现代白", 9_680),
                ("midnight", "Deep Indigo Midnight", 9_660),
            ] {
                if arg.is_empty() || name.starts_with(arg) {
                    let active = cur_theme.eq_ignore_ascii_case(name);
                    items.push(diag_action(
                        provider_id,
                        &format!("diag:theme:{name}"),
                        format!(":theme {name} — {desc}"),
                        if active { "Currently active theme" } else { "Enter to apply immediately & save" }.into(),
                        score,
                        "THM",
                        if active { "ACTIVE" } else { "THEME" },
                        &format!("Switch to `{name}` theme"),
                        InternalCommandKind::SetTheme { theme: name.into() },
                    ));
                }
            }
        } else {
            items.push(ResultItem {
                id: "diag:theme:hint".into(),
                provider_id: provider_id.into(),
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
                    kind: ActionKind::FillQuery { query: ":theme ".into() },
                }],
                preview: None,
            });
        }
    }

    if cmd == ":update" || cmd == ":" || ":update".starts_with(cmd) {
        items.push(build_update_item(provider_id, updater, &update_repo, lang));
    }

    if cmd == ":settings" || cmd == ":" || ":settings".starts_with(cmd) {
        items.push(diag_action(
            provider_id,
            "diag:settings",
            ":settings — Open Graphical Settings Center 设置中心".into(),
            "Configure Hotkey, Themes, Window Size, Plugin Store & Updates (0 idle RAM)".into(),
            9_950,
            "SET",
            "UI",
            "Open Settings Center",
            InternalCommandKind::OpenSettingsWindow,
        ));
    }

    if cmd == ":config" || cmd == ":" || ":config".starts_with(cmd) {
        let cfg_path = TomoyoConfig::config_path().to_string_lossy().into_owned();
        let mut it = diag_action(
            provider_id,
            "diag:config",
            ":config — Open config.toml".into(),
            format!("Edit hotkey, theme, search engines ({cfg_path})"),
            9_640,
            "CFG",
            "TOML",
            "Open config.toml",
            InternalCommandKind::OpenConfigFile,
        );
        it.copy_text = Some(cfg_path.clone());
        it.preview = Some(PreviewSpec::FilePreview { path: cfg_path });
        items.push(it);
    }

    if cmd == ":autostart" || cmd == ":" || ":autostart".starts_with(cmd) {
        let enabled = is_autostart_enabled();
        items.push(diag_action(
            provider_id,
            "diag:autostart",
            format!(":autostart — Launch at Windows Login ({})", if enabled { "Enabled" } else { "Disabled" }),
            "Toggles HKCU\\...\\Run\\Tomoyo (--silent background start)".into(),
            9_620,
            "RUN",
            if enabled { "ON" } else { "OFF" },
            if enabled { "Disable Launch at Login" } else { "Enable Launch at Login" },
            InternalCommandKind::ToggleAutostart,
        ));
    }

    if cmd == ":preview" || (":preview".starts_with(cmd) && cmd.len() > 1) {
        items.push(diag_action(
            provider_id,
            "diag:preview",
            ":preview — Toggle Right-Side Live Preview Pane (Ctrl+P)".into(),
            "Show or hide bounded file, clipboard, and plugin preview cards".into(),
            9_610,
            "PRV",
            "Ctrl+P",
            "Toggle Preview Pane",
            InternalCommandKind::TogglePreviewPane,
        ));
    }

    if cmd == ":gc" || cmd == ":" || ":gc".starts_with(cmd) {
        items.push(diag_action(
            provider_id,
            "diag:gc",
            ":gc — Hibernate & Trim Working Set Now".into(),
            "Release DIB surfaces, purge allocator pages, shrink SQLite & reap idle plugins".into(),
            9_600,
            "GC",
            "ANTI-LEAK",
            "Run Memory Trim",
            InternalCommandKind::TriggerHibernateGc,
        ));
    }

    if cmd == ":reload" || cmd == ":" || ":reload".starts_with(cmd) {
        items.push(diag_action(
            provider_id,
            "diag:reload",
            ":reload — Hot-Reload config.toml, Apps & Plugins".into(),
            "Re-read configuration, Start Menu apps, and all STDIO/WASM/WebView manifests".into(),
            9_500,
            "RE",
            "INDEX",
            "Reload All",
            InternalCommandKind::ReloadAll,
        ));
    }

    if cmd == ":quit" || cmd == ":exit" || cmd == ":" || ":quit".starts_with(cmd) {
        items.push(diag_action(
            provider_id,
            "diag:quit",
            ":quit — Exit Tomoyo Completely".into(),
            "Gracefully stop all plugin Job Objects & WASM sandboxes and exit".into(),
            9_000,
            "BYE",
            "EXIT",
            "Quit Tomoyo",
            InternalCommandKind::QuitApp,
        ));
    }

    items
}

#[allow(clippy::too_many_arguments)]
fn diag_action(
    provider_id: &str,
    id: &str,
    title: String,
    subtitle: String,
    score: i64,
    icon: &str,
    badge: &str,
    action_label: &str,
    command: InternalCommandKind,
) -> ResultItem {
    ResultItem {
        id: id.into(),
        provider_id: provider_id.into(),
        title,
        subtitle,
        category: ResultCategory::System,
        score,
        icon: IconSpec::Monogram(icon.into()),
        badge: Some(badge.into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: id.into(),
            label: action_label.into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::InternalCommand { command },
        }],
        preview: None,
    }
}
