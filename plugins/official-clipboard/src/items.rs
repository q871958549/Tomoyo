//! ResultItem builders for Clipboard Center entries (Text, Image, Files), commands & filter hints.

use crate::filter::{normalize_single_line, ClipboardSubtype};
use crate::store::{ClipboardEntry, ClipboardPayload};
use crate::PLUGIN_ID;
use chrono::{Local, TimeZone};
use serde_json::json;
use std::path::Path;
use tomoyo_sdk::{ActionKind, ActionSpec, IconSpec, PreviewSpec, ResultCategory, ResultItem};

pub fn build_clipboard_item(idx: usize, entry: ClipboardEntry) -> ResultItem {
    let time_str = Local
        .timestamp_opt(entry.captured_at_unix, 0)
        .single()
        .map(|dt| dt.format("%m-%d %H:%M:%S").to_string())
        .unwrap_or_default();
    let pin_prefix = if entry.pinned { "★ " } else { "" };
    let tag_prefix = if entry.tag.is_empty() { String::new() } else { format!("[#{}] ", entry.tag) };
    let src_suffix = if entry.source_app.is_empty() { String::new() } else { format!(" · {}", entry.source_app) };
    let note_suffix = if entry.note.is_empty() { String::new() } else { format!(" · {}", entry.note) };

    match entry.payload {
        ClipboardPayload::Text { text, char_count, subtype } => build_text_item(
            idx, entry.key, entry.preview_line, text, char_count, subtype, entry.captured_at_unix,
            entry.pinned, pin_prefix, &tag_prefix, &time_str, &src_suffix, &note_suffix,
            &entry.tag, &entry.note, &entry.source_app,
        ),
        ClipboardPayload::Image { path, width, height } => build_image_item(
            idx, entry.key, entry.preview_line, path, width, height, entry.captured_at_unix,
            entry.pinned, pin_prefix, &tag_prefix, &time_str, &src_suffix, &note_suffix,
        ),
        ClipboardPayload::Files { paths } => build_files_item(
            idx, entry.key, entry.preview_line, paths, entry.captured_at_unix,
            entry.pinned, pin_prefix, &tag_prefix, &time_str, &src_suffix, &note_suffix,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_text_item(
    idx: usize,
    key: String,
    preview_line: String,
    text: String,
    char_count: usize,
    subtype: ClipboardSubtype,
    ts: i64,
    pinned: bool,
    pin_prefix: &str,
    tag_prefix: &str,
    time_str: &str,
    src_suffix: &str,
    note_suffix: &str,
    tag: &str,
    note: &str,
    source_app: &str,
) -> ResultItem {
    let line_count = text.lines().count().max(1);
    let badge = if pinned {
        "置顶".to_string()
    } else if !tag.is_empty() {
        format!("#{tag}")
    } else {
        subtype.badge_label().to_string()
    };
    let single_line = normalize_single_line(&text);
    let subtitle = format!(
        "{time_str}{src_suffix} · {char_count} 字/{line_count} 行{note_suffix} · Shift+Enter 单行贴 · Ctrl+E 工作台"
    );

    let mut actions = vec![
        ActionSpec { id: "paste".into(), label: "粘贴到原窗口（纯文本）".into(), shortcut: Some("Enter".into()), kind: ActionKind::CopyText { text: text.clone(), paste_immediately: true } },
        ActionSpec { id: "copy".into(), label: "仅复制到剪贴板".into(), shortcut: Some("Ctrl+Enter".into()), kind: ActionKind::CopyText { text: text.clone(), paste_immediately: false } },
        ActionSpec { id: "paste_single".into(), label: "合并为单行纯文本并粘贴".into(), shortcut: Some("Shift+Enter".into()), kind: ActionKind::CopyText { text: single_line, paste_immediately: true } },
    ];
    if subtype == ClipboardSubtype::Link {
        actions.push(ActionSpec { id: "open_url".into(), label: "在浏览器中打开链接".into(), shortcut: None, kind: ActionKind::OpenUrl { url: text.trim().to_string() } });
    }
    actions.push(crate::paste_stack::enqueue_action(&text));
    actions.extend(common_manage_actions(&key, pinned));

    let mut preview_body = String::new();
    if !note.is_empty() || !tag.is_empty() || !source_app.is_empty() {
        preview_body.push_str(&format!(
            "来源: {} | 标签: {} | 备注: {}\n────────────────────────\n",
            if source_app.is_empty() { "-" } else { source_app },
            if tag.is_empty() { "-" } else { tag },
            if note.is_empty() { "-" } else { note },
        ));
    }
    preview_body.push_str(&text);

    ResultItem {
        id: format!("cb:{idx}:{ts}"),
        provider_id: PLUGIN_ID.into(),
        title: format!("{pin_prefix}{tag_prefix}{preview_line}"),
        subtitle,
        category: ResultCategory::Clipboard,
        score: if pinned { 9_400 } else { 8_500 } - (idx as i64 * 20),
        icon: IconSpec::Monogram(if pinned { "★".into() } else { subtype.badge_label().into() }),
        badge: Some(badge),
        copy_text: Some(text),
        actions,
        preview: Some(PreviewSpec::Text {
            title: format!("剪贴板 #{} · {} ({time_str})", idx + 1, subtype.badge_label()),
            body: preview_body,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_image_item(
    idx: usize,
    key: String,
    preview_line: String,
    path: String,
    width: u32,
    height: u32,
    ts: i64,
    pinned: bool,
    pin_prefix: &str,
    tag_prefix: &str,
    time_str: &str,
    src_suffix: &str,
    note_suffix: &str,
) -> ResultItem {
    let badge = if pinned { "置顶图".into() } else { "IMG".into() };
    let subtitle = format!("{time_str}{src_suffix} · {width}×{height}{note_suffix} · Shift+Enter 一键 OCR · Ctrl+E 工作台");
    let mut actions = vec![
        ActionSpec {
            id: "paste_img".into(),
            label: "粘贴图片到原窗口".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::CopyImage { path: path.clone(), paste_immediately: true },
        },
        ActionSpec {
            id: "copy_img".into(),
            label: "仅复制图片到剪贴板".into(),
            shortcut: Some("Ctrl+Enter".into()),
            kind: ActionKind::CopyImage { path: path.clone(), paste_immediately: false },
        },
        ActionSpec {
            id: "ocr_img".into(),
            label: "OCR 识别图片中的文字".into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::FillQuery { query: format!("ocr \"{path}\"") },
        },
        ActionSpec {
            id: "open_img".into(),
            label: "用系统看图打开".into(),
            shortcut: None,
            kind: ActionKind::LaunchPath { path: path.clone(), args: None, working_dir: None },
        },
        ActionSpec {
            id: "reveal_img".into(),
            label: "在文件管理器定位图片".into(),
            shortcut: None,
            kind: ActionKind::RevealInExplorer { path: path.clone() },
        },
    ];
    actions.extend(common_manage_actions(&key, pinned));

    ResultItem {
        id: format!("cb:img:{idx}:{ts}"),
        provider_id: PLUGIN_ID.into(),
        title: format!("{pin_prefix}{tag_prefix}{preview_line}"),
        subtitle,
        category: ResultCategory::Clipboard,
        score: if pinned { 9_450 } else { 8_520 } - (idx as i64 * 20),
        icon: IconSpec::Monogram("IMG".into()),
        badge: Some(badge),
        copy_text: Some(path.clone()),
        actions,
        preview: Some(PreviewSpec::ImagePreview { path, width, height }),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_files_item(
    idx: usize,
    key: String,
    preview_line: String,
    paths: Vec<String>,
    ts: i64,
    pinned: bool,
    pin_prefix: &str,
    tag_prefix: &str,
    time_str: &str,
    src_suffix: &str,
    note_suffix: &str,
) -> ResultItem {
    let joined = paths.join("\r\n");
    let first = paths.first().cloned().unwrap_or_default();
    let dir = Path::new(&first).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| first.clone());
    let mut actions = vec![
        ActionSpec {
            id: "paste_paths".into(),
            label: "粘贴文件路径到原窗口".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::CopyText { text: joined.clone(), paste_immediately: true },
        },
        ActionSpec {
            id: "reveal_first".into(),
            label: "在文件管理器定位首个文件".into(),
            shortcut: Some("Ctrl+Enter".into()),
            kind: ActionKind::RevealInExplorer { path: first.clone() },
        },
        ActionSpec {
            id: "term_here".into(),
            label: "在所在目录打开终端".into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::OpenTerminalAt { dir },
        },
    ];
    actions.extend(common_manage_actions(&key, pinned));

    ResultItem {
        id: format!("cb:files:{idx}:{ts}"),
        provider_id: PLUGIN_ID.into(),
        title: format!("{pin_prefix}{tag_prefix}{preview_line}"),
        subtitle: format!("{time_str}{src_suffix} · {} 个文件{note_suffix} · Enter 贴路径 · Ctrl+Enter 定位", paths.len()),
        category: ResultCategory::Clipboard,
        score: if pinned { 9_420 } else { 8_510 } - (idx as i64 * 20),
        icon: if first.is_empty() { IconSpec::Monogram("FILE".into()) } else { IconSpec::ShellPath(first) },
        badge: Some(if pinned { "置顶".into() } else { format!("{} 文件", paths.len()) }),
        copy_text: Some(joined.clone()),
        actions,
        preview: Some(PreviewSpec::Text {
            title: format!("已复制文件清单 ({})", paths.len()),
            body: joined,
        }),
    }
}

fn common_manage_actions(key: &str, pinned: bool) -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: "edit_workbench".into(),
            label: "在剪贴板工作台编辑 / 打标签 / 备注".into(),
            shortcut: Some("Ctrl+E".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "open_workbench".into(),
                payload: json!({ "focus_key": key }),
            },
        },
        ActionSpec {
            id: "pin".into(),
            label: if pinned { "取消置顶".into() } else { "置顶收藏".into() },
            shortcut: Some("Ctrl+Shift+P".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "toggle_pin".into(),
                payload: json!({ "id_key": key }),
            },
        },
        ActionSpec {
            id: "delete".into(),
            label: "删除该条剪贴板记录".into(),
            shortcut: Some("Ctrl+D".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "delete".into(),
                payload: json!({ "id_key": key }),
            },
        },
    ]
}

pub fn build_clip_commands(cmd: &str, capturing: bool, active_secrets: usize) -> Vec<ResultItem> {
    let low = cmd.to_ascii_lowercase();
    let mut out = Vec::new();
    if ":purge".starts_with(&low) || low == ":" {
        out.push(crate::secrets_guard::build_purge_command_item(active_secrets));
    }
    if ":manage".starts_with(&low) || ":m".starts_with(&low) || low == ":" {
        out.push(ResultItem {
            id: "cb:cmd:manage".into(),
            provider_id: PLUGIN_ID.into(),
            title: "cb :manage — 打开剪贴板工作台（分组标签 / 编辑 / 备份）".into(),
            subtitle: "按需拉起独立窗口，支持编辑文本、标注标签与导出 JSON，关闭即释放 100% 内存".into(),
            category: ResultCategory::Clipboard,
            score: 9_900,
            icon: IconSpec::Monogram("CB".into()),
            badge: Some("Ctrl+E".into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "open_workbench".into(),
                label: "打开剪贴板工作台".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "open_workbench".into(),
                    payload: json!({}),
                },
            }],
            preview: None,
        });
    }
    if ":stack".starts_with(&low) || low == ":" {
        out.push(crate::paste_stack::build_stack_command_item());
    }
    if ":pause".starts_with(&low) || ":resume".starts_with(&low) || low == ":" {
        out.push(ResultItem {
            id: "cb:cmd:pause".into(),
            provider_id: PLUGIN_ID.into(),
            title: if capturing {
                "cb :pause — 暂停后台剪贴板采集（隐私临时开关）".into()
            } else {
                "cb :resume — 恢复后台剪贴板采集".into()
            },
            subtitle: "暂停期间新复制内容不会写入历史库".into(),
            category: ResultCategory::Clipboard,
            score: 9_850,
            icon: IconSpec::Monogram(if capturing { "II".into() } else { "▶".into() }),
            badge: Some(if capturing { "ON".into() } else { "PAUSED".into() }),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "toggle_capture".into(),
                label: "切换采集状态".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "toggle_capture".into(),
                    payload: json!({}),
                },
            }],
            preview: None,
        });
    }
    if ":clear".starts_with(&low) || low == ":" {
        out.push(ResultItem {
            id: "cb:cmd:clear".into(),
            provider_id: PLUGIN_ID.into(),
            title: "cb :clear — 清空全部非置顶剪贴板记录".into(),
            subtitle: "保留已置顶（★）记录，清理其余文本与磁盘缓存图片".into(),
            category: ResultCategory::Clipboard,
            score: 9_800,
            icon: IconSpec::Monogram("CLR".into()),
            badge: Some("CLEAN".into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "clear_unpinned".into(),
                label: "清空非置顶记录".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "clear_unpinned".into(),
                    payload: json!({}),
                },
            }],
            preview: None,
        });
    }
    out
}
