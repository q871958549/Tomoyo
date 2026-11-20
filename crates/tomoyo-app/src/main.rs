#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tomoyo — 100% Open-Source, Ultra-Low-Memory Rust Command Launcher.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod actions;
mod brand;
mod drag;
mod icons;
mod popup_menu;
mod preview_loader;
mod renderer;
mod tray;
mod window;

use std::sync::{Arc, RwLock};
use tomoyo_core::{
    current_process_stats, data_dir, init_uptime_clock, ExternalPluginBridge, FrecencyStore,
    PrivateFonts, Provider, QueryRouter, TomoyoConfig, UpdateManager, CURRENT_VERSION,
};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};
use tomoyo_protocol::AttachedContext;
use tomoyo_providers::{
    AppsProvider, CalcProvider, ClipboardProvider, EverythingProvider, ProcessProvider,
    SystemProvider, WebSearchProvider, WindowsSwitcherProvider,
};
use window::AppShell;
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

struct OleGuard;
impl OleGuard {
    fn init() -> Self {
        unsafe {
            let _ = OleInitialize(None);
        }
        Self
    }
}
impl Drop for OleGuard {
    fn drop(&mut self) {
        unsafe {
            OleUninitialize();
        }
    }
}

struct SingletonMutex(HANDLE);
impl SingletonMutex {
    fn acquire() -> Option<Self> {
        unsafe {
            let handle = CreateMutexW(None, false, w!("Local\\TomoyoLauncherSingleton")).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return None;
            }
            Some(Self(handle))
        }
    }
}
impl Drop for SingletonMutex {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_invalid() {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn build_engine() -> (
    Arc<RwLock<TomoyoConfig>>,
    Arc<QueryRouter>,
    Arc<PluginHost>,
    Arc<ClipboardProvider>,
    Arc<AppsProvider>,
    Arc<UpdateManager>,
) {
    let loaded_cfg = TomoyoConfig::load_or_create();
    let clip_db_path = if loaded_cfg.clipboard.persist_sqlite {
        Some(data_dir().join("clipboard.db"))
    } else {
        None
    };
    let clip_max = loaded_cfg.clipboard.max_persisted_items;
    let config = Arc::new(RwLock::new(loaded_cfg));

    let plugin_dirs = default_plugin_dirs();
    let plugin_host = PluginHost::new(plugin_dirs);
    let bridge: Arc<dyn ExternalPluginBridge> = plugin_host.clone();
    let updater = UpdateManager::new();

    let apps = Arc::new(AppsProvider::new(config.clone()));
    let clipboard = Arc::new(ClipboardProvider::with_sqlite(
        clip_db_path,
        clip_max,
        config.clone(),
    ));
    let everything = Arc::new(EverythingProvider::new(config.clone()));
    let calc = Arc::new(CalcProvider::new());
    let web = Arc::new(WebSearchProvider::new(config.clone()));
    let windows = Arc::new(WindowsSwitcherProvider::new());
    let processes = Arc::new(ProcessProvider::new(config.clone()));
    let system = Arc::new(SystemProvider::new(
        Some(bridge.clone()),
        config.clone(),
        updater.clone(),
    ));

    let providers: Vec<Arc<dyn Provider>> = vec![
        apps.clone(),
        calc,
        web,
        everything,
        clipboard.clone(),
        windows,
        processes,
        system,
    ];

    let frecency_path = data_dir().join("frecency.json");
    let frecency = Arc::new(RwLock::new(FrecencyStore::load_or_default(frecency_path)));
    let router = Arc::new(QueryRouter::new(providers, Some(bridge), frecency));
    (config, router, plugin_host, clipboard, apps, updater)
}

fn log_mem(stage: &str) {
    let s = current_process_stats();
    println!(
        "  [MEM] {:<34} WS={:6.2} MB  Private={:6.2} MB  GDI={:<3} USER={:<3} Handles={}",
        stage,
        s.working_set_mb(),
        s.private_mb(),
        s.gdi_objects,
        s.user_objects,
        s.handle_count
    );
}

fn run_self_test(fonts: &PrivateFonts, strict_budget: bool) {
    println!("=== Tomoyo v{CURRENT_VERSION} Full-Stack Anti-Leak & Ecosystem Self-Test ===");
    println!(
        "Font engine: Regular=`{}` · Medium=`{}` (Alibaba PuHuiTi loaded={})",
        fonts.regular_family(),
        fonts.medium_family(),
        fonts.has_alibaba_regular()
    );
    log_mem("0. process startup");
    let (config, router, plugin_host, clipboard, apps, updater) = build_engine();
    log_mem("1. after build_engine (UWP+SQLite)");
    println!("Indexed installed apps (incl. UWP): {}", apps.app_count());
    println!("Clipboard SQLite persisted items: {}", clipboard.item_count());
    println!("Discovered plugins: {}", plugin_host.list_plugins().len());

    let mut shell = AppShell::create(
        config,
        router.clone(),
        plugin_host.clone(),
        clipboard,
        updater,
        fonts.regular_family(),
        fonts.medium_family(),
        false,
    )
    .expect("failed to create Win32 AppShell");
    log_mem("2. after AppShell::create");

    let _ = shell.self_test_paint_and_hibernate(":stats");
    log_mem("2a. SDF+AlibabaFont+Preview paint");
    let _ = shell.self_test_paint_and_hibernate("");
    log_mem("2b. after shell icon paint");

    let queries = [
        "jsq",
        "设置",
        "(1024*768)/16",
        "#7c6af7",
        "1700000000",
        "g rust tomoyo",
        "f Cargo.toml",
        "kill",
        "kill explorer",
        ":update",
        ":stats",
        ":plugins",
        "hi Tomoyo Rust",
        "py tomoyo",
        "unit 2.5gb",
        "json {\"ok\":true}",
    ];
    for q in queries {
        let hits = shell.self_test_paint_and_hibernate(q);
        println!("Query `{q}` -> {hits} hits");
    }

    // Exercise Super Match attached capsule (File SHA-256 + lazy Base64 PNG thumbnail preview)
    let super_hits = shell.self_test_attached_paint(AttachedContext::Files {
        paths: vec!["Cargo.toml".into()],
    });
    println!("Super Match [Files: Cargo.toml] -> {super_hits} hits");
    assert!((3..=6).contains(&super_hits), "Super Match should focus cleanly on file actions (got {super_hits})");

    if std::path::Path::new("assets/tomoyo-icon.png").is_file() {
        let img_hits = shell.self_test_attached_paint(AttachedContext::Image {
            path: "assets/tomoyo-icon.png".into(),
            width: 512,
            height: 512,
        });
        println!("Super Match [Image: tomoyo-icon.png] -> {img_hits} hits (PNG thumbnail painted)");
        assert!((3..=6).contains(&img_hits), "Super Match image should focus cleanly on image actions (got {img_hits})");
    }

    // Exercise zh/en i18n + all 3 themes live
    for lang in ["en", "zh"] {
        shell.apply_language(lang, false);
        for th in ["light", "midnight", "dark"] {
            shell.apply_theme(th, false);
            let _ = shell.self_test_paint_and_hibernate(":stats");
        }
    }

    let reaped = plugin_host.reap_plugins(true);
    println!("Reaped active STDIO + WASM plugins: {reaped}");
    shell.wait_background_idle_for_test();
    let _ = shell.run_hibernate();
    shell.wait_background_idle_for_test();
    assert!(
        shell.is_renderer_hibernated(),
        "renderer should release surfaces on hibernate"
    );
    let baseline = current_process_stats();
    log_mem("3. baseline (hibernated+mi_purge)");

    let soak_queries = ["jsq", "f Cargo.toml", "2^16", ":stats", "g rust", "cb", "@pdf"];
    for cycle in 0..100 {
        let q = soak_queries[cycle % soak_queries.len()];
        let _ = shell.self_test_paint_and_hibernate(q);
        shell.wait_background_idle_for_test();
        let _ = shell.run_hibernate();
        shell.wait_background_idle_for_test();
    }

    let after = current_process_stats();
    log_mem("4. after 100 soak cycles");
    let d_gdi = after.gdi_objects as i64 - baseline.gdi_objects as i64;
    let d_user = after.user_objects as i64 - baseline.user_objects as i64;
    let d_handles = after.handle_count as i64 - baseline.handle_count as i64;
    let d_priv = after.private_mb() - baseline.private_mb();
    println!(
        "Delta -> GDI: {d_gdi:+}, USER: {d_user:+}, Handles: {d_handles:+}, Private: {d_priv:+.2} MB"
    );

    if strict_budget {
        assert!(after.working_set_mb() <= 2.0, "Hibernated WS exceeded 2.0 MB budget: {:.2} MB", after.working_set_mb());
        assert!(after.private_mb() <= 28.0, "Hibernated Private Commit exceeded 28.0 MB budget: {:.2} MB", after.private_mb());
        assert_eq!(d_gdi, 0, "GDI handle leak detected across 100 soak cycles: {d_gdi:+}");
        assert_eq!(d_user, 0, "USER handle leak detected across 100 soak cycles: {d_user:+}");
        assert!(d_handles <= 2, "Kernel handle drift exceeded +2 across 100 soak cycles: {d_handles:+}");
        assert!(d_priv <= 0.5, "Private Commit grew > 0.50 MB across 100 soak cycles: {d_priv:+.2} MB");
        println!("[PASS] Strict Resource Budget Gate PASSED (WS={:.2} MB, Private={:.2} MB, Zero Leaks)!", after.working_set_mb(), after.private_mb());
    }
}

fn main() {
    init_uptime_clock();

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let _ole = OleGuard::init();
    let fonts = PrivateFonts::load_default();

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--self-test") {
        let strict = args.iter().any(|a| a == "--strict-budget");
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
        run_self_test(&fonts, strict);
        return;
    }

    let Some(_singleton) = SingletonMutex::acquire() else {
        tracing::info!("Tomoyo is already running.");
        return;
    };

    let (config, router, plugin_host, clipboard, _apps, updater) = build_engine();
    updater.check_async_if_due(&config);

    let mut shell = match AppShell::create(
        config,
        router,
        plugin_host,
        clipboard,
        updater,
        fonts.regular_family(),
        fonts.medium_family(),
        true,
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to initialize Tomoyo window: {e}");
            return;
        }
    };

    tracing::info!("Tomoyo ready! Press {} to summon.", shell.hotkey_label());

    if !args.iter().any(|a| a == "--silent") {
        shell.show_launcher();
    }

    shell.run_message_loop();
}
