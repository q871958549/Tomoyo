mod scanner;
mod uwp_reg;
mod watcher;

use scanner::{app_watch_paths, scan_dir_roots, ScannedApp};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tomoyo_core::{
    score_match_with_alias, AppsConfig, FrecencyStore, IndexedText, Lang, Provider, TomoyoConfig,
};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, QueryContext, ResultCategory, ResultItem, SearchFilter,
};
use uwp_reg::{scan_apps_folder, scan_registry_app_paths};
use watcher::AppDirWatcher;

#[derive(Debug, Clone)]
pub(super) struct AppEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub launch_args: Option<String>,
    pub subtitle: String,
    pub is_uwp: bool,
    pub indexed: IndexedText,
    pub alias_indexed: Option<IndexedText>,
}

/// Fast in-memory installed application index supporting Start Menu, Desktop & Desktop
/// Organizers (DeskBox, etc.), custom portable tool folders, Registry App Paths, and
/// Windows `shell:AppsFolder` (Win32 + UWP/MSIX), with Pinyin & target-name matching.
pub struct AppsProvider {
    entries: Arc<RwLock<Vec<AppEntry>>>,
    config: Arc<RwLock<TomoyoConfig>>,
    frecency: Option<Arc<RwLock<FrecencyStore>>>,
    _watcher: Option<AppDirWatcher>,
}

impl AppsProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        Self::create(config, None)
    }

    pub fn with_frecency(
        config: Arc<RwLock<TomoyoConfig>>,
        frecency: Arc<RwLock<FrecencyStore>>,
    ) -> Self {
        Self::create(config, Some(frecency))
    }

    fn create(
        config: Arc<RwLock<TomoyoConfig>>,
        frecency: Option<Arc<RwLock<FrecencyStore>>>,
    ) -> Self {
        let entries = Arc::new(RwLock::new(Vec::new()));
        let apps_cfg = config.read().map(|c| c.apps.clone()).unwrap_or_default();
        reload_entries(&entries, &apps_cfg);

        let watch_dirs = app_watch_paths(&apps_cfg);
        let w_entries = Arc::clone(&entries);
        let w_cfg = Arc::clone(&config);
        let watcher = AppDirWatcher::start(watch_dirs, move || {
            let cfg = w_cfg.read().map(|c| c.apps.clone()).unwrap_or_default();
            tracing::info!("Detected app shortcut folder change; incrementally refreshing index");
            reload_entries(&w_entries, &cfg);
        });

        Self {
            entries,
            config,
            frecency,
            _watcher: watcher,
        }
    }

    pub fn app_count(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }

    fn apps_config(&self) -> AppsConfig {
        self.config
            .read()
            .map(|c| c.apps.clone())
            .unwrap_or_default()
    }
}

impl Provider for AppsProvider {
    fn id(&self) -> &'static str {
        "builtin.apps"
    }

    fn name(&self) -> &'static str {
        "Installed Applications"
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let q = ctx.trimmed.as_str();
        let lang = self.lang();
        let cap = if ctx.filter == SearchFilter::App {
            ctx.limit.min(25)
        } else {
            ctx.limit.min(12)
        };
        let Ok(entries) = self.entries.read() else {
            return Vec::new();
        };

        if q.is_empty() {
            let empty_cap = if ctx.filter == SearchFilter::App { cap } else { cap.min(8) };
            let mut ranked_empty: Vec<(&AppEntry, i64)> = entries
                .iter()
                .map(|app| {
                    let bonus = self
                        .frecency
                        .as_ref()
                        .and_then(|f| f.read().ok())
                        .map(|s| s.bonus(&app.id))
                        .unwrap_or(0);
                    (app, 500 + bonus)
                })
                .collect();
            ranked_empty.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
            return ranked_empty
                .into_iter()
                .take(empty_cap)
                .map(|(app, sc)| {
                    let mut item = to_result_item(app, sc, lang);
                    if sc > 500 && item.badge.is_none() {
                        item.badge = Some(lang.pick("常用", "FREQUENT").into());
                    }
                    item
                })
                .collect();
        }

        let mut matched: Vec<ResultItem> = entries
            .iter()
            .filter_map(|app| {
                let s = score_match_with_alias(q, &app.indexed, app.alias_indexed.as_ref())?;
                Some(to_result_item(app, s, lang))
            })
            .collect();

        matched.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
        matched.truncate(cap);
        matched
    }

    fn reload(&self) {
        let apps_cfg = self.apps_config();
        reload_entries(&self.entries, &apps_cfg);
    }

    fn trim_memory(&self) {
        if let Ok(mut guard) = self.entries.write() {
            guard.shrink_to_fit();
        }
    }
}

fn reload_entries(entries: &Arc<RwLock<Vec<AppEntry>>>, apps_cfg: &AppsConfig) {
    let scanned = scan_all_apps(apps_cfg);
    tracing::info!(
        "Indexed {} applications (Desktop/Organizers + StartMenu + ExtraDirs + Registry + AppsFolder)",
        scanned.len()
    );
    if let Ok(mut guard) = entries.write() {
        *guard = scanned;
        guard.shrink_to_fit();
    }
}

fn scan_all_apps(apps_cfg: &AppsConfig) -> Vec<AppEntry> {
    let mut raw_entries: Vec<ScannedApp> = Vec::with_capacity(400);
    let mut seen_names: HashSet<String> = HashSet::with_capacity(400);

    scan_dir_roots(apps_cfg, &mut seen_names, &mut raw_entries);
    scan_registry_app_paths(&mut seen_names, &mut raw_entries);
    scan_apps_folder(&mut seen_names, &mut raw_entries);

    for (name, exe, desc) in [
        ("记事本 Notepad", "notepad.exe", "Windows 记事本"),
        ("计算器 Calculator", "calc.exe", "Windows 计算器"),
        ("命令提示符 CMD", "cmd.exe", "Windows 命令提示符"),
        ("PowerShell 终端", "powershell.exe", "Windows PowerShell"),
        ("画图 Paint", "mspaint.exe", "Windows 画图"),
        ("控制面板 Control Panel", "control.exe", "Windows 控制面板"),
        ("资源管理器 Explorer", "explorer.exe", "Windows 文件资源管理器"),
        ("注册表编辑器 Regedit", "regedit.exe", "Windows 注册表编辑器"),
    ] {
        let key = name.to_lowercase();
        if seen_names.insert(key) {
            raw_entries.push(ScannedApp {
                id: format!("app:sys:{exe}"),
                name: name.to_string(),
                path: exe.to_string(),
                launch_args: None,
                subtitle: desc.to_string(),
                is_uwp: false,
                alias_text: None,
            });
        }
    }

    let mut entries: Vec<AppEntry> = raw_entries
        .into_iter()
        .map(|s| AppEntry {
            id: s.id,
            indexed: IndexedText::new(&s.name),
            alias_indexed: s.alias_text.as_deref().map(IndexedText::new),
            name: s.name,
            path: s.path,
            launch_args: s.launch_args,
            subtitle: s.subtitle,
            is_uwp: s.is_uwp,
        })
        .collect();

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn to_result_item(app: &AppEntry, score: i64, lang: Lang) -> ResultItem {
    let parent = if app.is_uwp {
        String::new()
    } else {
        Path::new(&app.path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };

    let mut actions = vec![ActionSpec {
        id: "launch".into(),
        label: lang.pick("启动应用", "Launch Application").into(),
        shortcut: Some("Enter".into()),
        kind: ActionKind::LaunchPath {
            path: app.path.clone(),
            args: app.launch_args.clone(),
            working_dir: if parent.is_empty() { None } else { Some(parent) },
        },
    }];

    if !app.is_uwp {
        actions.push(ActionSpec {
            id: "reveal".into(),
            label: lang.pick("打开文件位置", "Open File Location").into(),
            shortcut: Some("Ctrl+Enter".into()),
            kind: ActionKind::RevealInExplorer {
                path: app.path.clone(),
            },
        });
        actions.push(ActionSpec {
            id: "copy_path".into(),
            label: lang.pick("复制路径", "Copy Path").into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::CopyText {
                text: app.path.clone(),
                paste_immediately: false,
            },
        });
    }

    let icon = if app.is_uwp {
        let mono: String = app
            .name
            .chars()
            .filter(|c| !c.is_whitespace())
            .take(2)
            .collect();
        IconSpec::Monogram(if mono.is_empty() { "APP".into() } else { mono })
    } else {
        IconSpec::ShellPath(app.path.clone())
    };

    ResultItem {
        id: app.id.clone(),
        provider_id: "builtin.apps".into(),
        title: app.name.clone(),
        subtitle: app.subtitle.clone(),
        category: ResultCategory::App,
        score,
        icon,
        badge: if app.is_uwp { Some("UWP".into()) } else { None },
        copy_text: Some(app.path.clone()),
        actions,
        preview: None,
    }
}

pub(super) fn is_noisy_shortcut(lower: &str) -> bool {
    const NOISE: [&str; 10] = [
        "uninstall",
        "卸载",
        "readme",
        "help",
        "manual",
        "license",
        "documentation",
        "release notes",
        "website",
        "访问主页",
    ];
    NOISE.iter().any(|n| lower.contains(n))
}
