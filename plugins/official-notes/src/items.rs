//! ResultItem builders for Quick Notes inline launcher entries.

use crate::store::NoteDoc;
use crate::PLUGIN_ID;
use tomoyo_sdk::{
    ActionKind, ActionSpec, IconSpec, PreviewSpec, ResultCategory, ResultItem,
};

pub(crate) fn build_open_workbench_item(total: usize, open_todos: usize, score: i64) -> ResultItem {
    let (title, subtitle, preview_body) = if total == 0 {
        (
            "打开本地备忘录工作台 · 立即记录第一条笔记".into(),
            "纯本地 .md + YAML 存储（双机 sync_dir 同步）· 回车直开独立窗口 · `note + 标题` 秒记".into(),
            "# 本地快速备忘录 2.0\n\n- 按 Enter 直开独立多栏工作台（支持置顶便签、待办 `- [ ]`、#标签与 `[[双向链接]]`）\n- 搜索框输入 `note + 标题 :: 内容` 或 `note clip` 可免开窗极速落盘\n- 输入 `note today` 一键打开今日速记，长文可随时交付 `Quiet.md` 精编".into(),
        )
    } else {
        (
            format!("打开本地备忘录独立工作台（共 {total} 条 · {open_todos} 项待办）"),
            "多栏编辑 / 实时预览 / 卡片置顶 / `[[双链]]` 反链 · 回车开启独立窗口".into(),
            format!("当前共 {total} 条本地 Markdown 备忘录（{open_todos} 项未完成待办）。\n按 Enter 开启独立备忘录工作台，或输入 `note today` 记录今日速记。"),
        )
    };
    ResultItem {
        id: "note:workbench".into(),
        provider_id: PLUGIN_ID.into(),
        title,
        subtitle,
        category: ResultCategory::Plugin,
        score,
        icon: IconSpec::Monogram("WB".into()),
        badge: Some("独立窗口".into()),
        copy_text: None,
        actions: vec![
            ActionSpec {
                id: "open_workbench".into(),
                label: "打开备忘录独立工作台".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "open_workbench".into(),
                    payload: serde_json::json!({ "focus_file": "" }),
                },
            },
            ActionSpec {
                id: "open_daily".into(),
                label: "打开今日速记（Daily Scratchpad）".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "open_workbench".into(),
                    payload: serde_json::json!({ "daily": true }),
                },
            },
        ],
        preview: Some(PreviewSpec::Text { title: "Quick Notes 独立工作台".into(), body: preview_body }),
    }
}

pub(crate) fn build_daily_item() -> ResultItem {
    ResultItem {
        id: "note:daily".into(),
        provider_id: PLUGIN_ID.into(),
        title: "打开今日速记 (Daily Scratchpad)".into(),
        subtitle: "自动创建/定位今天日期的 Markdown 速记并开启工作台".into(),
        category: ResultCategory::Plugin,
        score: 9_980,
        icon: IconSpec::Monogram("日".into()),
        badge: Some("TODAY".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "open_daily".into(),
            label: "打开今日速记".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "open_workbench".into(),
                payload: serde_json::json!({ "daily": true }),
            },
        }],
        preview: None,
    }
}

pub(crate) fn build_create_item(title: String, content: String, dir_disp: String) -> ResultItem {
    ResultItem {
        id: "note:create".into(),
        provider_id: PLUGIN_ID.into(),
        title: format!("新建备忘录：{title}"),
        subtitle: format!("保存为纯本地 Markdown ({dir_disp}) · 回车立即创建"),
        category: ResultCategory::Plugin,
        score: 9_950,
        icon: IconSpec::Monogram("+".into()),
        badge: Some("NEW".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "create".into(),
            label: "创建并留在列表".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "create".into(),
                payload: serde_json::json!({ "title": title, "content": content }),
            },
        }],
        preview: Some(PreviewSpec::Text {
            title: format!("即将创建：{title}"),
            body: format!("# {title}\n\n{content}"),
        }),
    }
}

pub(crate) fn build_clip_item(title: String, body: String) -> ResultItem {
    ResultItem {
        id: "note:from_clip".into(),
        provider_id: PLUGIN_ID.into(),
        title: format!("将剪贴板保存为备忘：{title}"),
        subtitle: "按回车将当前剪贴板文本落盘为本地 Markdown".into(),
        category: ResultCategory::Plugin,
        score: 9_900,
        icon: IconSpec::Monogram("CB".into()),
        badge: Some("CLIP→MD".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "create".into(),
            label: "保存为本地 .md 备忘".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "create".into(),
                payload: serde_json::json!({ "title": title, "content": body }),
            },
        }],
        preview: Some(PreviewSpec::Text { title, body }),
    }
}

pub(crate) fn build_quick_create_item(args: &str, content: String, dir_disp: String) -> ResultItem {
    ResultItem {
        id: "note:create_quick".into(),
        provider_id: PLUGIN_ID.into(),
        title: format!("未找到「{args}」· 按回车直接新建为备忘录"),
        subtitle: format!("存储目录：{dir_disp}（输入 `note today` 开启今日速记）"),
        category: ResultCategory::Plugin,
        score: 8_500,
        icon: IconSpec::Monogram("+".into()),
        badge: Some("NEW".into()),
        copy_text: None,
        actions: vec![
            ActionSpec {
                id: "create_quick".into(),
                label: "立即创建为本地备忘录".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "create".into(),
                    payload: serde_json::json!({ "title": args, "content": content }),
                },
            },
            ActionSpec {
                id: "open_workbench".into(),
                label: "打开备忘录独立工作台".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "open_workbench".into(),
                    payload: serde_json::json!({ "focus_file": "" }),
                },
            },
        ],
        preview: None,
    }
}

pub(crate) fn build_cli_hint_item() -> ResultItem {
    ResultItem {
        id: "note:cli_hint".into(),
        provider_id: PLUGIN_ID.into(),
        title: "极速命令新建 · `note + 标题 :: 内容` 或 `note clip`".into(),
        subtitle: "免开窗口直接落盘，也支持超级面板划词一键存为备忘".into(),
        category: ResultCategory::Plugin,
        score: 8_200,
        icon: IconSpec::Monogram("+".into()),
        badge: Some("TIP".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "fill_create".into(),
            label: "填充快捷新建格式".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::FillQuery { query: "note + ".into() },
        }],
        preview: None,
    }
}

pub(crate) fn build_note_item(doc: &NoteDoc, score: i64) -> ResultItem {
    let path_str = doc.path.to_string_lossy().into_owned();
    let pin_prefix = if doc.pinned { "★ " } else { "" };
    let todo_part = if doc.todo_open > 0 || doc.todo_done > 0 {
        format!("☑ {}/{} · ", doc.todo_done, doc.todo_open + doc.todo_done)
    } else {
        String::new()
    };
    let tag_str = if doc.tags.is_empty() { String::new() } else { format!(" · {}", doc.tags.join(" ")) };
    let badge = if doc.pinned {
        "置顶".into()
    } else if doc.todo_open > 0 {
        format!("待办 {}", doc.todo_open)
    } else {
        "NOTE".into()
    };

    ResultItem {
        id: format!("note:{}", doc.file_name),
        provider_id: PLUGIN_ID.into(),
        title: format!("{pin_prefix}{}", doc.title),
        subtitle: format!("{todo_part}{}{tag_str}", doc.snippet),
        category: ResultCategory::Plugin,
        score,
        icon: IconSpec::Monogram(if doc.pinned { "★".into() } else { "MD".into() }),
        badge: Some(badge),
        copy_text: Some(doc.body.clone()),
        actions: vec![
            ActionSpec {
                id: "open_workbench".into(),
                label: "在独立备忘录窗口打开并编辑".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "open_workbench".into(),
                    payload: serde_json::json!({ "focus_file": doc.file_name }),
                },
            },
            ActionSpec {
                id: "paste_back".into(),
                label: "复制正文并贴回原窗口".into(),
                shortcut: Some("Shift+Enter".into()),
                kind: ActionKind::CopyText { text: doc.body.clone(), paste_immediately: true },
            },
            ActionSpec {
                id: "copy".into(),
                label: "仅复制 Markdown 内容".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText { text: doc.body.clone(), paste_immediately: false },
            },
            ActionSpec {
                id: "edit_in_md".into(),
                label: "交付 Quiet.md 编辑器精编排版".into(),
                shortcut: None,
                kind: ActionKind::FillQuery { query: format!("md \"{path_str}\"") },
            },
            ActionSpec {
                id: "toggle_pin".into(),
                label: if doc.pinned { "取消置顶".into() } else { "置顶备忘录".into() },
                shortcut: None,
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "toggle_pin".into(),
                    payload: serde_json::json!({ "file_name": doc.file_name }),
                },
            },
            ActionSpec {
                id: "reveal".into(),
                label: "在资源管理器中定位".into(),
                shortcut: None,
                kind: ActionKind::RevealInExplorer { path: path_str },
            },
            ActionSpec {
                id: "archive".into(),
                label: "归档该备忘录".into(),
                shortcut: None,
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "archive".into(),
                    payload: serde_json::json!({ "file_name": doc.file_name }),
                },
            },
            ActionSpec {
                id: "delete".into(),
                label: "永久删除备忘录".into(),
                shortcut: None,
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "delete".into(),
                    payload: serde_json::json!({ "file_name": doc.file_name, "archived": doc.archived }),
                },
            },
        ],
        preview: Some(PreviewSpec::Text { title: doc.title.clone(), body: doc.body.clone() }),
    }
}
