//! Zero-friction `?` / `/` command & shortcut discovery hub.

use tomoyo_core::{score_match, IndexedText, Lang};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, PreviewSpec, ResultCategory, ResultItem,
};

struct HubEntry {
    id: &'static str,
    title_zh: &'static str,
    title_en: &'static str,
    sub_zh: &'static str,
    sub_en: &'static str,
    badge: &'static str,
    aliases: &'static str,
    action: ActionKind,
}

pub(super) fn is_command_hub_query(trimmed: &str) -> bool {
    trimmed.starts_with('?')
        || trimmed.starts_with('？')
        || (trimmed.starts_with('/') && !trimmed[1..].contains('/'))
}

fn cheat_sheet_preview(lang: Lang) -> PreviewSpec {
    let zh = lang == Lang::Zh;
    PreviewSpec::KeyValue {
        title: if zh { "Tomoyo 快捷导航速查" } else { "Tomoyo Quick Cheat Sheet" }.into(),
        entries: vec![
            ("Alt + Space".into(), if zh { "呼出 / 收起启动器" } else { "Toggle Launcher" }.into()),
            ("Ctrl + Tab".into(), if zh { "切换文件分类标签" } else { "Switch Filter Tag" }.into()),
            ("Ctrl + P".into(), if zh { "开关右侧预览面板" } else { "Toggle Preview Pane" }.into()),
            ("Tab".into(), if zh { "展开更多文件/条目动作" } else { "Open Action Menu" }.into()),
            ("Ctrl + ,".into(), if zh { "打开 Tomoyo 设置中心" } else { "Open Settings" }.into()),
            ("?  或  /".into(), if zh { "随时查看本指令速查" } else { "Open Command Hub" }.into()),
        ],
    }
}

pub(super) fn build_command_hub_items(
    provider_id: &str,
    trimmed: &str,
    lang: Lang,
) -> Vec<ResultItem> {
    let zh = lang == Lang::Zh;
    let kw = trimmed
        .trim_start_matches(['?', '？', '/'])
        .trim()
        .to_lowercase();
    let preview = cheat_sheet_preview(lang);

    let entries = [
        HubEntry {
            id: "hub:file",
            title_zh: "f  ·  自然语言文件搜索",
            title_en: "f  ·  Natural-Language File Search",
            sub_zh: "支持直接说人话：f 今天 pdf · f 大文件 · f 近7天 合同 · f 下载目录",
            sub_en: "Try: f today pdf · f large · f last7days report · f downloads",
            badge: "f ",
            aliases: "file everything search 文件 搜索 今天 大文件",
            action: ActionKind::FillQuery { query: "f ".into() },
        },
        HubEntry {
            id: "hub:plugins",
            title_zh: "插件  ·  搜索已装插件并直达设置",
            title_en: "plugins  ·  Search Plugins & Jump to Settings",
            sub_zh: "输入「插件」或插件名（如 翻译 / 截图 / OCR），回车直接打开对应设置页",
            sub_en: "Type plugin name to inspect or jump straight to its Settings panel",
            badge: "PLG",
            aliases: "plugin 插件 设置 扩展",
            action: ActionKind::FillQuery { query: "插件 ".into() },
        },
        HubEntry {
            id: "hub:clipboard",
            title_zh: "cb  ·  剪贴板历史与队列粘贴",
            title_en: "cb  ·  Clipboard History & Paste Stack",
            sub_zh: "检索文本/图片历史；输入 cb :stack 管理顺序粘贴队列",
            sub_en: "Search clipboard history; type cb :stack for Paste Stack",
            badge: "cb",
            aliases: "clipboard 剪贴板 粘贴 stack",
            action: ActionKind::FillQuery { query: "cb ".into() },
        },
        HubEntry {
            id: "hub:capture",
            title_zh: "cap / pin  ·  截图、标注与桌面贴图",
            title_en: "cap / pin  ·  Screenshot & Desktop Pin",
            sub_zh: "快捷触发智能窗体磁吸截图，或将剪贴板内容贴图置顶桌面",
            sub_en: "Launch smart snap capture or pin clipboard items to desktop",
            badge: "cap",
            aliases: "capture screenshot pin 截图 贴图 标注",
            action: ActionKind::FillQuery { query: "cap".into() },
        },
        HubEntry {
            id: "hub:translate",
            title_zh: "fy  ·  多源翻译与领域润色",
            title_en: "fy  ·  Multi-Engine Translation",
            sub_zh: "例：fy hello world · 支持标准 / 学术 / 代码 / 商务预设与 OCR 取词",
            sub_en: "Example: fy hello world · Supports Standard/Academic/Code presets",
            badge: "fy",
            aliases: "translate 翻译 fy",
            action: ActionKind::FillQuery { query: "fy ".into() },
        },
        HubEntry {
            id: "hub:notes",
            title_zh: "note  ·  本地 Markdown 备忘录",
            title_en: "note  ·  Local Markdown Notes",
            sub_zh: "检索已有笔记，或输入 note <内容> 秒速记一条并自动落盘",
            sub_en: "Search saved notes or type note <text> to capture immediately",
            badge: "note",
            aliases: "note markdown 备忘录 笔记",
            action: ActionKind::FillQuery { query: "note ".into() },
        },
        HubEntry {
            id: "hub:port",
            title_zh: "port / kill  ·  端口查杀与进程管理",
            title_en: "port / kill  ·  TCP Ports & Processes",
            sub_zh: "输入 port 3000 查端口占用并回车结束；kill <进程> 快速终止进程",
            sub_en: "Type port 3000 to locate listening PIDs or kill <proc>",
            badge: "port",
            aliases: "port kill process 端口 进程 查杀",
            action: ActionKind::FillQuery { query: "port ".into() },
        },
        HubEntry {
            id: "hub:flow",
            title_zh: "flow  ·  轻量工作流串联",
            title_en: "flow  ·  Lightweight Flows",
            sub_zh: "一键同时打开应用、文件夹、网址或执行预设脚本动作",
            sub_en: "Trigger multi-step app, folder, URL, or script workflows",
            badge: "flow",
            aliases: "flow workflow 工作流 自动化",
            action: ActionKind::FillQuery { query: "flow ".into() },
        },
        HubEntry {
            id: "hub:windows",
            title_zh: "w  ·  极速窗口切换",
            title_en: "w  ·  Instant Window Switcher",
            sub_zh: "按窗口标题实时模糊检索并前台激活任意已打开窗口",
            sub_en: "Fuzzy-search open desktop windows and focus immediately",
            badge: "w ",
            aliases: "window switch 窗口 切换",
            action: ActionKind::FillQuery { query: "w ".into() },
        },
        HubEntry {
            id: "hub:settings",
            title_zh: "Ctrl+,  ·  打开 Tomoyo 设置中心",
            title_en: "Ctrl+,  ·  Open Tomoyo Settings",
            sub_zh: "配置外观主题（浅色/深色/午夜蓝）、字体排版、热键、标签与插件市场",
            sub_en: "Configure theme, typography, hotkeys, filter tabs & plugins",
            badge: "SET",
            aliases: "settings 主题 字体 热键 设置",
            action: ActionKind::InternalCommand { command: InternalCommandKind::OpenSettingsWindow },
        },
        HubEntry {
            id: "hub:stats",
            title_zh: ":stats  ·  系统诊断与内存预算",
            title_en: ":stats  ·  Diagnostics & Memory Budget",
            sub_zh: "查看当前进程内存占用、已加载插件数、热键注册状态与更新检查（:update）",
            sub_en: "Inspect live memory footprint, plugin count, hotkeys & :update",
            badge: ":stat",
            aliases: "stats diag update 诊断 内存 更新",
            action: ActionKind::FillQuery { query: ":stats".into() },
        },
    ];

    let mut out = Vec::new();
    for (idx, e) in entries.into_iter().enumerate() {
        let title = if zh { e.title_zh } else { e.title_en };
        let subtitle = if zh { e.sub_zh } else { e.sub_en };
        let hay = format!("{title} {subtitle} {}", e.aliases);
        let score = if kw.is_empty() {
            9_600 - (idx as i64 * 20)
        } else if let Some(ts) = score_match(&kw, &IndexedText::new(title)) {
            ts + 2_000
        } else if let Some(s) = score_match(&kw, &IndexedText::new(&hay)) {
            s
        } else if hay.to_lowercase().contains(&kw) {
            7_000 - (idx as i64 * 20)
        } else {
            continue;
        };
        out.push(ResultItem {
            id: e.id.into(),
            provider_id: provider_id.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            category: ResultCategory::System,
            score,
            icon: IconSpec::Monogram(e.badge.into()),
            badge: Some(e.badge.into()),
            copy_text: None,
            actions: vec![ActionSpec {
                id: "use".into(),
                label: if zh { "使用此指令" } else { "Use Command" }.into(),
                shortcut: Some("Enter".into()),
                kind: e.action,
            }],
            preview: Some(preview.clone()),
        });
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_question_and_single_slash_without_path_false_positives() {
        assert!(is_command_hub_query("?"));
        assert!(is_command_hub_query("？文件"));
        assert!(is_command_hub_query("/截图"));
        assert!(!is_command_hub_query("/usr/bin/env"));
        assert!(!is_command_hub_query("cargo"));
    }

    #[test]
    fn filters_hub_entries_by_keyword() {
        let all = build_command_hub_items("builtin.system", "?", Lang::Zh);
        assert!(all.len() >= 10);
        let filtered = build_command_hub_items("builtin.system", "? 截图", Lang::Zh);
        assert!(!filtered.is_empty());
        assert_eq!(filtered[0].id, "hub:capture");
    }
}
