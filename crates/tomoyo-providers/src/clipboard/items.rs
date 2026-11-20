//! ResultItem builders for Clipboard Center entries (Text, Image, Files) and quick filter hints.

use super::filter::{normalize_single_line, ClipboardSubtype};
use super::store::{ClipboardEntry, ClipboardPayload};
use chrono::{Local, TimeZone};
use std::path::Path;
use tomoyo_core::Lang;
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, PreviewSpec, ResultCategory, ResultItem,
};

pub(super) fn build_clipboard_item(
    provider_id: &str,
    idx: usize,
    entry: ClipboardEntry,
    lang: Lang,
) -> ResultItem {
    let time_str = Local
        .timestamp_opt(entry.captured_at_unix, 0)
        .single()
        .map(|dt| dt.format("%m-%d %H:%M:%S").to_string())
        .unwrap_or_default();
    let pin_prefix = if entry.pinned { "★ " } else { "" };
    let tag_prefix = if entry.tag.is_empty() {
        String::new()
    } else {
        format!("[#{}] ", entry.tag)
    };
    let src_suffix = if entry.source_app.is_empty() {
        String::new()
    } else {
        format!(" · {}", entry.source_app)
    };
    let note_suffix = if entry.note.is_empty() {
        String::new()
    } else {
        format!(" · {}", entry.note)
    };

    match entry.payload {
        ClipboardPayload::Text { text, char_count, subtype } => build_text_item(
            provider_id, idx, entry.key, entry.preview_line, text, char_count, subtype,
            entry.captured_at_unix, entry.pinned, pin_prefix, &tag_prefix, &time_str,
            &src_suffix, &note_suffix, &entry.tag, &entry.note, &entry.source_app, lang,
        ),
        ClipboardPayload::Image { path, width, height } => build_image_item(
            provider_id, idx, entry.key, entry.preview_line, path, width, height,
            entry.captured_at_unix, entry.pinned, pin_prefix, &tag_prefix, &time_str,
            &src_suffix, &note_suffix, lang,
        ),
        ClipboardPayload::Files { paths } => build_files_item(
            provider_id, idx, entry.key, entry.preview_line, paths,
            entry.captured_at_unix, entry.pinned, pin_prefix, &tag_prefix, &time_str,
            &src_suffix, &note_suffix, lang,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_text_item(
    provider_id: &str,
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
    lang: Lang,
) -> ResultItem {
    let line_count = text.lines().count().max(1);
    let badge = if pinned {
        lang.pick("置顶", "PIN").to_string()
    } else if !tag.is_empty() {
        format!("#{tag}")
    } else {
        subtype.badge_label().to_string()
    };
    let single_line = normalize_single_line(&text);
    let subtitle = match lang {
        Lang::Zh => format!("{time_str}{src_suffix} · {char_count} 字/{line_count} 行{note_suffix} · Shift+Enter 单行贴 · Ctrl+E 工作台"),
        Lang::En => format!("{time_str}{src_suffix} · {char_count} ch/{line_count} ln{note_suffix} · Shift+Enter single-line · Ctrl+E edit"),
    };

    let mut actions = vec![
        ActionSpec {
            id: "paste".into(),
            label: lang.pick("粘贴到原窗口（纯文本）", "Paste Plain Text").into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::CopyText { text: text.clone(), paste_immediately: true },
        },
        ActionSpec {
            id: "copy".into(),
            label: lang.pick("仅复制到剪贴板", "Copy Only").into(),
            shortcut: Some("Ctrl+Enter".into()),
            kind: ActionKind::CopyText { text: text.clone(), paste_immediately: false },
        },
        ActionSpec {
            id: "paste_single".into(),
            label: lang.pick("合并为单行纯文本并粘贴", "Paste as Single Line").into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::CopyText { text: single_line, paste_immediately: true },
        },
    ];
    if subtype == ClipboardSubtype::Link {
        actions.push(ActionSpec {
            id: "open_url".into(),
            label: lang.pick("在浏览器中打开链接", "Open URL in Browser").into(),
            shortcut: None,
            kind: ActionKind::OpenUrl { url: text.trim().to_string() },
        });
    }
    actions.extend(common_manage_actions(&key, pinned, lang));

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
        provider_id: provider_id.into(),
        title: format!("{pin_prefix}{tag_prefix}{preview_line}"),
        subtitle,
        category: ResultCategory::Clipboard,
        score: if pinned { 9_400 } else { 8_500 } - (idx as i64 * 20),
        icon: IconSpec::Monogram(if pinned { "★".into() } else { subtype.badge_label().into() }),
        badge: Some(badge),
        copy_text: Some(text),
        actions,
        preview: Some(PreviewSpec::Text {
            title: match lang {
                Lang::Zh => format!("剪贴板 #{} · {} ({time_str})", idx + 1, subtype.badge_label()),
                Lang::En => format!("Clipboard #{} · {} ({time_str})", idx + 1, subtype.badge_label()),
            },
            body: preview_body,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_image_item(
    provider_id: &str,
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
    lang: Lang,
) -> ResultItem {
    let badge = if pinned { lang.pick("置顶图", "PIN IMG").to_string() } else { "IMG".into() };
    let subtitle = match lang {
        Lang::Zh => format!("{time_str}{src_suffix} · {width}×{height}{note_suffix} · Shift+Enter 一键 OCR · Ctrl+E 工作台"),
        Lang::En => format!("{time_str}{src_suffix} · {width}×{height}{note_suffix} · Shift+Enter OCR · Ctrl+E manage"),
    };
    let mut actions = vec![
        ActionSpec {
            id: "paste_img".into(),
            label: lang.pick("粘贴图片到原窗口", "Paste Image").into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::CopyImage { path: path.clone(), paste_immediately: true },
        },
        ActionSpec {
            id: "copy_img".into(),
            label: lang.pick("仅复制图片到剪贴板", "Copy Image Only").into(),
            shortcut: Some("Ctrl+Enter".into()),
            kind: ActionKind::CopyImage { path: path.clone(), paste_immediately: false },
        },
        ActionSpec {
            id: "ocr_img".into(),
            label: lang.pick("OCR 识别图片中的文字", "Run Offline OCR on Image").into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::FillQuery { query: format!("ocr \"{path}\"") },
        },
        ActionSpec {
            id: "open_img".into(),
            label: lang.pick("用系统看图打开", "Open Image").into(),
            shortcut: None,
            kind: ActionKind::LaunchPath { path: path.clone(), args: None, working_dir: None },
        },
        ActionSpec {
            id: "reveal_img".into(),
            label: lang.pick("在资源管理器定位图片", "Reveal in Explorer").into(),
            shortcut: None,
            kind: ActionKind::RevealInExplorer { path: path.clone() },
        },
    ];
    actions.extend(common_manage_actions(&key, pinned, lang));

    ResultItem {
        id: format!("cb:img:{idx}:{ts}"),
        provider_id: provider_id.into(),
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
    provider_id: &str,
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
    lang: Lang,
) -> ResultItem {
    let joined = paths.join("\r\n");
    let first = paths.first().cloned().unwrap_or_default();
    let dir = Path::new(&first).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| first.clone());
    let mut actions = vec![
        ActionSpec {
            id: "paste_paths".into(),
            label: lang.pick("粘贴文件路径到原窗口", "Paste File Paths").into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::CopyText { text: joined.clone(), paste_immediately: true },
        },
        ActionSpec {
            id: "reveal_first".into(),
            label: lang.pick("在资源管理器定位首个文件", "Reveal First File").into(),
            shortcut: Some("Ctrl+Enter".into()),
            kind: ActionKind::RevealInExplorer { path: first.clone() },
        },
        ActionSpec {
            id: "term_here".into(),
            label: lang.pick("在所在目录打开终端", "Open Terminal Here").into(),
            shortcut: Some("Shift+Enter".into()),
            kind: ActionKind::OpenTerminalAt { dir },
        },
    ];
    actions.extend(common_manage_actions(&key, pinned, lang));

    ResultItem {
        id: format!("cb:files:{idx}:{ts}"),
        provider_id: provider_id.into(),
        title: format!("{pin_prefix}{tag_prefix}{preview_line}"),
        subtitle: format!("{time_str}{src_suffix} · {} 个文件{note_suffix} · Enter 贴路径 · Ctrl+Enter 定位", paths.len()),
        category: ResultCategory::Clipboard,
        score: if pinned { 9_420 } else { 8_510 } - (idx as i64 * 20),
        icon: if first.is_empty() { IconSpec::Monogram("FILE".into()) } else { IconSpec::ShellPath(first) },
        badge: Some(if pinned { lang.pick("置顶", "PIN").into() } else { format!("{} 文件", paths.len()) }),
        copy_text: Some(joined.clone()),
        actions,
        preview: Some(PreviewSpec::Text {
            title: format!("已复制文件清单 ({})", paths.len()),
            body: joined,
        }),
    }
}

fn common_manage_actions(key: &str, pinned: bool, lang: Lang) -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: "edit_workbench".into(),
            label: lang.pick("在剪贴板工作台编辑 / 打标签 / 备注", "Edit / Tag / Note in Workbench").into(),
            shortcut: Some("Ctrl+E".into()),
            kind: ActionKind::InternalCommand {
                command: InternalCommandKind::OpenClipboardManager { focus_key: Some(key.to_string()) },
            },
        },
        ActionSpec {
            id: "pin".into(),
            label: if pinned { lang.pick("取消置顶", "Unpin").into() } else { lang.pick("置顶收藏", "Pin Favorite").into() },
            shortcut: Some("Ctrl+Shift+P".into()),
            kind: ActionKind::InternalCommand {
                command: InternalCommandKind::ToggleClipboardPin { id_key: key.to_string() },
            },
        },
        ActionSpec {
            id: "delete".into(),
            label: lang.pick("删除该条剪贴板记录", "Delete Clipboard Entry").into(),
            shortcut: Some("Ctrl+D".into()),
            kind: ActionKind::InternalCommand {
                command: InternalCommandKind::DeleteClipboardItem { id_key: key.to_string() },
            },
        },
    ]
}

pub(super) fn build_filter_hints(provider_id: &str, tags: &[(String, usize)], lang: Lang) -> Vec<ResultItem> {
    let mut items = Vec::new();
    let presets = [
        ("@pin", "置顶收藏记录", "Pinned favorites"),
        ("@img", "仅图片/截图记录", "Images & screenshots"),
        ("@text", "仅文本记录", "Text entries"),
        ("@link", "仅网址链接", "HTTP(S) links"),
        ("@file", "仅复制的文件/路径", "Copied files & paths"),
        ("@code", "仅代码 / JSON 片段", "Code & JSON snippets"),
        ("@color", "仅色值 (#HEX / rgb)", "Color literals"),
    ];
    for (i, (tok, zh, en)) in presets.iter().enumerate() {
        items.push(ResultItem {
            id: format!("cb:hint:{tok}"),
            provider_id: provider_id.into(),
            title: format!("cb {tok} — {}", lang.pick(zh, en)),
            subtitle: lang.pick("回车填入筛选前缀，可继续叠加关键词搜索", "Enter to apply filter chip").into(),
            category: ResultCategory::Clipboard,
            score: 9_800 - (i as i64 * 10),
            icon: IconSpec::Monogram("FLT".into()),
            badge: Some((*tok).into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "fill".into(),
                label: lang.pick("应用筛选", "Apply Filter").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::FillQuery { query: format!("cb {tok} ") },
            }],
            preview: None,
        });
    }
    for (i, (tag, count)) in tags.iter().take(10).enumerate() {
        items.push(ResultItem {
            id: format!("cb:tag:{tag}"),
            provider_id: provider_id.into(),
            title: format!("cb #{tag} — 标签分组（{count} 条）"),
            subtitle: lang.pick("回车按自定义标签过滤剪贴板历史", "Filter clipboard by custom tag").into(),
            category: ResultCategory::Clipboard,
            score: 9_650 - (i as i64 * 10),
            icon: IconSpec::Monogram("TAG".into()),
            badge: Some(format!("{count}")),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "fill_tag".into(),
                label: lang.pick("按标签筛选", "Filter Tag").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::FillQuery { query: format!("cb #{tag} ") },
            }],
            preview: None,
        });
    }
    items
}
