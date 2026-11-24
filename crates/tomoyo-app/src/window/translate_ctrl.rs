//! Win32 lossless foreground selection capture helpers (`GetClipboardSequenceNumber` + `SendInput Ctrl+C`)
//! shared by Protocol v3 plugin hotkeys with `capture_selection = true` (`Permission::SelectionRead`).

#[cfg(windows)]
pub(super) fn clipboard_seq_num() -> u32 {
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

#[cfg(windows)]
pub(super) fn send_ctrl_c_cleanly() {
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
