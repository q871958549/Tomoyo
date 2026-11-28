use crate::PLUGIN_ID;
use serde_json::json;
use tomoyo_sdk::{ActionKind, ActionSpec, IconSpec, ResultCategory, ResultItem};

pub fn build_filter_hints(tags: &[(String, usize)]) -> Vec<ResultItem> {
    let mut items = Vec::new();
    let presets = [
        ("@pin", "置顶收藏记录"),
        ("@img", "仅图片/截图记录"),
        ("@text", "仅文本记录"),
        ("@link", "仅网址链接"),
        ("@file", "仅复制的文件/路径"),
        ("@code", "仅代码 / JSON 片段"),
        ("@color", "仅色值 (#HEX / rgb)"),
    ];
    for (i, (tok, zh)) in presets.iter().enumerate() {
        items.push(ResultItem {
            id: format!("cb:hint:{tok}"),
            provider_id: PLUGIN_ID.into(),
            title: format!("cb {tok} — {zh}"),
            subtitle: "回车填入筛选前缀，可继续叠加关键词搜索".into(),
            category: ResultCategory::Clipboard,
            score: 9_800 - (i as i64 * 10),
            icon: IconSpec::Monogram("FLT".into()),
            badge: Some((*tok).into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "fill".into(),
                label: "应用筛选".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::FillQuery { query: format!("cb {tok} ") },
            }],
            preview: None,
        });
    }
    for (i, (tag, count)) in tags.iter().take(10).enumerate() {
        items.push(ResultItem {
            id: format!("cb:tag:{tag}"),
            provider_id: PLUGIN_ID.into(),
            title: format!("cb #{tag} — 标签分组（{count} 条）"),
            subtitle: "回车按自定义标签过滤剪贴板历史".into(),
            category: ResultCategory::Clipboard,
            score: 9_650 - (i as i64 * 10),
            icon: IconSpec::Monogram("TAG".into()),
            badge: Some(format!("{count}")),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "fill_tag".into(),
                label: "按标签筛选".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::FillQuery { query: format!("cb #{tag} ") },
            }],
            preview: None,
        });
    }
    items
}

pub fn empty_clipboard_item(filter: &str) -> ResultItem {
    ResultItem {
        id: "cb:empty".into(),
        provider_id: PLUGIN_ID.into(),
        title: if filter.is_empty() {
            "剪贴板历史为空".into()
        } else {
            format!("未找到匹配「{filter}」的剪贴板记录")
        },
        subtitle: "支持 @pin / @img / @link / @file / @code / #标签 筛选，或回车打开工作台".into(),
        category: ResultCategory::Clipboard,
        score: 5_000,
        icon: IconSpec::Monogram("CB".into()),
        badge: Some("0".into()),
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
    }
}
