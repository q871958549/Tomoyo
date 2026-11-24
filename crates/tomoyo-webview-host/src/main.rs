#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Ephemeral On-Demand WebView2 Host (`tomoyo-webview-host.exe`).
//!
//! Zero-idle-RAM architecture: spawned on demand for WebView plugins or the Settings Center,
//! terminated completely on close (`Esc`) so 100% of `msedgewebview2.exe` memory is released.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod backup;
mod bridge;
mod bridge_ops;
mod bridge_os;
mod clip_view;
mod db;
#[cfg(windows)]
mod folder_picker;
#[cfg(windows)]
mod hotkey_capture;
#[cfg(windows)]
mod hotkey_doctor;
#[cfg(windows)]
mod hotkey_signatures;
#[cfg(windows)]
mod inline_dock;
#[cfg(target_os = "linux")]
mod linux_host;
#[cfg(target_os = "linux")]
mod linux_hotkeys;
#[cfg(target_os = "linux")]
mod linux_workers;
mod ocr_view;
mod shim_js;
#[cfg(windows)]
mod single_instance;
mod store;
mod store_cache;
mod store_index;
mod tr_view;
#[cfg(windows)]
mod win_host;
#[cfg(windows)]
mod win_msg;
#[cfg(windows)]
mod win_theme;
#[cfg(windows)]
mod win_workers;

use bridge::handle_host_message;
use db::PluginDb;
use shim_js::build_init_script;
use std::io::{self, Write};
use store::fetch_store_catalog_json;
use tomoyo_core::{
    count_recent_log_issues, format_beijing_now, init_process_diagnostics, is_autostart_enabled,
    locate_fonts_dir, logs_dir, write_process_log_bytes, PrivateFonts, TomoyoConfig,
    CURRENT_VERSION, DEFAULT_SUB_LOG_BYTES,
};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};

#[derive(Clone, Copy)]
struct WebviewLogMakeWriter;
struct WebviewLogSink;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for WebviewLogMakeWriter {
    type Writer = WebviewLogSink;
    fn make_writer(&'a self) -> Self::Writer {
        WebviewLogSink
    }
}

impl Write for WebviewLogSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write_process_log_bytes(buf, false)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct BeijingTimer;

impl tracing_subscriber::fmt::time::FormatTime for BeijingTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        w.write_str(&format_beijing_now())
    }
}

fn init_webview_logging() {
    init_process_diagnostics("webview-host", "webview", DEFAULT_SUB_LOG_BYTES, false);
    let _ = tracing_subscriber::fmt()
        .with_timer(BeijingTimer)
        .with_writer(WebviewLogMakeWriter)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
}

const SETTINGS_HTML_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/settings.html.gz"));

/// Inflates a compile-time gzip-compressed single-file Svelte 5 HTML bundle on demand (< 2ms).
pub fn inflate_embedded_html(gz: &[u8]) -> String {
    use std::io::Read;
    let mut dec = flate2::read::GzDecoder::new(gz);
    let mut out = String::with_capacity(gz.len() * 3);
    let _ = dec.read_to_string(&mut out);
    out
}

pub fn assemble_settings_html() -> String {
    inflate_embedded_html(SETTINGS_HTML_GZ)
}

pub struct LaunchArgs {
    pub plugin_id: String,
    pub title: String,
    pub entry: String,
    pub query: String,
    pub width: i32,
    pub height: i32,
    pub utools_compat: bool,
    pub theme: String,
    pub icon: Option<String>,
    pub settings_mode: bool,
    pub notify_hwnd: Option<isize>,
    pub inline_hwnd: Option<isize>,
    pub dock_rect: Option<(i32, i32, i32, i32)>,
    pub self_test: bool,
}

fn parse_dock_rect(raw: Option<String>) -> Option<(i32, i32, i32, i32)> {
    let s = raw?;
    let mut parts = s.split(',').filter_map(|p| p.trim().parse::<i32>().ok());
    Some((parts.next()?, parts.next()?, parts.next()?, parts.next()?))
}

fn parse_args() -> LaunchArgs {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let val = |flag: &str| -> Option<String> {
        args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
    };
    let settings_mode = args.iter().any(|a| a == "--settings");
    let entry = val("--entry").unwrap_or_default();
    let plugin_id = val("--plugin-id").unwrap_or_else(|| "dev.tomoyo.webview".into());
    let cfg_theme = if settings_mode
        || ocr_view::is_ocr_viewer_entry(&entry)
        || clip_view::is_clip_manager_entry(&entry)
        || tr_view::is_translator_entry(&entry)
    {
        TomoyoConfig::load_or_create().theme
    } else if args.iter().any(|a| a == "--light") {
        "light".into()
    } else {
        "dark".into()
    };
    let theme = val("--theme").unwrap_or(cfg_theme);
    let icon = val("--icon")
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            (!settings_mode)
                .then(|| tomoyo_core::resolve_bundled_module_icon(&plugin_id))
                .flatten()
                .map(|p| p.to_string_lossy().into_owned())
        });

    LaunchArgs {
        plugin_id,
        title: val("--title").unwrap_or_else(|| {
            if settings_mode {
                "Tomoyo 设置中心".into()
            } else {
                "Tomoyo 插件".into()
            }
        }),
        entry,
        query: val("--query").unwrap_or_default(),
        width: val("--width")
            .and_then(|s| s.parse().ok())
            .unwrap_or(if settings_mode { 960 } else { 860 }),
        height: val("--height")
            .and_then(|s| s.parse().ok())
            .unwrap_or(if settings_mode { 670 } else { 600 }),
        utools_compat: args.iter().any(|a| a == "--utools-compat"),
        theme,
        icon,
        settings_mode,
        notify_hwnd: val("--notify-hwnd").and_then(|s| s.parse().ok()),
        inline_hwnd: val("--inline-hwnd").and_then(|s| s.parse().ok()),
        dock_rect: parse_dock_rect(val("--dock-rect")),
        self_test: args.iter().any(|a| a == "--self-test"),
    }
}

pub fn build_settings_init_script() -> String {
    build_settings_init_script_for("")
}

pub fn build_settings_init_script_for(initial_route: &str) -> String {
    let cfg = TomoyoConfig::load_or_create();
    let autostart = is_autostart_enabled();
    let host = PluginHost::new(default_plugin_dirs());
    let catalog_map = store_index::local_catalog_index_map(&cfg.plugin_registries);
    let plugins: Vec<serde_json::Value> = host
        .list_plugins()
        .into_iter()
        .map(|p| {
            let icon_url = p
                .icon_path
                .as_deref()
                .and_then(|ip| tomoyo_core::read_icon_data_url(std::path::Path::new(ip)))
                .or_else(|| {
                    tomoyo_core::resolve_bundled_module_icon(&p.id)
                        .and_then(|ip| tomoyo_core::read_icon_data_url(&ip))
                });
            let cat_entry = catalog_map.get(&p.id.to_ascii_lowercase());
            let upd_avail = cat_entry.is_some_and(|e| store_cache::is_newer_semver(&e.version, &p.version));
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "version": p.version,
                "latest_version": cat_entry.map(|e| e.version.as_str()),
                "update_available": upd_avail,
                "update_source": cat_entry.map(|e| e.source.as_str()),
                "update_sha256": cat_entry.map(|e| e.sha256.as_str()),
                "description": p.description,
                "author": p.author,
                "license": p.license,
                "homepage": p.homepage,
                "category": p.category,
                "display_mode": p.display_mode,
                "icon_url": icon_url,
                "runtime": p.runtime_badge,
                "keywords": p.keywords,
                "max_memory_mb": p.max_memory_mb,
                "declared_permissions": p.declared_permissions,
                "granted_permissions": p.granted_permissions,
                "bundled_trusted": p.bundled_trusted,
                "declared_hotkeys": p.declared_hotkeys,
                "declared_events": p.declared_events,
                "settings_schema": p.settings_schema,
            })
        })
        .collect();
    host.shutdown_all();

    let mut official_icons = serde_json::Map::new();
    for mid in [
        tomoyo_core::PLUGIN_CLIPBOARD,
        tomoyo_core::PLUGIN_TRANSLATE,
        tomoyo_core::PLUGIN_CAPTURE,
        tomoyo_core::PLUGIN_OCR,
        tomoyo_core::PLUGIN_NOTES,
        tomoyo_core::PLUGIN_MD_EDITOR,
    ] {
        if let Some(url) = tomoyo_core::resolve_bundled_module_icon(mid)
            .and_then(|ip| tomoyo_core::read_icon_data_url(&ip))
        {
            official_icons.insert(mid.to_string(), serde_json::Value::String(url));
        }
    }
    let quiet_md_path = tomoyo_core::detect_quiet_md_binary(None)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    let plugins_dir = tomoyo_core::data_dir().join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    let (log_warns, log_errs, log_crash) = count_recent_log_issues();
    let cached_update = tomoyo_core::load_cached_update_status(&cfg.update.repo, CURRENT_VERSION);
    let payload = serde_json::json!({
        "version": CURRENT_VERSION,
        "platform": std::env::consts::OS,
        "config": cfg,
        "autostart": autostart,
        "plugins": plugins,
        "flows": tomoyo_core::load_or_init_flows(),
        "flows_path": tomoyo_core::flows_file_path().to_string_lossy(),
        "config_path": TomoyoConfig::config_path().to_string_lossy(),
        "sync_dir": tomoyo_core::configured_sync_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        "plugins_dir": plugins_dir.to_string_lossy(),
        "logs_dir": logs_dir().to_string_lossy(),
        "log_issues": { "warnings": log_warns, "errors": log_errs, "has_crash_report": log_crash },
        "running_apps": clip_view::enumerate_running_app_exes(),
        "secrets_status": tr_view::secrets_status_json(),
        "hotkey_health": tomoyo_core::load_hotkey_health(),
        "cached_update": cached_update,
        "has_bundled_alibaba_font": locate_fonts_dir().is_some(),
        "system_fonts": tomoyo_core::list_system_font_families(),
        "official_icons": official_icons,
        "quiet_md_path": quiet_md_path,
        "initial_route": initial_route,
    });
    format!(
        "window.__TOMOYO_SETTINGS_INIT__ = {};",
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
    )
}

fn main() {
    let _fonts = PrivateFonts::load_default();
    let opts = parse_args();
    if opts.self_test {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
        let tmp =
            std::env::temp_dir().join(format!("tomoyo_wv_selftest_{}.json", std::process::id()));
        let mut pdb = PluginDb::open_at(tmp.clone());
        let _ = handle_host_message(
            r#"{"cmd":"db_put","id":"demo","doc":{"_id":"demo","val":42}}"#,
            &mut pdb,
            None,
        );
        let script = build_init_script(
            &opts.plugin_id,
            &opts.query,
            opts.theme != "light",
            true,
            &pdb.snapshot_json(),
        );
        let settings_js = build_settings_init_script();
        let html = assemble_settings_html();
        assert!(script.contains("window.utools"));
        assert!(script.contains("setSubInput"));
        assert!(settings_js.contains("__TOMOYO_SETTINGS_INIT__"));
        assert!(settings_js.contains("hotkey_health"));
        assert!(html.contains("AlibabaPuHuiTi-3-55-Regular.ttf"));
        assert!(html.contains("__TOMOYO_SETTINGS_INIT__"));
        assert!(html.contains("check_hotkey_health"));
        assert!(html.contains("start_capture"));
        assert!(html.contains("save_settings"));
        assert!(!html.contains("type=\"number\""));
        assert!(!html.contains("<textarea"));
        assert!(ocr_view::assemble_ocr_viewer_html().contains("__TOMOYO_OCR_DATA__"));
        assert!(clip_view::assemble_clip_manager_html().contains("__TOMOYO_CLIP_INIT__"));
        assert!(tr_view::assemble_translator_html().contains("__TOMOYO_TR_INIT__"));
        assert!(handle_host_message(
            r#"{"cmd":"clip_list","query":""}"#,
            &mut pdb,
            None
        )
        .reply_json
        .is_some());
        assert!(handle_host_message(
            r#"{"cmd":"read_logs","source":"tomoyo.log","errors_only":false}"#,
            &mut pdb,
            None
        )
        .reply_json
        .is_some());
        assert!(pdb.get("demo").is_some());
        let mut dirty_cfg = TomoyoConfig::default();
        dirty_cfg.apps.extra_scan_depth = 12000;
        dirty_cfg.apps.extra_dirs = vec!["C:\\Windows".into(), "C:\\".into()];
        dirty_cfg.web_searches.push(tomoyo_core::WebSearchEngine {
            keyword: "bad".into(),
            name: "Bad".into(),
            url: "file:///C:/Windows/System32/cmd.exe?{q}".into(),
        });
        let notes = dirty_cfg.sanitize();
        assert!(!notes.is_empty());
        assert_eq!(dirty_cfg.apps.extra_scan_depth, 4);
        assert!(dirty_cfg.apps.extra_dirs.is_empty());
        assert!(dirty_cfg.web_searches.iter().all(|w| w.keyword != "bad"));
        assert!(handle_host_message(r#"{"cmd":"close"}"#, &mut pdb, None).should_close);
        assert_eq!(
            handle_host_message(r#"{"cmd":"sync_theme","theme":"light"}"#, &mut pdb, None)
                .apply_theme
                .as_deref(),
            Some("light")
        );
        assert!(fetch_store_catalog_json(&["registry/index.json".into()]).contains("dev.tomoyo.example-wasm"));
        #[cfg(windows)]
        {
            let hinst = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap() };
            let (big_ic, small_ic) = win_theme::load_embedded_window_icons(hinst);
            assert!(big_ic.is_some() && small_ic.is_some(), "Embedded app logo icon (ID 1) missing from tomoyo-webview-host.exe");
        }
        let _ = std::fs::remove_file(&tmp);
        println!(
            "[OK] tomoyo-webview-host v{} self-test passed (uTools bridge {}B, Settings UI {}B, Store & Updater verified, Alibaba font={})",
            CURRENT_VERSION,
            script.len(),
            html.len(),
            locate_fonts_dir().is_some()
        );
        return;
    }

    if !opts.settings_mode && opts.entry.is_empty() {
        return;
    }

    init_webview_logging();
    tracing::info!(
        settings = opts.settings_mode,
        plugin_id = %opts.plugin_id,
        entry = %opts.entry,
        "Starting tomoyo-webview-host v{}",
        CURRENT_VERSION
    );

    #[cfg(windows)]
    win_host::run_windows_host(opts);
    #[cfg(target_os = "linux")]
    linux_host::run_linux_host(opts);
}
