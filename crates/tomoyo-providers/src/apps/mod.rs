mod custom_dirs;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod organizers;
mod scanner;
#[cfg(not(target_os = "linux"))]
mod uwp_reg;
mod watcher;

#[cfg(not(target_os = "linux"))]
use scanner::scan_dir_roots;
use scanner::{app_watch_paths, ScannedApp};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tomoyo_core::{
    score_match_with_alias, AppsConfig, FrecencyStore, IndexedText, Lang, Provider, TomoyoConfig,
};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, PreviewSpec, QueryContext, ResultCategory, ResultItem,
    SearchFilter,
};
#[cfg(not(target_os = "linux"))]
use uwp_reg::{push_windows_builtin_apps, scan_apps_folder, scan_registry_app_paths, scan_registry_uninstall};
use watcher::AppDirWatcher;

#[derive(Debug, Clone)]
pub(super) struct AppEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub launch_args: Option<String>,
    pub subtitle: String,
    pub is_uwp: bool,
    pub is_folder: bool,
    pub is_file: bool,
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

    fn supports_file_filters(&self) -> bool {
        true
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let q = ctx.trimmed.as_str();
        let lang = self.lang();
        let cap = if ctx.filter != SearchFilter::All { ctx.limit.min(25) } else { ctx.limit.min(16) };
        let Ok(entries) = self.entries.read() else { return Vec::new() };
        let custom_exts = match ctx.filter {
            SearchFilter::Custom(idx) => self
                .config
                .read()
                .ok()
                .and_then(|c| c.custom_filters.get(idx as usize).map(|t| t.extensions.clone())),
            _ => None,
        };
        let allow_entry = |app: &AppEntry| {
            custom_dirs::entry_matches_filter(app, ctx.filter, custom_exts.as_deref())
        };

        if q.is_empty() {
            let empty_cap = if matches!(ctx.filter, SearchFilter::App | SearchFilter::Folder) { cap } else { cap.min(8) };
            let mut ranked_empty: Vec<(&AppEntry, i64)> = entries
                .iter()
                .filter(|app| {
                    allow_entry(app)
                        && (ctx.filter == SearchFilter::Folder || !app.is_folder)
                        && (ctx.filter.is_file_only() || !app.is_file)
                })
                .map(|app| {
                    let bonus = self.frecency.as_ref().and_then(|f| f.read().ok()).map(|s| s.bonus(&app.id)).unwrap_or(0);
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
            .filter(|app| allow_entry(app))
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
    let mut raw_entries: Vec<ScannedApp> = Vec::with_capacity(480);
    let mut seen_names: HashSet<String> = HashSet::with_capacity(480);
    let mut seen_targets: HashSet<String> = HashSet::with_capacity(480);

    #[cfg(target_os = "linux")]
    {
        linux::scan_linux_apps(apps_cfg, &mut seen_names, &mut seen_targets, &mut raw_entries);
    }
    #[cfg(not(target_os = "linux"))]
    {
        scan_dir_roots(apps_cfg, &mut seen_names, &mut seen_targets, &mut raw_entries);
        scan_registry_app_paths(&mut seen_names, &mut seen_targets, &mut raw_entries);
        scan_apps_folder(&mut seen_names, &mut seen_targets, &mut raw_entries);
        scan_registry_uninstall(&mut seen_names, &mut seen_targets, &mut raw_entries);
        push_windows_builtin_apps(&mut seen_names, &mut raw_entries);
    }
    custom_dirs::scan_custom_extra_dirs(apps_cfg, &mut seen_targets, &mut raw_entries);

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
            is_folder: s.is_folder,
            is_file: s.is_file,
        })
        .collect();

    entries.sort_by_cached_key(|a| a.name.to_lowercase());
    entries
}

fn to_result_item(app: &AppEntry, score: i64, lang: Lang) -> ResultItem {
    if app.is_file {
        return custom_dirs::custom_file_to_result_item(app, score, lang);
    }
    if app.is_folder {
        return ResultItem {
            id: app.id.clone(),
            provider_id: "builtin.apps".into(),
            title: app.name.clone(),
            subtitle: app.subtitle.clone(),
            category: ResultCategory::Folder,
            score,
            icon: IconSpec::ShellPath(app.path.clone()),
            badge: Some(lang.pick("文件夹", "Folder").into()),
            copy_text: Some(app.path.clone()),
            actions: vec![
                ActionSpec {
                    id: "open".into(),
                    label: lang.pick("打开文件夹", "Open Folder").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::LaunchPath {
                        path: app.path.clone(),
                        args: None,
                        working_dir: Some(app.path.clone()),
                    },
                },
                ActionSpec {
                    id: "reveal".into(),
                    label: lang.pick("在资源管理器中定位", "Reveal in Explorer").into(),
                    shortcut: Some("Ctrl+Enter".into()),
                    kind: ActionKind::RevealInExplorer { path: app.path.clone() },
                },
                ActionSpec {
                    id: "terminal".into(),
                    label: lang.pick("在此处打开终端", "Open Terminal Here").into(),
                    shortcut: Some("Shift+Enter".into()),
                    kind: ActionKind::OpenTerminalAt { dir: app.path.clone() },
                },
                ActionSpec {
                    id: "copy_path".into(),
                    label: lang.pick("复制完整路径", "Copy Full Path").into(),
                    shortcut: Some("Ctrl+C".into()),
                    kind: ActionKind::CopyText { text: app.path.clone(), paste_immediately: false },
                },
            ],
            preview: Some(PreviewSpec::FilePreview { path: app.path.clone() }),
        };
    }

    let parent = if app.is_uwp {
        String::new()
    } else {
        Path::new(&app.path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };

    let work_dir = if parent.is_empty() { None } else { Some(parent) };
    let mut actions = vec![ActionSpec {
        id: "launch".into(),
        label: lang.pick("启动应用", "Launch Application").into(),
        shortcut: Some("Enter".into()),
        kind: ActionKind::LaunchPath {
            path: app.path.clone(),
            args: app.launch_args.clone(),
            working_dir: work_dir.clone(),
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
        actions.push(ActionSpec {
            id: "run_admin".into(),
            label: lang.pick("以管理员身份运行", "Run as Administrator").into(),
            shortcut: Some("Ctrl+Shift+Enter".into()),
            kind: ActionKind::LaunchAsAdmin {
                path: app.path.clone(),
                args: app.launch_args.clone(),
                working_dir: work_dir,
            },
        });
    }

    let icon = if app.is_uwp {
        if let Some(aumid) = app.launch_args.as_deref().filter(|a| a.starts_with("shell:AppsFolder\\")) {
            IconSpec::ShellPath(aumid.to_string())
        } else {
            let mono: String = app.name.chars().filter(|c| !c.is_whitespace()).take(2).collect();
            IconSpec::Monogram(if mono.is_empty() { "APP".into() } else { mono })
        }
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

/// Word-boundary aware Start Menu noise filter: skips app uninstallers/docs/website links
/// without falsely dropping real utilities like `HiBit Uninstaller`, `Geek Uninstaller`,
/// `HelpDesk`, or `License Manager`.
pub(super) fn is_noisy_shortcut(lower: &str) -> bool {
    let s = lower.trim();
    if s.contains("卸载") || s.contains("访问主页") || s.contains("release notes") || s.contains("readme") {
        return true;
    }
    let words: Vec<&str> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.iter().any(|&w| matches!(w, "uninstall" | "documentation")) {
        return true;
    }
    matches!(
        words.last().copied(),
        Some("help" | "manual" | "manuals" | "documentation" | "website" | "license")
    )
}

#[cfg(test)]
mod tests {
    use super::is_noisy_shortcut;

    #[test]
    fn noise_filter_keeps_real_uninstaller_apps_and_drops_uninstall_links() {
        assert!(!is_noisy_shortcut("hibit uninstaller"));
        assert!(!is_noisy_shortcut("geek uninstaller"));
        assert!(!is_noisy_shortcut("revo uninstaller pro"));
        assert!(!is_noisy_shortcut("bcuninstaller"));
        assert!(!is_noisy_shortcut("helpdesk client"));
        assert!(!is_noisy_shortcut("license manager"));

        assert!(is_noisy_shortcut("uninstall node.js"));
        assert!(is_noisy_shortcut("xyplorer uninstall"));
        assert!(is_noisy_shortcut("卸载hibit uninstaller"));
        assert!(is_noisy_shortcut("卸载微信"));
        assert!(is_noisy_shortcut("7-zip help"));
        assert!(is_noisy_shortcut("git release notes"));
        assert!(is_noisy_shortcut("node.js documentation"));
    }

    #[test]
    #[ignore]
    fn live_desktop_apps_completeness_smoke() {
        use super::*;
        let cfg = Arc::new(RwLock::new(TomoyoConfig::load_or_create()));
        let provider = AppsProvider::new(cfg);
        eprintln!("TOTAL INDEXED APPS/DESKTOP ITEMS: {}", provider.app_count());
        for q in ["hibit", "1118", "rdp", "peco", "hatch", "vsc"] {
            let ctx = QueryContext::new(q, 1, 10);
            let hits = provider.query(&ctx);
            let names: Vec<String> = hits
                .iter()
                .map(|i| format!("{} [{:?}] (score={})", i.title, i.category, i.score))
                .collect();
            eprintln!("QUERY `{q}` => {names:?}");
        }
        let hibit = provider.query(&QueryContext::new("hibit", 1, 10));
        assert!(
            hibit.first().map(|i| i.title.to_lowercase().contains("hibit")).unwrap_or(false),
            "Expected HiBit Uninstaller first, got: {:?}",
            hibit.iter().map(|i| &i.title).collect::<Vec<_>>()
        );
        assert!(
            hibit.iter().all(|i| !i.title.contains("W168") && !i.title.contains("Regedit")),
            "Unexpected false positives for `hibit`"
        );
    }
}
