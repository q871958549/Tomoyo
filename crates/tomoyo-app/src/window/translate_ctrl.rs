//! Selection-translate hotkey (`Alt+T`) with lossless clipboard save/restore and
//! zero clipboard-history pollution, plus ephemeral Translator popup launcher.

use super::AppShell;
use crate::actions::{read_clipboard_text, set_clipboard_text, spawn_translator_window};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

impl AppShell {
    pub fn open_translator_window(&mut self, text: &str, target_lang: Option<&str>) {
        match spawn_translator_window(self.hwnd.0 as isize, &self.theme_name, text, target_lang) {
            Ok(()) => self.hide_launcher(),
            Err(e) => self.show_toast(&format!("翻译窗口启动失败: {e}")),
        }
    }

    /// Captures the currently selected text in the active application via simulated `Ctrl+C`,
    /// restores the previous clipboard text cleanly without polluting Tomoyo's clipboard history,
    /// and opens the ephemeral multi-engine Translator popup.
    pub fn trigger_selection_translate(&mut self) {
        let theme = self.theme_name.clone();
        let parent_hwnd = self.hwnd.0 as isize;
        let suppress_flag = Arc::clone(&self.suppress_clipboard_capture);

        let _ = std::thread::Builder::new()
            .name("tomoyo-sel-translate".into())
            .spawn(move || {
                suppress_flag.store(true, Ordering::Release);
                let prev_text = read_clipboard_text();
                let seq_before = clipboard_seq_num();

                send_ctrl_c_cleanly();

                let mut captured = String::new();
                let mut changed = false;
                for _ in 0..11 {
                    std::thread::sleep(Duration::from_millis(16));
                    if clipboard_seq_num() != seq_before {
                        changed = true;
                        if let Some(txt) = read_clipboard_text() {
                            captured = txt;
                        }
                        break;
                    }
                }

                // Restore prior clipboard text cleanly so selection-translate never clobbers user data.
                if changed {
                    if let Some(old) = prev_text {
                        let _ = set_clipboard_text(&old);
                    }
                }
                std::thread::sleep(Duration::from_millis(60));
                suppress_flag.store(false, Ordering::Release);

                let trimmed = captured.trim().to_string();
                if let Err(e) = spawn_translator_window(parent_hwnd, &theme, &trimmed, None) {
                    tracing::error!("Failed to launch translator popup: {e}");
                }
            });
    }
}

#[cfg(windows)]
fn clipboard_seq_num() -> u32 {
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

#[cfg(windows)]
fn send_ctrl_c_cleanly() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_CONTROL, VK_MENU, VK_SHIFT,
    };

    let make = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                ..Default::default()
            },
        },
    };
    // Release hotkey modifiers (e.g. Alt from Alt+T) before injecting clean Ctrl+C.
    let inputs = [
        make(VK_MENU, true),
        make(VK_SHIFT, true),
        make(VK_CONTROL, false),
        make(VIRTUAL_KEY(b'C' as u16), false),
        make(VIRTUAL_KEY(b'C' as u16), true),
        make(VK_CONTROL, true),
    ];
    unsafe {
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}
