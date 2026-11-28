//! Official standalone Tomoyo plugin: Dedicated Markdown Editor powered by `s1dashu/quiet-md` (MIT).
//!
//! Strictly decoupled from Quick Notes (`dev.tomoyo.notes`) and keeps zero UI bloat inside
//! `tomoyo.exe`: discovers or SHA-256 installs `Quiet.md`, previews local `.md` files inline,
//! and opens/creates standalone Markdown documents directly in Quiet.md.

mod quiet_discovery;
mod quiet_install;

use quiet_discovery::{
    MdDoc, create_md_file, docs_dir_from, find_quiet_md, is_md_file, read_md_doc,
    scan_markdown_docs,
};
use std::path::{Path, PathBuf};
use tomoyo_sdk::{
    ActionKind, ActionSpec, AttachedContext, ExecuteRequest, ExecuteResponse, IconSpec,
    PluginMeta, PreviewSpec, QueryContext, ResultCategory, ResultItem, TomoyoPlugin,
};

const PLUGIN_ID: &str = "dev.tomoyo.markdown-editor";
const QUIET_MD_URL: &str = "https://github.com/s1dashu/quiet-md";

struct MarkdownEditorPlugin;

fn parse_create_title(args: &str) -> Option<String> {
    let rest = args
        .strip_prefix("+ ")
        .or_else(|| args.strip_prefix('+'))
        .or_else(|| args.strip_prefix("new "))
        .or_else(|| args.strip_prefix("新建 "))?
        .trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.chars().take(72).collect())
    }
}

fn build_open_doc_item(doc: &MdDoc, quiet_exe: Option<&Path>, score: i64) -> ResultItem {
    let path_str = doc.path.to_string_lossy().into_owned();
    let mut actions = Vec::new();
    if let Some(qm) = quiet_exe {
        actions.push(ActionSpec {
            id: "open_quiet_md".into(),
            label: "在 Quiet.md 中打开编辑".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::LaunchPath {
                path: qm.to_string_lossy().into_owned(),
                args: Some(format!("\"{path_str}\"")),
                working_dir: doc.path.parent().map(|p| p.to_string_lossy().into_owned()),
            },
        });
    } else {
        actions.push(ActionSpec {
            id: "install_quiet_md".into(),
            label: "下载安装 Quiet.md (MIT · SHA-256 校验)".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "install_quiet_md".into(),
                payload: serde_json::Value::Null,
            },
        });
    }
    actions.push(ActionSpec {
        id: "open_default".into(),
        label: "用系统默认关联程序打开 .md".into(),
        shortcut: Some("Ctrl+Enter".into()),
        kind: ActionKind::LaunchPath {
            path: path_str.clone(),
            args: None,
            working_dir: None,
        },
    });
    actions.push(ActionSpec {
        id: "reveal".into(),
        label: "在资源管理器中定位文件".into(),
        shortcut: Some("Shift+Enter".into()),
        kind: ActionKind::RevealInExplorer { path: path_str.clone() },
    });

    ResultItem {
        id: format!("mde:doc:{path_str}"),
        provider_id: PLUGIN_ID.into(),
        title: doc.title.clone(),
        subtitle: format!("{} · {}", path_str, doc.snippet),
        category: ResultCategory::Plugin,
        score,
        icon: IconSpec::Monogram("MD".into()),
        badge: Some(if quiet_exe.is_some() { "Quiet.md" } else { "MD" }.into()),
        copy_text: Some(doc.body_preview.clone()),
        actions,
        preview: Some(PreviewSpec::Text {
            title: doc
                .path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| doc.title.clone()),
            body: doc.body_preview.clone(),
        }),
    }
}

fn build_create_doc_item(
    title: &str,
    docs_dir: &Path,
    quiet_exe: Option<&Path>,
    open_after: bool,
    score: i64,
    is_quick: bool,
) -> ResultItem {
    let qm_str = quiet_exe
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let label = if quiet_exe.is_some() && open_after {
        "创建并在 Quiet.md 中打开"
    } else if is_quick {
        "创建并编辑文档"
    } else {
        "创建 .md 文档"
    };
    ResultItem {
        id: if is_quick { "mde:create_quick" } else { "mde:create" }.into(),
        provider_id: PLUGIN_ID.into(),
        title: if is_quick {
            format!("新建 Markdown 文档「{title}.md」")
        } else {
            format!("新建 Markdown 文档：{title}")
        },
        subtitle: format!(
            "保存至 {} · {}",
            docs_dir.display(),
            if is_quick { "与快速备忘录目录隔离" } else { "回车创建并打开编辑" }
        ),
        category: ResultCategory::Plugin,
        score,
        icon: IconSpec::Monogram("MD+".into()),
        badge: Some(if is_quick { "CREATE" } else { "NEW" }.into()),
        copy_text: None,
        actions: vec![ActionSpec {
            id: "create_and_open".into(),
            label: label.into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: PLUGIN_ID.into(),
                action_id: "create_and_open".into(),
                payload: serde_json::json!({
                    "title": title,
                    "dir": docs_dir.to_string_lossy(),
                    "quiet_exe": qm_str,
                    "open_after": open_after
                }),
            },
        }],
        preview: (!is_quick).then(|| PreviewSpec::Text {
            title: format!("{title}.md"),
            body: format!("# {title}\n\n（独立 Markdown 文档，位于 {}）", docs_dir.display()),
        }),
    }
}

fn build_primary_launcher_item(docs_dir: &Path, quiet_exe: Option<&Path>) -> ResultItem {
    if let Some(qm) = quiet_exe {
        return ResultItem {
            id: "mde:launch_quiet_md".into(),
            provider_id: PLUGIN_ID.into(),
            title: "启动 Quiet.md 桌面 Markdown 编辑器".into(),
            subtitle: format!("MIT 开源 Rust/GPUI 原生编辑器 · 已就绪 ({})", qm.display()),
            category: ResultCategory::Plugin,
            score: 10_000,
            icon: IconSpec::Monogram("QM".into()),
            badge: Some("Quiet.md".into()),
            copy_text: None,
            actions: vec![
                ActionSpec {
                    id: "launch_qm".into(),
                    label: "打开 Quiet.md 窗口".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::LaunchPath {
                        path: qm.to_string_lossy().into_owned(),
                        args: None,
                        working_dir: qm.parent().map(|p| p.to_string_lossy().into_owned()),
                    },
                },
                ActionSpec {
                    id: "open_docs_dir".into(),
                    label: "打开本地 Markdown 文档库目录".into(),
                    shortcut: Some("Ctrl+Enter".into()),
                    kind: ActionKind::LaunchPath {
                        path: docs_dir.to_string_lossy().into_owned(),
                        args: None,
                        working_dir: None,
                    },
                },
            ],
            preview: Some(PreviewSpec::KeyValue {
                title: "Quiet.md Markdown 编辑器 (独立解耦)".into(),
                entries: vec![
                    ("定位分工".into(), "专注长文排版与所见即所得 Markdown 编辑（与备忘录分离）".into()),
                    ("开源引擎".into(), "s1dashu/quiet-md (MIT · Rust + GPUI)".into()),
                    ("可执行路径".into(), qm.to_string_lossy().into_owned()),
                    ("默认文档目录".into(), docs_dir.to_string_lossy().into_owned()),
                ],
            }),
        };
    }

    ResultItem {
        id: "mde:install_quiet_md".into(),
        provider_id: PLUGIN_ID.into(),
        title: "一键安装 Quiet.md 桌面 Markdown 编辑器（SHA-256 校验）".into(),
        subtitle: "从 github.com/s1dashu/quiet-md 官方 Releases 校验部署 · 零 Tomoyo 主程序膨胀".into(),
        category: ResultCategory::Plugin,
        score: 10_000,
        icon: IconSpec::Monogram("DL".into()),
        badge: Some("INSTALL".into()),
        copy_text: None,
        actions: vec![
            ActionSpec {
                id: "install_qm".into(),
                label: "SHA-256 校验下载并安装 Quiet.md".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::PluginExecute {
                    plugin_id: PLUGIN_ID.into(),
                    action_id: "install_quiet_md".into(),
                    payload: serde_json::Value::Null,
                },
            },
            ActionSpec {
                id: "open_repo".into(),
                label: "浏览 GitHub 仓库 (s1dashu/quiet-md)".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::OpenUrl { url: QUIET_MD_URL.into() },
            },
        ],
        preview: Some(PreviewSpec::KeyValue {
            title: "集成独立编辑器：Quiet.md (MIT)".into(),
            entries: vec![
                ("为什么分离".into(), "保持 Tomoyo 主内核 < 6.5 MB 与 2 MB 休眠红线，同时提供原生 GPUI Markdown 体验".into()),
                ("官方源".into(), QUIET_MD_URL.into()),
                ("验证方式".into(), "官方 SHA256SUMS 逐字节核验后静默安装".into()),
            ],
        }),
    }
}

impl TomoyoPlugin for MarkdownEditorPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("Quiet.md Markdown 编辑器", "2.6.0")
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let custom_bin = ctx
            .plugin_settings
            .get("custom_bin_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let quiet_exe = find_quiet_md(custom_bin);
        let docs_dir = docs_dir_from(ctx);
        let raw_args = ctx.args.trim();
        let unquoted = raw_args.trim_matches('"').trim();
        let open_after = ctx
            .plugin_settings
            .get("open_new_in_quiet")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let mut out = Vec::new();

        if let Some(AttachedContext::Files { paths }) = &ctx.attached {
            for p_str in paths {
                let p = Path::new(p_str);
                if is_md_file(p)
                    && let Some(doc) = read_md_doc(p)
                {
                    out.push(build_open_doc_item(&doc, quiet_exe.as_deref(), 9_980));
                }
            }
        }
        if !unquoted.is_empty() {
            let p = Path::new(unquoted);
            if is_md_file(p)
                && let Some(doc) = read_md_doc(p)
            {
                out.push(build_open_doc_item(&doc, quiet_exe.as_deref(), 9_990));
                return out;
            }
        }

        if let Some(title) = parse_create_title(raw_args) {
            out.push(build_create_doc_item(&title, &docs_dir, quiet_exe.as_deref(), open_after, 9_950, false));
            return out;
        }

        if quiet_exe.is_none() || raw_args.is_empty() {
            out.push(build_primary_launcher_item(&docs_dir, quiet_exe.as_deref()));
        }

        let docs = scan_markdown_docs(&docs_dir);
        let tokens: Vec<String> = raw_args.split_whitespace().map(|t| t.to_ascii_lowercase()).collect();
        for (i, doc) in docs
            .iter()
            .filter(|d| {
                tokens.is_empty()
                    || tokens.iter().all(|t| {
                        d.title.to_ascii_lowercase().contains(t)
                            || d.path.to_string_lossy().to_ascii_lowercase().contains(t)
                            || d.body_preview.to_ascii_lowercase().contains(t)
                    })
            })
            .take(12)
            .enumerate()
        {
            out.push(build_open_doc_item(doc, quiet_exe.as_deref(), 9_400 - i as i64));
        }

        if !raw_args.is_empty() {
            out.push(build_create_doc_item(raw_args, &docs_dir, quiet_exe.as_deref(), open_after, 8_200, true));
        }
        out
    }

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        if req.action_id == "install_quiet_md" {
            quiet_install::spawn_verified_install();
            return ExecuteResponse {
                hide_window: false,
                toast: Some("正在插件后台 SHA-256 校验下载并安装 Quiet.md…".into()),
                ..Default::default()
            };
        }
        if req.action_id == "open_md_file" {
            let path_str = req.payload.get("path").and_then(|v| v.as_str()).unwrap_or("").trim();
            if path_str.is_empty() || !Path::new(path_str).is_file() {
                return ExecuteResponse { hide_window: false, toast: Some("未找到目标 Markdown 文件".into()), ..Default::default() };
            }
            let custom_bin = req
                .plugin_settings
                .get("custom_bin_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if let Some(qm) = find_quiet_md(custom_bin) {
                quiet_install::spawn_detached_editor(&qm, path_str);
                return ExecuteResponse { hide_window: true, ..Default::default() };
            }
            return ExecuteResponse {
                hide_window: false,
                toast: Some("未检测到 Quiet.md，已打开插件安装入口".into()),
                fill_query: Some(format!("md \"{path_str}\"")),
                ..Default::default()
            };
        }
        if req.action_id != "create_and_open" {
            return ExecuteResponse::default();
        }
        let title = req.payload.get("title").and_then(|v| v.as_str()).unwrap_or("未命名文档");
        let dir = req
            .payload
            .get("dir")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let qm_exe = req.payload.get("quiet_exe").and_then(|v| v.as_str()).unwrap_or("");
        let open_after = req.payload.get("open_after").and_then(|v| v.as_bool()).unwrap_or(true);

        match create_md_file(&dir, title) {
            Ok(created) => {
                if open_after && !qm_exe.is_empty() && Path::new(qm_exe).is_file() {
                    quiet_install::spawn_detached_editor(Path::new(qm_exe), &created.to_string_lossy());
                    return ExecuteResponse {
                        hide_window: true,
                        toast: Some(format!("已新建并用 Quiet.md 打开：{}", created.file_name().unwrap_or_default().to_string_lossy())),
                        ..Default::default()
                    };
                }
                ExecuteResponse {
                    hide_window: false,
                    toast: Some(format!("已创建 Markdown 文档：{}", created.display())),
                    fill_query: Some(format!("md \"{}\"", created.display())),
                    ..Default::default()
                }
            }
            Err(e) => ExecuteResponse { hide_window: false, toast: Some(e), ..Default::default() },
        }
    }
}

fn main() {
    tomoyo_sdk::run(MarkdownEditorPlugin);
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
