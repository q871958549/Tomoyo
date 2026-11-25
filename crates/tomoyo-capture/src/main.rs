#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Ephemeral On-Demand Native Screenshot, Editable Vector Annotation, Color Picker,
//! Vertical/Horizontal Long Screenshot & Pin-to-Desktop (`tomoyo-capture.exe`).
//!
//! Zero-idle-RAM architecture: exits completely (0 MB) once copy/save/OCR/translate finishes,
//! or stays alive solely while a user-pinned desktop image window remains open.

mod annotate;
mod annotate_arrow;
mod annotate_render;
mod export;
#[cfg(target_os = "linux")]
mod linux_clip;
#[cfg(target_os = "linux")]
mod linux_overlay;
#[cfg(target_os = "linux")]
mod linux_overlay_input;
#[cfg(target_os = "linux")]
mod linux_pin;
#[cfg(target_os = "linux")]
mod linux_rec_hud;
#[cfg(target_os = "linux")]
mod linux_record;
#[cfg(target_os = "linux")]
mod linux_record_export;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod overlay_actions;
mod overlay_draw;
mod overlay_icons;
#[cfg(windows)]
mod overlay_input;
#[cfg(windows)]
mod overlay_text;
#[cfg(windows)]
mod overlay_text_editor;
mod overlay_toolbar;
#[cfg(windows)]
mod pin_card;
#[cfg(windows)]
mod pin_clipboard;
#[cfg(windows)]
mod pin_window;
mod screen;
mod snap_detect;
#[cfg(windows)]
mod scroll_capture;
#[cfg(windows)]
mod scroll_hud;
#[cfg(windows)]
mod scroll_preview;
mod stitch;
#[cfg(windows)]
mod win_record;
#[cfg(windows)]
mod win_record_audio;
#[cfg(windows)]
mod win_record_gif;
#[cfg(windows)]
mod win_record_hud;
#[cfg(windows)]
mod win_record_hud_draw;
#[cfg(windows)]
mod win_record_mf;

#[cfg(windows)]
fn run_windows_main() {
    use annotate::{Annotation, AnnotationStack, COLOR_PRESETS};
    use annotate_render::render_annotations;
    use export::{load_png_as_bgra, save_bgra_as_png};
    use overlay::{run_capture_overlay, CaptureMode, CaptureOutcome};
    use pin_card::render_text_or_color_pin_card;
    use pin_clipboard::read_latest_clipboard_for_pin;
    use pin_window::{run_pin_window, run_pin_window_with_text};
    use screen::{capture_virtual_desktop, synthetic_dib_snapshot, ScreenRect};
    use scroll_capture::{run_scroll_capture, ScrollOutcome};
    use std::path::Path;
    use tomoyo_core::current_process_stats;
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--self-test") {
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
        let before = current_process_stats();
        let (snap, cap_mode) = match capture_virtual_desktop() {
            Ok(s) => (s, "live-desktop"),
            Err(_) => (
                synthetic_dib_snapshot(320, 240).expect("synthetic GDI DIB fallback failed"),
                "headless-dib-fallback",
            ),
        };
        assert!(snap.width > 0 && snap.height > 0);
        let mut sctx = snap_detect::SnapContext::new(vec![ScreenRect { x: 0, y: 0, w: snap.width, h: snap.height }], vec![], snap.width, snap.height);
        assert!(sctx.hover_at(&snap, snap.width / 2, snap.height / 2).is_some());
        let (cw, ch, mut crop) = snap
            .crop_bgra(ScreenRect { x: 0, y: 0, w: snap.width.min(160), h: snap.height.min(120) })
            .expect("crop failed");
        let mut stack = AnnotationStack::default();
        stack.push(Annotation::Rect {
            rect: ScreenRect { x: 4, y: 4, w: 40, h: 30 },
            color: COLOR_PRESETS[0],
            thickness: 3,
            filled: false,
        });
        let mut arrow = Annotation::arrow(10, 50, 70, 60, COLOR_PRESETS[2], 3);
        arrow.move_control(1, 40, 35);
        stack.push(arrow);
        stack.push(Annotation::Mosaic { rect: ScreenRect { x: 50, y: 10, w: 30, h: 30 }, block: 10, blur: true });
        stack.push(Annotation::StepNumber { cx: 90, cy: 25, number: 1, radius: 13, color: COLOR_PRESETS[3] });
        stack.push(Annotation::Text { x: 12, y: 80, text: "Tomoyo".into(), size: 16, color: COLOR_PRESETS[0], bold: false });
        render_annotations(&mut crop, cw as i32, ch as i32, 0, 0, stack.items());
        overlay_text::bake_text_annotations_into_bgra(&mut crop, cw as i32, ch as i32, 0, 0, stack.items());

        let tmp = std::env::temp_dir().join(format!("tomoyo_cap_test_{}.png", std::process::id()));
        save_bgra_as_png(&tmp, cw, ch, &crop).expect("PNG save failed");
        let (dw, dh, decoded) = load_png_as_bgra(&tmp).expect("PNG reload failed");
        assert_eq!((dw, dh), (cw, ch));
        assert_eq!(decoded.len(), crop.len());
        let _ = std::fs::remove_file(&tmp);
        let after = current_process_stats();
        let d_gdi = after.gdi_objects as i64 - before.gdi_objects as i64;
        let d_user = after.user_objects as i64 - before.user_objects as i64;
        assert_eq!(d_gdi, 0, "GDI leak in capture self-test: {d_gdi:+}");
        assert_eq!(d_user, 0, "USER leak in capture self-test: {d_user:+}");

        // Warm up one-time OS CJK font-link handle, then verify 0 GDI/USER drift over 5 pin renders.
        let text_card = render_text_or_color_pin_card("Tomoyo Pin Text Self-Test\n#2563EB", "light", 1.0)
            .expect("text pin card failed");
        assert!(text_card.width >= 240 && text_card.height >= 60);
        let color_card = render_text_or_color_pin_card("#10B981", "dark", 1.0)
            .expect("color pin card failed");
        assert_eq!((color_card.width, color_card.height), (290, 124));
        let pin_before = current_process_stats();
        for _ in 0..5 {
            let _ = render_text_or_color_pin_card("Tomoyo F3 贴图零泄露校验\nfn main() {}", "midnight", 1.0)
                .expect("repeated text pin card failed");
            let _ = render_text_or_color_pin_card("rgb(37, 99, 235)", "light", 1.0)
                .expect("repeated color pin card failed");
        }
        let pin_after = current_process_stats();
        let pin_dgdi = pin_after.gdi_objects as i64 - pin_before.gdi_objects as i64;
        let pin_duser = pin_after.user_objects as i64 - pin_before.user_objects as i64;
        assert_eq!(pin_dgdi, 0, "GDI leak in pin-card self-test: {pin_dgdi:+}");
        assert_eq!(pin_duser, 0, "USER leak in pin-card self-test: {pin_duser:+}");

        println!(
            "[OK] tomoyo-capture v{} self-test passed ({cap_mode} {}x{}, crop {}x{}, curved-arrow+blur+text+pin-card+PNG OK, GDI {d_gdi:+}/{pin_dgdi:+}, USER {d_user:+}/{pin_duser:+})",
            env!("CARGO_PKG_VERSION"),
            snap.width, snap.height, cw, ch
        );
        return;
    }

    if args.iter().any(|a| a == "--pin-clipboard") {
        let cfg = tomoyo_core::TomoyoConfig::load_or_create();
        if let Some(p) = read_latest_clipboard_for_pin(&cfg.theme) {
            let _ = run_pin_window_with_text(p.width, p.height, p.bgra, i32::MIN, i32::MIN, p.source_text);
        }
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--pin") {
        if let Some(p) = args.get(pos + 1) {
            let img_path = Path::new(p);
            let loaded = load_png_as_bgra(img_path).ok().or_else(|| {
                let (_, _, w, h, mut rgba) =
                    tomoyo_core::wic_thumb::decode_image_thumbnail_wic(img_path, 3840, 2160)?;
                for px in rgba.as_chunks_mut::<4>().0 {
                    px.swap(0, 2); // RGBA -> BGRA
                }
                Some((w, h, rgba))
            });
            if let Some((w, h, bgra)) = loaded {
                let _ = run_pin_window(w, h, bgra, 120, 120);
            }
        }
        return;
    }

    if win_record::try_stop_active_recording() {
        return;
    }

    let mode = if args.iter().any(|a| a == "--ocr") {
        CaptureMode::QuickOcr
    } else if args.iter().any(|a| a == "--translate") {
        CaptureMode::QuickTranslate
    } else if args.iter().any(|a| a == "--record") {
        CaptureMode::Record
    } else {
        CaptureMode::Region
    };

    match run_capture_overlay(mode) {
        Ok(CaptureOutcome::Pin { width, height, bgra, screen_x, screen_y }) => {
            let _ = run_pin_window(width, height, bgra, screen_x, screen_y);
        }
        Ok(CaptureOutcome::ScrollLong { screen_x, screen_y, width, height, save_dir }) => {
            if let Ok(ScrollOutcome::Pin { width, height, bgra, screen_x, screen_y }) =
                run_scroll_capture(screen_x, screen_y, width, height, save_dir)
            {
                let _ = run_pin_window(width, height, bgra, screen_x, screen_y);
            }
        }
        Ok(CaptureOutcome::Record(req)) => {
            win_record::run_screen_recording(req);
        }
        _ => {}
    }
}

#[cfg(target_os = "linux")]
fn run_windows_main() {
    use annotate::{AnnotColor, Annotation, AnnotationStack};
    use annotate_render::render_annotations;
    use export::{load_png_as_bgra, save_bgra_as_png};
    use linux_overlay::{run_capture_overlay, CaptureMode, CaptureOutcome};
    use linux_pin::{read_latest_clipboard_for_pin, render_text_or_color_pin_card, run_pin_window, run_pin_window_with_text};
    use screen::{synthetic_dib_snapshot, ScreenRect};
    use tomoyo_core::current_process_stats;

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--self-test") {
        let before = current_process_stats();
        let snap = synthetic_dib_snapshot(320, 240).expect("synthetic snapshot failed");
        let mut sctx = snap_detect::SnapContext::new(vec![ScreenRect { x: 10, y: 10, w: 300, h: 220 }], vec![], 320, 240);
        assert!(sctx.hover_at(&snap, 50, 50).is_some());
        let rect = ScreenRect { x: 20, y: 20, w: 120, h: 90 };
        let (cw, ch, mut crop) = snap.crop_bgra(rect).expect("crop failed");
        let mut stack = AnnotationStack::default();
        stack.push(Annotation::Rect {
            rect: ScreenRect { x: 10, y: 10, w: 50, h: 40 },
            color: AnnotColor(0x3B, 0x82, 0xF6),
            thickness: 3,
            filled: false,
        });
        stack.push(Annotation::arrow(15, 15, 95, 70, AnnotColor(0xEF, 0x44, 0x44), 3));
        render_annotations(&mut crop, cw as i32, ch as i32, 0, 0, stack.items());

        let tmp = std::env::temp_dir().join(format!("tomoyo_cap_self_{}.png", std::process::id()));
        save_bgra_as_png(&tmp, cw, ch, &crop).expect("PNG save failed");
        let (dw, dh, decoded) = load_png_as_bgra(&tmp).expect("PNG reload failed");
        assert_eq!((dw, dh), (cw, ch));
        assert_eq!(decoded.len(), crop.len());
        let _ = std::fs::remove_file(&tmp);

        let text_card = render_text_or_color_pin_card("Tomoyo Pin Text Self-Test\n#2563EB", "light", 1.0)
            .expect("text pin card failed");
        assert!(text_card.width >= 240 && text_card.height >= 60);
        let color_card = render_text_or_color_pin_card("#10B981", "dark", 1.0)
            .expect("color pin card failed");
        assert_eq!((color_card.width, color_card.height), (290, 124));
        for _ in 0..5 {
            let _ = render_text_or_color_pin_card("Tomoyo F3 贴图零泄露校验\nfn main() {}", "midnight", 1.0)
                .expect("repeated text pin card failed");
            let _ = render_text_or_color_pin_card("rgb(37, 99, 235)", "light", 1.0)
                .expect("repeated color pin card failed");
        }
        let after = current_process_stats();
        let d_handles = after.handle_count as i64 - before.handle_count as i64;
        assert!(d_handles <= 2, "FD leak in Linux capture self-test: {d_handles:+}");
        println!(
            "[OK] tomoyo-capture v{} Linux self-test passed ({}x{}, crop {cw}x{ch}, snap+Pango+PNG OK, Handles {d_handles:+})",
            env!("CARGO_PKG_VERSION"), snap.width, snap.height
        );
        return;
    }

    if args.iter().any(|a| a == "--pin-clipboard") {
        let cfg = tomoyo_core::TomoyoConfig::load_or_create();
        if let Some(p) = read_latest_clipboard_for_pin(&cfg.theme) {
            run_pin_window_with_text(p.width, p.height, p.bgra, i32::MIN, i32::MIN, p.source_text);
        }
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--pin")
        && let Some(path) = args.get(pos + 1)
    {
        if let Ok((w, h, bgra)) = load_png_as_bgra(std::path::Path::new(path)) {
            run_pin_window(w, h, bgra, 120, 120);
        }
        return;
    }

    let mode = if args.iter().any(|a| a == "--ocr") {
        CaptureMode::QuickOcr
    } else if args.iter().any(|a| a == "--translate") {
        CaptureMode::QuickTranslate
    } else if args.iter().any(|a| a == "--record") {
        if linux_record::try_stop_active_recording() { return; }
        CaptureMode::Record
    } else {
        CaptureMode::Region
    };

    match run_capture_overlay(mode) {
        Ok(CaptureOutcome::Pin { width, height, bgra, screen_x, screen_y }) => {
            run_pin_window(width, height, bgra, screen_x, screen_y);
        }
        Ok(CaptureOutcome::Record(req)) => {
            if let Err(e) = linux_record::run_screen_recording(req) {
                let _ = std::process::Command::new("notify-send").args(["Tomoyo 录屏提示", &e]).spawn();
            }
        }
        _ => {}
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn run_windows_main() {
    eprintln!("tomoyo-capture requires Windows or Linux");
}

#[cfg(all(windows, test))]
pub(crate) static PROCESS_GUI_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn main() {
    run_windows_main();
}
