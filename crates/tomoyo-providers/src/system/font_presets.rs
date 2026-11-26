//! Quick `:font` launcher command builder backed by `tomoyo_core::BUILTIN_FONT_PRESETS`.

use tomoyo_core::{Lang, BUILTIN_FONT_PRESETS};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, ResultCategory, ResultItem,
};

pub(super) fn append_font_items(
    items: &mut Vec<ResultItem>,
    provider_id: &str,
    cmd: &str,
    cur_family: &str,
    lang: Lang,
) {
    if !(cmd.starts_with(":font") || cmd == ":" || ":font".starts_with(cmd)) {
        return;
    }
    if cmd.starts_with(":font") {
        let arg = cmd.strip_prefix(":font").unwrap_or("").trim().to_ascii_lowercase();
        for (idx, p) in BUILTIN_FONT_PRESETS.iter().enumerate() {
            let name = match lang {
                Lang::Zh => p.name_zh,
                Lang::En => p.name_en,
            };
            let desc = match lang {
                Lang::Zh => p.desc_zh,
                Lang::En => p.desc_en,
            };
            let hit = arg.is_empty()
                || p.id.contains(&arg)
                || p.name_zh.to_ascii_lowercase().contains(&arg)
                || p.name_en.to_ascii_lowercase().contains(&arg)
                || p.family.to_ascii_lowercase().contains(&arg);
            if !hit {
                continue;
            }
            let active = cur_family.trim().eq_ignore_ascii_case(p.family);
            items.push(ResultItem {
                id: format!("diag:font:{}", p.id),
                provider_id: provider_id.into(),
                title: format!(":font {} — {name}", p.id),
                subtitle: if active {
                    format!("{} · {desc}", lang.pick("当前使用中", "Active"))
                } else {
                    desc.into()
                },
                category: ResultCategory::System,
                score: 9_650 - (idx as i64) * 15,
                icon: IconSpec::Monogram("Aa".into()),
                badge: Some(if active { "ACTIVE" } else { "FONT" }.into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: format!("diag:font:{}", p.id),
                    label: lang.pick("立即切换字体方案", "Apply Font Preset").into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::SetFontFamily {
                            family: p.family.into(),
                            label: name.into(),
                        },
                    },
                }],
                preview: None,
            });
        }
        return;
    }

    let active_label = BUILTIN_FONT_PRESETS
        .iter()
        .find(|p| p.family.eq_ignore_ascii_case(cur_family.trim()))
        .map(|p| match lang {
            Lang::Zh => p.name_zh,
            Lang::En => p.name_en,
        })
        .unwrap_or_else(|| if cur_family.trim().is_empty() { "Native OS" } else { cur_family.trim() });

    items.push(ResultItem {
        id: "diag:font:hint".into(),
        provider_id: provider_id.into(),
        title: format!(":font — 快捷切换字体方案 (当前: {active_label})"),
        subtitle: lang
            .pick(
                "一键切换 极简现代 / 极客清晰 / 干练科技 / 温润文楷 / 系统原生",
                "Switch Minimal Modern / Crisp Geek / Sharp Tech / Warm WenKai / Native OS",
            )
            .into(),
        category: ResultCategory::System,
        score: 9_650,
        icon: IconSpec::Monogram("Aa".into()),
        badge: Some("FONT".into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "fill_font".into(),
            label: lang.pick("选择字体方案", "Choose Font Preset").into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::FillQuery { query: ":font ".into() },
        }],
        preview: None,
    });
}
