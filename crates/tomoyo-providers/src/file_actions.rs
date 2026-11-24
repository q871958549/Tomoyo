//! Contextual secondary actions ("File Action Flow") appended to file search results by extension.

use std::path::Path;
use tomoyo_core::Lang;
use tomoyo_protocol::{ActionKind, ActionSpec, InternalCommandKind};

/// Appends rich, zero-I/O extension-aware actions to a non-directory file result (`Tab` menu).
pub(crate) fn append_contextual_file_actions(
    actions: &mut Vec<ActionSpec>,
    full_path: &str,
    parent_dir: &str,
    lang: Lang,
) {
    let ext = Path::new(full_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if matches!(ext.as_str(), "exe" | "lnk" | "bat" | "cmd" | "msi" | "msc" | "ps1") {
        actions.push(ActionSpec {
            id: "run_admin".into(),
            label: lang.pick("以管理员身份运行", "Run as Administrator").into(),
            shortcut: Some("Ctrl+Shift+Enter".into()),
            kind: ActionKind::LaunchAsAdmin {
                path: full_path.to_string(),
                args: None,
                working_dir: Some(parent_dir.to_string()),
            },
        });
    }

    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "ico") {
        actions.push(ActionSpec {
            id: "pin_image".into(),
            label: lang.pick("贴图到桌面 (Pin)", "Pin Image to Desktop").into(),
            shortcut: None,
            kind: ActionKind::InternalCommand {
                command: InternalCommandKind::PinImageFile { path: full_path.to_string() },
            },
        });
        actions.push(ActionSpec {
            id: "copy_b64".into(),
            label: lang.pick("复制 Base64 Data URI", "Copy Base64 Data URI").into(),
            shortcut: None,
            kind: ActionKind::InternalCommand {
                command: InternalCommandKind::CopyImageAsBase64 { path: full_path.to_string() },
            },
        });
    } else if ext == "zip" {
        actions.push(ActionSpec {
            id: "extract_zip".into(),
            label: lang.pick("安全解压到同名目录", "Extract ZIP Safely Here").into(),
            shortcut: None,
            kind: ActionKind::InternalCommand {
                command: InternalCommandKind::ExtractZipHere { path: full_path.to_string() },
            },
        });
    } else if matches!(
        ext.as_str(),
        "txt" | "md" | "markdown" | "json" | "toml" | "yaml" | "yml" | "ini" | "cfg"
            | "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "c" | "h" | "cpp" | "java"
            | "sql" | "csv" | "log" | "xml" | "html" | "css" | "svelte" | "sh" | "ps1"
    ) {
        actions.push(ActionSpec {
            id: "copy_file_text".into(),
            label: lang.pick("复制文件内容 (≤128KB)", "Copy File Text (≤128KB)").into(),
            shortcut: None,
            kind: ActionKind::InternalCommand {
                command: InternalCommandKind::CopyFileContents { path: full_path.to_string() },
            },
        });
    }

    actions.push(ActionSpec {
        id: "sha256_file".into(),
        label: lang.pick("计算并复制 SHA-256 校验值", "Copy File SHA-256 Hash").into(),
        shortcut: None,
        kind: ActionKind::InternalCommand {
            command: InternalCommandKind::ComputeFileSha256 { path: full_path.to_string() },
        },
    });
}
