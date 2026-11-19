//! System commands, Super Match attached-capsule actions, Auto-Update (`:update`), and Diagnostics.

mod diagnostics;
mod super_match;
mod updates;

use diagnostics::build_diagnostics_items;
use std::sync::{Arc, RwLock};
use super_match::build_super_match_items;
use tomoyo_core::{
    score_match, ExternalPluginBridge, IndexedText, Lang, Provider, TomoyoConfig, UpdateManager,
};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, QueryContext, ResultCategory,
    ResultItem, SystemCommandKind,
};

struct SysEntry {
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    badge: &'static str,
    indexed: IndexedText,
    kind: ActionKind,
}

pub struct SystemProvider {
    commands: Vec<SysEntry>,
    plugin_bridge: Option<Arc<dyn ExternalPluginBridge>>,
    config: Arc<RwLock<TomoyoConfig>>,
    updater: Arc<UpdateManager>,
}

impl SystemProvider {
    pub fn new(
        plugin_bridge: Option<Arc<dyn ExternalPluginBridge>>,
        config: Arc<RwLock<TomoyoConfig>>,
        updater: Arc<UpdateManager>,
    ) -> Self {
        let specs: [(&str, &str, &str, &str, ActionKind); 8] = [
            (
                "sys:tomoyo_settings",
                "Tomoyo 设置中心 Settings",
                "打开图形化设置中心（主题、快捷键自动录入、语言、Everything、插件市场、检查更新）",
                "SET",
                ActionKind::InternalCommand {
                    command: InternalCommandKind::OpenSettingsWindow,
                },
            ),
            (
                "sys:check_update",
                "检查更新 Check for Updates",
                "联网检查 GitHub 最新 Release 版本并查看更新日志",
                "UPD",
                ActionKind::InternalCommand {
                    command: InternalCommandKind::CheckForUpdates,
                },
            ),
            (
                "sys:lock",
                "Lock Screen 锁屏",
                "Lock your Windows workstation immediately",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::LockWorkstation,
                },
            ),
            (
                "sys:sleep",
                "Sleep 睡眠",
                "Put computer into low-power sleep mode",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::Sleep,
                },
            ),
            (
                "sys:recycle",
                "Empty Recycle Bin 清空回收站",
                "Permanently clear Windows Recycle Bin",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::EmptyRecycleBin,
                },
            ),
            (
                "sys:settings",
                "Windows Settings 系统设置",
                "Open Windows Settings (ms-settings:)",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::OpenSettings,
                },
            ),
            (
                "sys:taskmgr",
                "Task Manager 任务管理器",
                "Open Windows Task Manager",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::OpenTaskManager,
                },
            ),
            (
                "sys:terminal",
                "Terminal 终端",
                "Open Windows Terminal / PowerShell",
                "SYS",
                ActionKind::SystemCommand {
                    command: SystemCommandKind::OpenTerminal,
                },
            ),
        ];

        let commands = specs
            .into_iter()
            .map(|(id, title, subtitle, badge, kind)| SysEntry {
                id,
                title,
                subtitle,
                badge,
                indexed: IndexedText::new(title),
                kind,
            })
            .collect();

        Self {
            commands,
            plugin_bridge,
            config,
            updater,
        }
    }
}

impl Provider for SystemProvider {
    fn id(&self) -> &'static str {
        "builtin.system"
    }

    fn name(&self) -> &'static str {
        "System Commands & Diagnostics"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &[":"]
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let lang = self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh);

        if let Some(attached) = &ctx.attached {
            return build_super_match_items(self.id(), attached, &ctx.trimmed, lang);
        }

        let trimmed = ctx.trimmed.as_str();
        if trimmed.starts_with(':') {
            return build_diagnostics_items(
                self.id(),
                &trimmed.to_lowercase(),
                self.plugin_bridge.as_ref(),
                &self.config,
                &self.updater,
            );
        }
        if trimmed.is_empty() {
            return Vec::new();
        }

        self.commands
            .iter()
            .filter_map(|cmd| {
                let s = score_match(trimmed, &cmd.indexed)?;
                Some(ResultItem {
                    id: cmd.id.into(),
                    provider_id: self.id().into(),
                    title: cmd.title.into(),
                    subtitle: cmd.subtitle.into(),
                    category: ResultCategory::System,
                    score: s - 200,
                    icon: IconSpec::Monogram(cmd.badge.into()),
                    badge: Some(cmd.badge.into()),
                    copy_text: None,
                    actions: vec![ActionSpec {
                        id: "exec".into(),
                        label: cmd.title.into(),
                        shortcut: Some("Enter".into()),
                        kind: cmd.kind.clone(),
                    }],
                    preview: None,
                })
            })
            .collect()
    }
}
