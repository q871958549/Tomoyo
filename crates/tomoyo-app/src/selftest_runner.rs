//! Full-stack self-test & strict resource-budget anti-leak harness (`tomoyo --self-test`).

use crate::build_engine;
#[cfg(target_os = "linux")]
use crate::linux::AppShell;
#[cfg(windows)]
use crate::window::AppShell;
use tomoyo_core::{current_process_stats, PrivateFonts, CURRENT_VERSION};
use tomoyo_protocol::AttachedContext;

pub(crate) fn log_mem(stage: &str) {
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

#[cfg(windows)]
fn disable_third_party_ime_for_selftest() {
    use windows::core::{s, w};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    unsafe {
        if let Ok(hmod) = LoadLibraryW(w!("imm32.dll")) {
            for sym in [s!("ImmDisableIME"), s!("ImmDisableTextFrameService")] {
                if let Some(proc_fn) = GetProcAddress(hmod, sym) {
                    let disable_fn: unsafe extern "system" fn(u32) -> i32 =
                        std::mem::transmute(proc_fn);
                    let _ = disable_fn(u32::MAX);
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn disable_third_party_ime_for_selftest() {}

pub(crate) fn run_self_test(fonts: &PrivateFonts, strict_budget: bool) {
    disable_third_party_ime_for_selftest();
    println!("=== Tomoyo v{CURRENT_VERSION} Full-Stack Anti-Leak & Ecosystem Self-Test ===");
    println!(
        "Font engine: Regular=`{}` · Medium=`{}` (custom_bundled={})",
        fonts.regular_family(),
        fonts.medium_family(),
        fonts.has_alibaba_regular()
    );
    log_mem("0. process startup");
    let (config, router, plugin_host, apps, updater) = build_engine();
    plugin_host.set_interactive_prompts(false);
    log_mem("1. after build_engine (Microkernel)");
    println!("Indexed installed apps (incl. UWP): {}", apps.app_count());
    println!("Discovered plugins: {}", plugin_host.list_plugins().len());

    let mut shell = AppShell::create(
        config,
        router.clone(),
        plugin_host.clone(),
        updater,
        fonts.regular_family(),
        fonts.medium_family(),
        false,
    )
    .expect("failed to create AppShell");
    log_mem("2. after AppShell::create");

    let _ = shell.self_test_paint_and_hibernate(":stats");
    log_mem("2a. SDF+NativeFont+Preview paint");
    let _ = shell.self_test_paint_and_hibernate("");
    log_mem("2b. after shell icon paint");

    let queries = [
        "jsq", "设置", "(1024*768)/16", "#7c6af7", "1700000000", "g rust tomoyo",
        "f Cargo.toml", "kill", "kill explorer", ":update", ":stats", ":plugins",
        "snip iso", "flow", "cb", "cap", "@pdf", "hi Tomoyo Rust", "py tomoyo",
        "unit 2.5gb", "json {\"ok\":true}",
    ];
    for q in queries {
        let hits = shell.self_test_paint_and_hibernate(q);
        println!("Query `{q}` -> {hits} hits");
    }

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
    #[cfg(windows)]
    shell.compact_os_heaps();
    assert!(shell.is_renderer_hibernated(), "renderer should release surfaces on hibernate");

    let soak_queries = ["jsq", "f Cargo.toml", "2^16", ":stats", "g rust", "cb", "@pdf"];
    let mut baseline = current_process_stats();
    for cycle in 0..(soak_queries.len() + 100) {
        if cycle == soak_queries.len() {
            baseline = current_process_stats();
            log_mem("3. baseline (hibernated+mi_purge)");
        }
        let _ = shell.self_test_paint_and_hibernate(soak_queries[cycle % soak_queries.len()]);
        shell.wait_background_idle_for_test();
        let _ = shell.run_hibernate();
        shell.wait_background_idle_for_test();
        #[cfg(windows)]
        shell.compact_os_heaps();
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
        #[cfg(windows)]
        assert!(after.working_set_mb() <= 2.0, "Hibernated WS exceeded 2.0 MB budget: {:.2} MB", after.working_set_mb());
        #[cfg(windows)]
        assert!(after.private_mb() <= 28.0, "Hibernated Private Commit exceeded 28.0 MB budget: {:.2} MB", after.private_mb());
        #[cfg(not(windows))]
        assert!(after.private_mb() <= 45.0, "Hibernated Private RSS exceeded 45.0 MB budget: {:.2} MB", after.private_mb());
        assert_eq!(d_gdi, 0, "GDI handle leak detected across 100 soak cycles: {d_gdi:+}");
        assert_eq!(d_user, 0, "USER handle leak detected across 100 soak cycles: {d_user:+}");
        assert!(d_handles <= 2, "Kernel/FD handle drift exceeded +2 across 100 soak cycles: {d_handles:+}");
        assert!(d_priv <= 0.5, "Private Commit grew > 0.50 MB across 100 soak cycles: {d_priv:+.2} MB");
        println!(
            "[PASS] Strict Resource Budget Gate PASSED (WS={:.2} MB, Private={:.2} MB, Zero Leaks)!",
            after.working_set_mb(),
            after.private_mb()
        );
    }
}
