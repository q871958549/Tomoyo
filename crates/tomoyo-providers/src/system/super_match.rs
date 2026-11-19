//! Super Match attached-capsule actions for pasted Images (`CF_DIB`) and Files (`CF_HDROP`).
//!
//! Anti-Bloat Invariant: ZERO large binary reads or Base64 allocations during `query()`.
//! Heavy conversions (`CopyImageAsBase64`) execute lazily only when the user hits `Enter`.

use sha2::{Digest, Sha256};
use std::path::Path;
use tomoyo_core::Lang;
use tomoyo_protocol::{
    ActionKind, ActionSpec, AttachedContext, IconSpec, InternalCommandKind, PreviewSpec,
    ResultCategory, ResultItem,
};

pub fn build_super_match_items(
    provider_id: &str,
    attached: &AttachedContext,
    filter: &str,
    lang: Lang,
) -> Vec<ResultItem> {
    let mut items = Vec::new();
    let f = filter.to_lowercase();

    match attached {
        AttachedContext::Image {
            path,
            width,
            height,
        } => {
            let file_kb = std::fs::metadata(path)
                .map(|m| m.len() / 1024)
                .unwrap_or(0);

            items.push(ResultItem {
                id: "super:img:open".into(),
                provider_id: provider_id.into(),
                title: lang.pick("打开粘贴的图片", "Open Pasted Image").into(),
                subtitle: format!("{width}×{height} PNG · {file_kb} KB · {path}"),
                category: ResultCategory::File,
                score: 10_500,
                icon: IconSpec::Monogram("IMG".into()),
                badge: Some(format!("{width}×{height}")),
                copy_text: Some(path.clone()),
                actions: vec![
                    ActionSpec {
                        id: "open".into(),
                        label: lang.pick("用系统看图打开", "Open Image").into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::LaunchPath {
                            path: path.clone(),
                            args: None,
                            working_dir: None,
                        },
                    },
                    ActionSpec {
                        id: "reveal".into(),
                        label: lang.pick("在资源管理器定位", "Reveal in Explorer").into(),
                        shortcut: None,
                        kind: ActionKind::RevealInExplorer { path: path.clone() },
                    },
                ],
                preview: Some(PreviewSpec::ImagePreview {
                    path: path.clone(),
                    width: *width,
                    height: *height,
                }),
            });

            items.push(ResultItem {
                id: "super:img:b64".into(),
                provider_id: provider_id.into(),
                title: lang
                    .pick("复制图片为 Base64 Data URI", "Copy Image as Base64 Data URI")
                    .into(),
                subtitle: lang
                    .pick(
                        "按回车按需编码 data:image/png;base64,...（零搜索期内存开销）",
                        "Encodes data:image/png;base64,... lazily on Enter (0 query RAM)",
                    )
                    .into(),
                category: ResultCategory::Calc,
                score: 10_400,
                icon: IconSpec::Monogram("B64".into()),
                badge: Some("BASE64".into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: "copy_b64".into(),
                    label: lang.pick("编码并复制 Base64", "Encode & Copy Base64").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::CopyImageAsBase64 { path: path.clone() },
                    },
                }],
                preview: Some(PreviewSpec::ImagePreview {
                    path: path.clone(),
                    width: *width,
                    height: *height,
                }),
            });

            items.push(ResultItem {
                id: "super:img:reveal".into(),
                provider_id: provider_id.into(),
                title: lang
                    .pick("在资源管理器中定位该图片文件", "Reveal Image File in Explorer")
                    .into(),
                subtitle: path.clone(),
                category: ResultCategory::Folder,
                score: 10_300,
                icon: IconSpec::Monogram("DIR".into()),
                badge: Some("PNG".into()),
                copy_text: Some(path.clone()),
                actions: vec![ActionSpec {
                    id: "reveal".into(),
                    label: lang.pick("在资源管理器定位", "Reveal in Explorer").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::RevealInExplorer { path: path.clone() },
                }],
                preview: Some(PreviewSpec::ImagePreview {
                    path: path.clone(),
                    width: *width,
                    height: *height,
                }),
            });
        }
        AttachedContext::Files { paths } => {
            if let Some(first) = paths.first() {
                let joined = paths.join("\n");
                let parent = Path::new(first)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| first.clone());

                items.push(ResultItem {
                    id: "super:files:copy_paths".into(),
                    provider_id: provider_id.into(),
                    title: match lang {
                        Lang::Zh => format!("复制已粘贴的 {} 个文件路径", paths.len()),
                        Lang::En => format!("Copy {} Pasted File Path(s)", paths.len()),
                    },
                    subtitle: first.clone(),
                    category: ResultCategory::File,
                    score: 10_500,
                    icon: IconSpec::ShellPath(first.clone()),
                    badge: Some(format!("{} files", paths.len())),
                    copy_text: Some(joined.clone()),
                    actions: vec![ActionSpec {
                        id: "copy".into(),
                        label: lang.pick("复制全部路径", "Copy All Paths").into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::CopyText {
                            text: joined,
                            paste_immediately: false,
                        },
                    }],
                    preview: Some(PreviewSpec::FilePreview { path: first.clone() }),
                });

                if Path::new(first).is_file() {
                    if let Ok(meta) = std::fs::metadata(first) {
                        if meta.len() <= 8 * 1024 * 1024 {
                            if let Ok(data) = std::fs::read(first) {
                                let digest: String = Sha256::digest(&data)
                                    .iter()
                                    .map(|b| format!("{b:02x}"))
                                    .collect();
                                items.push(ResultItem {
                                    id: "super:files:sha256".into(),
                                    provider_id: provider_id.into(),
                                    title: format!("SHA-256: {digest}"),
                                    subtitle: lang
                                        .pick(
                                            "已自动计算文件校验和 · 回车复制",
                                            "File SHA-256 checksum · Enter to copy",
                                        )
                                        .into(),
                                    category: ResultCategory::Calc,
                                    score: 10_400,
                                    icon: IconSpec::Monogram("SHA".into()),
                                    badge: Some("SHA256".into()),
                                    copy_text: Some(digest.clone()),
                                    actions: vec![ActionSpec {
                                        id: "copy_sha".into(),
                                        label: lang.pick("复制 SHA-256", "Copy SHA-256").into(),
                                        shortcut: Some("Enter".into()),
                                        kind: ActionKind::CopyText {
                                            text: digest.clone(),
                                            paste_immediately: false,
                                        },
                                    }],
                                    preview: Some(PreviewSpec::KeyValue {
                                        title: "文件校验信息".into(),
                                        entries: vec![
                                            ("文件".into(), first.clone()),
                                            ("大小".into(), format!("{} B", meta.len())),
                                            ("SHA-256".into(), digest),
                                        ],
                                    }),
                                });
                            }
                        }
                    }
                }

                items.push(ResultItem {
                    id: "super:files:terminal".into(),
                    provider_id: provider_id.into(),
                    title: lang
                        .pick("在文件所在目录打开终端", "Open Terminal at File Directory")
                        .into(),
                    subtitle: parent.clone(),
                    category: ResultCategory::Folder,
                    score: 10_300,
                    icon: IconSpec::Monogram("CMD".into()),
                    badge: Some("TERM".into()),
                    copy_text: Some(parent.clone()),
                    actions: vec![ActionSpec {
                        id: "term".into(),
                        label: lang.pick("打开终端", "Open Terminal").into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::OpenTerminalAt { dir: parent },
                    }],
                    preview: None,
                });
            }
        }
    }

    items.push(ResultItem {
        id: "super:clear".into(),
        provider_id: provider_id.into(),
        title: lang
            .pick(
                "清除当前粘贴胶囊（或按 Backspace）",
                "Clear Attached Capsule (or press Backspace)",
            )
            .into(),
        subtitle: lang.pick("返回普通搜索模式", "Return to normal search mode").into(),
        category: ResultCategory::System,
        score: 9_900,
        icon: IconSpec::Monogram("CLR".into()),
        badge: Some("ESC".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "clear".into(),
            label: lang.pick("清除粘贴胶囊", "Clear Capsule").into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::InternalCommand {
                command: InternalCommandKind::ClearAttachedContext,
            },
        }],
        preview: None,
    });

    if !f.is_empty() {
        items.retain(|it| {
            it.title.to_lowercase().contains(&f) || it.subtitle.to_lowercase().contains(&f)
        });
    }
    items
}
