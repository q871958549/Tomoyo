//! Event-driven, strictly memory-bounded SQLite Text + Image Clipboard History provider (`cb`).

mod store;

use chrono::{Local, TimeZone};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use store::{ClipboardPayload, ClipboardStore};
use tomoyo_core::{Lang, Provider, TomoyoConfig};
use tomoyo_protocol::{
    ActionKind, ActionSpec, ClipboardSnapshot, IconSpec, InternalCommandKind, PreviewSpec,
    QueryContext, ResultCategory, ResultItem,
};

pub struct ClipboardProvider {
    store: ClipboardStore,
    config: Arc<RwLock<TomoyoConfig>>,
}

impl ClipboardProvider {
    pub fn with_sqlite(
        db_path: Option<PathBuf>,
        max_persisted: usize,
        config: Arc<RwLock<TomoyoConfig>>,
    ) -> Self {
        Self {
            store: ClipboardStore::open(db_path, max_persisted),
            config,
        }
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }

    pub fn push_text(&self, raw: String) {
        self.store.push_text(raw);
    }

    pub fn push_image(&self, path: String, width: u32, height: u32, digest_key: String) {
        self.store.push_image(path, width, height, digest_key);
    }

    pub fn toggle_pin(&self, id_key: &str) -> bool {
        self.store.toggle_pin(id_key)
    }

    pub fn item_count(&self) -> usize {
        self.store.item_count()
    }

    pub fn shutdown_checkpoint(&self) {
        self.store.shutdown_checkpoint();
    }

    pub fn latest_snapshot(&self) -> Option<ClipboardSnapshot> {
        let guard = self.store.history.read().ok()?;
        for entry in guard.iter() {
            if let ClipboardPayload::Text { text, .. } = &entry.payload {
                return Some(ClipboardSnapshot {
                    text: text.clone(),
                    captured_at_unix: entry.captured_at_unix,
                });
            }
        }
        None
    }
}

impl Provider for ClipboardProvider {
    fn id(&self) -> &'static str {
        "builtin.clipboard"
    }

    fn name(&self) -> &'static str {
        "Clipboard History (SQLite Text + Image)"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["cb"]
    }

    fn supports_global(&self) -> bool {
        false
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let lang = self.lang();
        let filter = ctx.args.trim().to_lowercase();
        let entries = self.store.query_entries(&filter, ctx.limit.min(30));

        if entries.is_empty() {
            return vec![empty_clipboard_item(self.id(), &filter, lang)];
        }

        entries
            .into_iter()
            .enumerate()
            .map(|(idx, e)| {
                let time_str = Local
                    .timestamp_opt(e.captured_at_unix, 0)
                    .single()
                    .map(|dt| dt.format("%m-%d %H:%M:%S").to_string())
                    .unwrap_or_default();
                let pin_prefix = if e.pinned { "★ " } else { "" };

                match e.payload {
                    ClipboardPayload::Text { text, char_count } => build_text_item(
                        self.id(),
                        idx,
                        e.key,
                        e.preview_line,
                        text,
                        char_count,
                        e.captured_at_unix,
                        e.pinned,
                        pin_prefix,
                        &time_str,
                        lang,
                    ),
                    ClipboardPayload::Image { path, width, height } => build_image_item(
                        self.id(),
                        idx,
                        e.key,
                        e.preview_line,
                        path,
                        width,
                        height,
                        e.captured_at_unix,
                        e.pinned,
                        pin_prefix,
                        &time_str,
                        lang,
                    ),
                }
            })
            .collect()
    }

    fn trim_memory(&self) {
        self.store.trim_memory();
    }
}

fn empty_clipboard_item(provider_id: &str, filter: &str, lang: Lang) -> ResultItem {
    ResultItem {
        id: "cb:empty".into(),
        provider_id: provider_id.into(),
        title: if filter.is_empty() {
            lang.pick("剪贴板历史为空", "Clipboard history is empty").into()
        } else {
            match lang {
                Lang::Zh => format!("未找到包含「{filter}」的剪贴板记录"),
                Lang::En => format!("No clipboard entries matching `{filter}`"),
            }
        },
        subtitle: lang
            .pick(
                "支持文本与截图图片 · 磁盘缩略图 LRU（零图片常驻内存）",
                "Supports text & images · Disk thumbnail LRU (0 image RAM)",
            )
            .into(),
        category: ResultCategory::Clipboard,
        score: 5_000,
        icon: IconSpec::Monogram("CB".into()),
        badge: Some("0".into()),
        copy_text: None,
        actions: vec![],
        preview: None,
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
    ts: i64,
    pinned: bool,
    pin_prefix: &str,
    time_str: &str,
    lang: Lang,
) -> ResultItem {
    let line_count = text.lines().count().max(1);
    let badge = if pinned {
        lang.pick("置顶", "PIN").to_string()
    } else {
        format!("#{}", idx + 1)
    };
    let subtitle = match lang {
        Lang::Zh => format!("{time_str} · {char_count} 字符 · {line_count} 行 · 回车直贴原窗口"),
        Lang::En => format!("{time_str} · {char_count} chars · {line_count} lines · Enter to paste"),
    };
    ResultItem {
        id: format!("cb:{idx}:{ts}"),
        provider_id: provider_id.into(),
        title: format!("{pin_prefix}{preview_line}"),
        subtitle,
        category: ResultCategory::Clipboard,
        score: if pinned { 9_400 } else { 8_500 } - (idx as i64 * 20),
        icon: IconSpec::Monogram(if pinned { "★".into() } else { "CB".into() }),
        badge: Some(badge),
        copy_text: Some(text.clone()),
        actions: vec![
            ActionSpec {
                id: "paste".into(),
                label: lang.pick("粘贴到原焦点窗口", "Paste into Active App").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText {
                    text: text.clone(),
                    paste_immediately: true,
                },
            },
            ActionSpec {
                id: "copy".into(),
                label: lang.pick("仅复制到剪贴板", "Copy Only").into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText {
                    text: text.clone(),
                    paste_immediately: false,
                },
            },
            ActionSpec {
                id: "pin".into(),
                label: if pinned {
                    lang.pick("取消置顶", "Unpin").into()
                } else {
                    lang.pick("置顶收藏", "Pin Favorite").into()
                },
                shortcut: None,
                kind: ActionKind::InternalCommand {
                    command: InternalCommandKind::ToggleClipboardPin { id_key: key },
                },
            },
        ],
        preview: Some(PreviewSpec::Text {
            title: match lang {
                Lang::Zh => format!("文本剪贴板 #{} ({time_str})", idx + 1),
                Lang::En => format!("Clipboard #{} ({time_str})", idx + 1),
            },
            body: text,
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
    time_str: &str,
    lang: Lang,
) -> ResultItem {
    let badge = if pinned {
        lang.pick("置顶图", "PIN IMG").to_string()
    } else {
        lang.pick("图片", "IMG").to_string()
    };
    let subtitle = match lang {
        Lang::Zh => format!("{time_str} · {width}×{height} 截图/图片 · 回车直接贴图到原窗口"),
        Lang::En => format!("{time_str} · {width}×{height} Image · Enter to paste image"),
    };
    ResultItem {
        id: format!("cb:img:{idx}:{ts}"),
        provider_id: provider_id.into(),
        title: format!("{pin_prefix}{preview_line}"),
        subtitle,
        category: ResultCategory::Clipboard,
        score: if pinned { 9_450 } else { 8_520 } - (idx as i64 * 20),
        icon: IconSpec::Monogram("IMG".into()),
        badge: Some(badge),
        copy_text: Some(path.clone()),
        actions: vec![
            ActionSpec {
                id: "paste_img".into(),
                label: lang.pick("粘贴图片到原焦点窗口", "Paste Image into Active App").into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyImage {
                    path: path.clone(),
                    paste_immediately: true,
                },
            },
            ActionSpec {
                id: "copy_img".into(),
                label: lang.pick("仅复制图片到剪贴板", "Copy Image Only").into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyImage {
                    path: path.clone(),
                    paste_immediately: false,
                },
            },
            ActionSpec {
                id: "open_img".into(),
                label: lang.pick("用系统看图打开", "Open Image").into(),
                shortcut: None,
                kind: ActionKind::LaunchPath {
                    path: path.clone(),
                    args: None,
                    working_dir: None,
                },
            },
            ActionSpec {
                id: "reveal_img".into(),
                label: lang.pick("在资源管理器定位图片", "Reveal PNG in Explorer").into(),
                shortcut: None,
                kind: ActionKind::RevealInExplorer { path: path.clone() },
            },
            ActionSpec {
                id: "pin_img".into(),
                label: if pinned {
                    lang.pick("取消置顶", "Unpin").into()
                } else {
                    lang.pick("置顶收藏", "Pin Favorite").into()
                },
                shortcut: None,
                kind: ActionKind::InternalCommand {
                    command: InternalCommandKind::ToggleClipboardPin { id_key: key },
                },
            },
        ],
        preview: Some(PreviewSpec::ImagePreview { path, width, height }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_clipboard_persists_text_and_images_and_pins() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_clip_v5_{}.db", std::process::id()));
        let img_tmp = std::env::temp_dir().join(format!("tomoyo_clip_img_{}.png", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::write(&img_tmp, b"fake_png_bytes");
        let cfg = Arc::new(RwLock::new(TomoyoConfig::default()));

        {
            let cb = ClipboardProvider::with_sqlite(Some(tmp.clone()), 100, cfg.clone());
            cb.push_text("hello tomoyo rust".into());
            cb.push_image(img_tmp.to_string_lossy().into_owned(), 640, 480, "abc123".into());
            assert!(cb.toggle_pin("hello tomoyo rust"));
            assert_eq!(cb.item_count(), 2);
            let ctx = QueryContext::new("cb", 1, 10).with_keyword("cb", "");
            let items = cb.query(&ctx);
            assert_eq!(items.len(), 2);
            assert!(items[0].title.contains("★"));
            assert!(items[1].title.contains("640×480"));
            cb.trim_memory();
        }

        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&img_tmp);
        let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
        let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    }
}
