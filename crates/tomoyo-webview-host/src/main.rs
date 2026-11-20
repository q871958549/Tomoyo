#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Ephemeral On-Demand WebView2 Host (`tomoyo-webview-host.exe`).
//!
//! Zero-idle-RAM architecture: spawned on demand for WebView plugins or the Settings Center,
//! terminated completely on close (`Esc`) so 100% of `msedgewebview2.exe` memory is released.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod bridge;
mod db;
mod shim_js;
#[cfg(windows)]
mod single_instance;
mod store;
#[cfg(windows)]
mod win_host;
#[cfg(windows)]
mod win_workers;

use bridge::handle_host_message;
use db::PluginDb;
use shim_js::build_init_script;
use store::fetch_store_catalog_json;
use tomoyo_core::{
    is_autostart_enabled, locate_fonts_dir, PrivateFonts, TomoyoConfig, CURRENT_VERSION,
};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};

const SETTINGS_HTML_TPL: &str = include_str!("ui/settings.html");
const SETTINGS_CSS: &str = include_str!("ui/settings.css");
const SETTINGS_I18N_JS: &str = include_str!("ui/i18n.js");
const SETTINGS_STORE_JS: &str = include_str!("ui/store_updater.js");
const SETTINGS_APP_JS: &str = include_str!("ui/settings.js");

pub fn assemble_settings_html() -> String {
    SETTINGS_HTML_TPL
        .replace("/*__SETTINGS_CSS__*/", SETTINGS_CSS)
        .replace("/*__SETTINGS_I18N__*/", SETTINGS_I18N_JS)
        .replace("/*__SETTINGS_STORE_JS__*/", SETTINGS_STORE_JS)
        .replace("/*__SETTINGS_JS__*/", SETTINGS_APP_JS)
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
    pub settings_mode: bool,
    pub notify_hwnd: Option<isize>,
    pub self_test: bool,
}

fn parse_args() -> LaunchArgs {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let val = |flag: &str| -> Option<String> {
        args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
    };
    let settings_mode = args.iter().any(|a| a == "--settings");
    let cfg_theme = if settings_mode {
        TomoyoConfig::load_or_create().theme
    } else if args.iter().any(|a| a == "--light") {
        "light".into()
    } else {
        "dark".into()
    };
    let theme = val("--theme").unwrap_or(cfg_theme);

    LaunchArgs {
        plugin_id: val("--plugin-id").unwrap_or_else(|| "dev.tomoyo.webview".into()),
        title: val("--title").unwrap_or_else(|| {
            if settings_mode {
                "Tomoyo 设置中心".into()
            } else {
                "Tomoyo 插件".into()
            }
        }),
        entry: val("--entry").unwrap_or_default(),
        query: val("--query").unwrap_or_default(),
        width: val("--width")
            .and_then(|s| s.parse().ok())
            .unwrap_or(if settings_mode { 960 } else { 860 }),
        height: val("--height")
            .and_then(|s| s.parse().ok())
            .unwrap_or(if settings_mode { 670 } else { 600 }),
        utools_compat: args.iter().any(|a| a == "--utools-compat"),
        theme,
        settings_mode,
        notify_hwnd: val("--notify-hwnd").and_then(|s| s.parse().ok()),
        self_test: args.iter().any(|a| a == "--self-test"),
    }
}

pub fn build_settings_init_script() -> String {
    let cfg = TomoyoConfig::load_or_create();
    let autostart = is_autostart_enabled();
    let host = PluginHost::new(default_plugin_dirs());
    let plugins: Vec<serde_json::Value> = host
        .list_plugins()
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "runtime": p.runtime_badge,
                "keywords": p.keywords,
                "max_memory_mb": p.max_memory_mb,
                "declared_permissions": p.declared_permissions,
                "granted_permissions": p.granted_permissions,
                "bundled_trusted": p.bundled_trusted,
            })
        })
        .collect();
    host.shutdown_all();

    let plugins_dir = tomoyo_core::data_dir().join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    let payload = serde_json::json!({
        "version": CURRENT_VERSION,
        "config": cfg,
        "autostart": autostart,
        "plugins": plugins,
        "config_path": TomoyoConfig::config_path().to_string_lossy(),
        "plugins_dir": plugins_dir.to_string_lossy(),
        "has_bundled_alibaba_font": locate_fonts_dir().is_some(),
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
        assert!(html.contains("AlibabaPuHuiTi-3-55-Regular.ttf"));
        assert!(html.contains("hotkey-recorder"));
        assert!(html.contains("apps_dirs"));
        assert!(html.contains("show_filter_bar"));
        assert!(html.contains("tab-store"));
        assert!(html.contains("tab-updates"));
        assert!(pdb.get("demo").is_some());
        assert!(handle_host_message(r#"{"cmd":"close"}"#, &mut pdb, None).should_close);
        assert_eq!(
            handle_host_message(r#"{"cmd":"sync_theme","theme":"light"}"#, &mut pdb, None)
                .apply_theme
                .as_deref(),
            Some("light")
        );
        assert!(fetch_store_catalog_json(&["registry/index.json".into()]).contains("dev.tomoyo.example-wasm"));
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

    #[cfg(windows)]
    win_host::run_windows_host(opts);
}
