#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tomoyo — 100% Open-Source, Ultra-Low-Memory Rust Command Launcher.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod actions;
mod brand;
mod crash_guard;
#[cfg(windows)]
mod drag;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod image_icons;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod popup_menu;
mod preview_loader;
mod renderer;
mod selftest_runner;
#[cfg(windows)]
mod super_panel;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod window;

#[cfg(target_os = "linux")]
use linux::{wake_existing_instance, AppShell, SingletonGuard as SingletonMutex};
use std::sync::{Arc, RwLock};
use tomoyo_core::{
    data_dir, init_uptime_clock, ExternalPluginBridge, FrecencyStore, PrivateFonts, Provider,
    QueryRouter, TomoyoConfig, UpdateManager, CURRENT_VERSION,
};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};
use tomoyo_providers::{
    AppsProvider, CalcProvider, EverythingProvider, ProcessProvider, SnippetsProvider,
    SystemProvider, WebSearchProvider, WindowsSwitcherProvider,
};
#[cfg(windows)]
use window::{AppShell, WM_APP_UPDATE_READY, WM_APP_WAKE_EXISTING};
#[cfg(windows)]
use windows::core::{w, PCWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, WPARAM,
};
#[cfg(windows)]
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, PostMessageW,
};

#[cfg(windows)]
struct OleGuard;
#[cfg(windows)]
impl OleGuard {
    fn init() -> Self {
        unsafe {
            let _ = OleInitialize(None);
        }
        Self
    }
}
#[cfg(windows)]
impl Drop for OleGuard {
    fn drop(&mut self) {
        unsafe {
            OleUninitialize();
        }
    }
}

#[cfg(windows)]
struct SingletonMutex(HANDLE);
#[cfg(windows)]
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
#[cfg(windows)]
impl Drop for SingletonMutex {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_invalid() {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

type EngineBundle = (
    Arc<RwLock<TomoyoConfig>>,
    Arc<QueryRouter>,
    Arc<PluginHost>,
    Arc<AppsProvider>,
    Arc<UpdateManager>,
);

fn build_engine() -> EngineBundle {
    let loaded_cfg = TomoyoConfig::load_or_create();
    let updater = UpdateManager::with_cached(&loaded_cfg.update.repo);
    let config = Arc::new(RwLock::new(loaded_cfg));

    let plugin_dirs = default_plugin_dirs();
    let plugin_host = PluginHost::new(plugin_dirs);
    let bridge: Arc<dyn ExternalPluginBridge> = plugin_host.clone();

    let frecency_path = data_dir().join("frecency.json");
    let frecency = Arc::new(RwLock::new(FrecencyStore::load_or_default(frecency_path)));

    let apps = Arc::new(AppsProvider::with_frecency(config.clone(), frecency.clone()));
    let everything = Arc::new(EverythingProvider::new(config.clone()));
    let calc = Arc::new(CalcProvider::new());
    let snippets = Arc::new(SnippetsProvider::new(config.clone()));
    let flows = Arc::new(tomoyo_providers::FlowsProvider::new(config.clone()));
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
        snippets,
        flows,
        web,
        everything,
        windows,
        processes,
        system,
    ];

    let router = Arc::new(QueryRouter::new(providers, Some(bridge), frecency).with_config(config.clone()));
    (config, router, plugin_host, apps, updater)
}

#[cfg(windows)]
fn wake_existing_instance(silent: bool) {
    if silent {
        return;
    }
    unsafe {
        if let Ok(hwnd) = FindWindowW(w!("TomoyoLauncherWindowClass"), PCWSTR::null())
            && !hwnd.is_invalid() {
                let mut pid = 0u32;
                let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != 0 {
                    let _ = AllowSetForegroundWindow(pid);
                }
                let _ = PostMessageW(Some(hwnd), WM_APP_WAKE_EXISTING, WPARAM(0), LPARAM(0));
                tracing::info!("Signaled existing Tomoyo instance (PID {pid}) to show launcher.");
            }
    }
}

fn main() {
    const MI_OPTION_PURGE_DELAY: std::ffi::c_int = 15;
    unsafe extern "C" {
        fn mi_option_set(option: std::ffi::c_int, value: std::ffi::c_long);
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("MIMALLOC_ALLOW_THP").is_none() {
        const MI_OPTION_ALLOW_THP: std::ffi::c_int = 43;
        linux::disable_transparent_hugepages();
        unsafe { mi_option_set(MI_OPTION_ALLOW_THP, 0); }
    }
    if std::env::var_os("MIMALLOC_PURGE_DELAY").is_none() {
        unsafe { mi_option_set(MI_OPTION_PURGE_DELAY, 0); }
    }
    init_uptime_clock();
    let args: Vec<String> = std::env::args().collect();
    let is_self_test = args.iter().any(|a| a == "--self-test");
    let is_silent = args.iter().any(|a| a == "--silent");
    let is_updated = args.iter().any(|a| a == "--updated");
    let is_update_failed = args.iter().any(|a| a == "--update-failed");

    let log_writer = crash_guard::init_logging_and_crash_guards(is_self_test);
    let _ = tracing_subscriber::fmt()
        .with_timer(crash_guard::BeijingTimer)
        .with_writer(log_writer)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    #[cfg(windows)]
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    #[cfg(windows)]
    let _ole = OleGuard::init();
    let fonts = PrivateFonts::load_default();

    if is_self_test {
        let strict = args.iter().any(|a| a == "--strict-budget");
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
        selftest_runner::run_self_test(&fonts, strict);
        return;
    }

    let Some(_singleton) = SingletonMutex::acquire() else {
        tracing::info!("Tomoyo is already running (silent={is_silent}).");
        wake_existing_instance(is_silent);
        return;
    };

    tracing::info!("Starting Tomoyo v{CURRENT_VERSION} (args={args:?})");
    let (config, router, plugin_host, _apps, updater) = build_engine();

    let mut shell = match AppShell::create(
        Arc::clone(&config),
        router,
        plugin_host,
        Arc::clone(&updater),
        fonts.regular_family(),
        fonts.medium_family(),
        true,
    ) {
        Ok(s) => s,
        Err(e) => {
            crash_guard::show_fatal_error_dialog(&format!("Failed to initialize launcher window: {e}"));
            return;
        }
    };

    #[cfg(windows)]
    {
        let hwnd_raw = shell.hwnd_raw();
        updater.check_async_if_due_with_notify(&config, move || unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd_raw as *mut std::ffi::c_void)),
                WM_APP_UPDATE_READY,
                WPARAM(0),
                LPARAM(0),
            );
        });
    }
    #[cfg(target_os = "linux")]
    shell.schedule_startup_update_check();

    tracing::info!("Tomoyo ready! Press {} to summon.", shell.hotkey_label());

    if is_updated {
        shell.show_launcher();
        shell.show_toast(&format!("✓ Tomoyo 已自动更新至 v{CURRENT_VERSION}"));
    } else if is_update_failed {
        shell.show_launcher();
        shell.show_toast("⚠ 自动安装未完成，已为您恢复旧版本（可在 :update 重试）");
    } else if !is_silent {
        shell.show_launcher();
    }

    shell.run_message_loop();
    tracing::info!("Tomoyo message loop exited cleanly.");
}
