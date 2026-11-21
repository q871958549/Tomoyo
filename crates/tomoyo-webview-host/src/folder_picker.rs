//! Native Windows folder picker (`IFileOpenDialog` + `FOS_PICKFOLDERS`) for Settings Center.

use std::ffi::c_void;
use tomoyo_core::validate_scan_dir;
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER,
};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};

/// Opens the native Windows folder selection dialog parented to `owner` and validates
/// the chosen directory against Tomoyo's scan directory safety rules.
pub fn pick_scan_folder(owner: HWND) -> String {
    match show_folder_dialog(owner, w!("选择自定义应用或便携软件目录")) {
        Some(raw_path) => match validate_scan_dir(&raw_path) {
            Ok(clean) => serde_json::json!({
                "event": "folder_picked",
                "ok": true,
                "cancelled": false,
                "path": clean
            })
            .to_string(),
            Err(reason) => serde_json::json!({
                "event": "folder_picked",
                "ok": false,
                "cancelled": false,
                "path": raw_path,
                "error": reason
            })
            .to_string(),
        },
        None => serde_json::json!({
            "event": "folder_picked",
            "ok": false,
            "cancelled": true
        })
        .to_string(),
    }
}

pub fn pick_capture_save_folder(owner: HWND) -> String {
    match show_folder_dialog(owner, w!("选择截图默认保存目录")) {
        Some(raw_path) if std::path::Path::new(&raw_path).is_dir() => serde_json::json!({
            "event": "capture_dir_picked",
            "ok": true,
            "cancelled": false,
            "path": raw_path
        })
        .to_string(),
        Some(raw_path) => serde_json::json!({
            "event": "capture_dir_picked",
            "ok": false,
            "cancelled": false,
            "error": format!("所选目录不可访问: {raw_path}")
        })
        .to_string(),
        None => serde_json::json!({
            "event": "capture_dir_picked",
            "ok": false,
            "cancelled": true
        })
        .to_string(),
    }
}

fn show_folder_dialog(owner: HWND, title: windows::core::PCWSTR) -> Option<String> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let cur_opts = dialog.GetOptions().unwrap_or_default();
        let _ = dialog.SetOptions(
            cur_opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR,
        );
        let _ = dialog.SetTitle(title);
        if dialog.Show(Some(owner)).is_err() {
            return None;
        }
        let item = dialog.GetResult().ok()?;
        let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = pwstr.to_string().ok();
        CoTaskMemFree(Some(pwstr.0 as *const c_void));
        path
    }
}
