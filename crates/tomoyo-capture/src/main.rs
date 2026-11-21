#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Ephemeral On-Demand Native Screenshot, Editable Vector Annotation, Color Picker,
//! Vertical/Horizontal Long Screenshot & Pin-to-Desktop (`tomoyo-capture.exe`).
//!
//! Zero-idle-RAM architecture: exits completely (0 MB) once copy/save/OCR/translate finishes,
//! or stays alive solely while a user-pinned desktop image window remains open.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod annotate;
mod annotate_arrow;
mod annotate_render;
mod export;
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
mod pin_window;
mod screen;
#[cfg(windows)]
mod scroll_capture;
#[cfg(windows)]
mod scroll_hud;
#[cfg(windows)]
mod scroll_preview;
mod stitch;

#[cfg(windows)]
fn run_windows_main() {
    use annotate::{Annotation, AnnotationStack, COLOR_PRESETS};
    use annotate_render::render_annotations;
    use export::{load_png_as_bgra, read_clipboard_image_bgra, save_bgra_as_png};
    use overlay::{run_capture_overlay, CaptureMode, CaptureOutcome};
    use pin_window::run_pin_window;
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
        stack.push(Annotation::Text { x: 12, y: 80, text: "Tomoyo".into(), size: 16, color: COLOR_PRESETS[0] });
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
        println!(
            "[OK] tomoyo-capture v{} self-test passed ({cap_mode} {}x{}, crop {}x{}, curved-arrow+blur+text+PNG OK, GDI {d_gdi:+}, USER {d_user:+})",
            env!("CARGO_PKG_VERSION"),
            snap.width, snap.height, cw, ch
        );
        return;
    }

    if args.iter().any(|a| a == "--pin-clipboard") {
        if let Some((w, h, bgra)) = read_clipboard_image_bgra() {
            let _ = run_pin_window(w, h, bgra, 120, 120);
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

    let mode = if args.iter().any(|a| a == "--ocr") {
        CaptureMode::QuickOcr
    } else if args.iter().any(|a| a == "--translate") {
        CaptureMode::QuickTranslate
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
        _ => {}
    }
}

#[cfg(not(windows))]
fn run_windows_main() {
    eprintln!("tomoyo-capture requires Windows");
}

fn main() {
    run_windows_main();
}
