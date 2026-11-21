//! Declarative Lightweight Action Pipelines (`flows.toml`) — Zero script runtime bloat!
//!
//! Users compose bounded multi-step action chains (e.g. Read Clipboard -> Single Line -> Paste,
//! or Screenshot -> OCR -> Translate) in portable `<sync_root>/flows.toml` or via Settings Center.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_FLOWS: usize = 32;
pub const MAX_FLOW_STEPS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowStepKind {
    ReadClipboard,
    CaptureRegion,
    CaptureOcr,
    CaptureTranslate,
    Trim,
    SingleLine,
    TrimLines,
    Uppercase,
    Lowercase,
    UrlEncode,
    Template,
    CopyText,
    PasteText,
    OpenUrl,
    OpenPath,
    LaunchTranslate,
    SaveNote,
    FillQuery,
    Toast,
}

impl FlowStepKind {
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::ReadClipboard => "读取剪贴板",
            Self::CaptureRegion => "区域截图",
            Self::CaptureOcr => "截图并 OCR",
            Self::CaptureTranslate => "截图并翻译",
            Self::Trim => "首尾去空白",
            Self::SingleLine => "合并单行",
            Self::TrimLines => "去除空行",
            Self::Uppercase => "转大写",
            Self::Lowercase => "转小写",
            Self::UrlEncode => "URL 编码",
            Self::Template => "模板拼装",
            Self::CopyText => "复制到剪贴板",
            Self::PasteText => "直贴回原窗口",
            Self::OpenUrl => "打开网页",
            Self::OpenPath => "打开路径",
            Self::LaunchTranslate => "多源翻译",
            Self::SaveNote => "存为本地备忘",
            Self::FillQuery => "预填启动器指令",
            Self::Toast => "底部提示",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::ReadClipboard => "Read Clipboard",
            Self::CaptureRegion => "Capture Region",
            Self::CaptureOcr => "Capture & OCR",
            Self::CaptureTranslate => "Capture & Translate",
            Self::Trim => "Trim",
            Self::SingleLine => "Single Line",
            Self::TrimLines => "Strip Blank Lines",
            Self::Uppercase => "Uppercase",
            Self::Lowercase => "Lowercase",
            Self::UrlEncode => "URL Encode",
            Self::Template => "Template",
            Self::CopyText => "Copy Text",
            Self::PasteText => "Paste to Window",
            Self::OpenUrl => "Open URL",
            Self::OpenPath => "Open Path",
            Self::LaunchTranslate => "Translate",
            Self::SaveNote => "Save Note",
            Self::FillQuery => "Fill Query",
            Self::Toast => "Toast",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowStep {
    pub kind: FlowStepKind,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub keyword: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub steps: Vec<FlowStep>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FlowsFile {
    #[serde(default)]
    flows: Vec<WorkflowDef>,
}

/// Concrete side effects produced by evaluating a declarative workflow pipeline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowRunPlan {
    pub final_text: String,
    pub copy_text: Option<String>,
    pub paste_after_hide: bool,
    pub open_url: Option<String>,
    pub open_path: Option<String>,
    pub launch_translate: Option<String>,
    pub capture_mode: Option<String>,
    pub save_note_title_body: Option<(String, String)>,
    pub fill_query: Option<String>,
    pub toast: Option<String>,
    pub hide_window: bool,
}

pub fn flows_file_path() -> PathBuf {
    crate::sync_root_dir().join("flows.toml")
}

pub fn default_workflows() -> Vec<WorkflowDef> {
    vec![
        WorkflowDef {
            id: "shot_ocr_tr".into(),
            name: "截图识字并多源翻译".into(),
            keyword: "shottr".into(),
            description: "框选屏幕区域 → WinRT 离线 OCR → 立即打开多源翻译对比".into(),
            enabled: true,
            steps: vec![FlowStep { kind: FlowStepKind::CaptureTranslate, value: String::new() }],
        },
        WorkflowDef {
            id: "clip_clean_paste".into(),
            name: "剪贴板去换行纯文本直贴".into(),
            keyword: "cleanpaste".into(),
            description: "读取剪贴板 → 合并断行与多余空格 → 无格式直接贴回当前窗口（PDF 复制利器）".into(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::SingleLine, value: String::new() },
                FlowStep { kind: FlowStepKind::PasteText, value: String::new() },
            ],
        },
        WorkflowDef {
            id: "clip_translate".into(),
            name: "剪贴板一键多源翻译".into(),
            keyword: "cliptr".into(),
            description: "读取剪贴板或输入参数 → 清理首尾空白 → 拉起多源翻译工作台".into(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::Trim, value: String::new() },
                FlowStep { kind: FlowStepKind::LaunchTranslate, value: String::new() },
            ],
        },
        WorkflowDef {
            id: "clip_to_note".into(),
            name: "剪贴板整理并存为本地备忘".into(),
            keyword: "clipnote".into(),
            description: "读取剪贴板 → 去除多余空行 → 直接写入本地 Markdown 备忘录目录".into(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::TrimLines, value: String::new() },
                FlowStep { kind: FlowStepKind::SaveNote, value: "剪贴板摘录".into() },
                FlowStep { kind: FlowStepKind::Toast, value: "已将剪贴板保存为本地 Markdown 备忘录".into() },
            ],
        },
        WorkflowDef {
            id: "clip_web_search".into(),
            name: "剪贴板文本速查网页".into(),
            keyword: "clipweb".into(),
            description: "读取剪贴板 → 合并单行 → URL 编码并在浏览器中检索".into(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::SingleLine, value: String::new() },
                FlowStep { kind: FlowStepKind::UrlEncode, value: String::new() },
                FlowStep { kind: FlowStepKind::OpenUrl, value: "https://www.google.com/search?q={{text}}".into() },
            ],
        },
    ]
}

pub fn sanitize_workflows(raw: Vec<WorkflowDef>) -> Vec<WorkflowDef> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for mut f in raw {
        let id = f.id.trim().to_ascii_lowercase();
        let name = f.name.trim().chars().take(48).collect::<String>();
        if id.is_empty() || name.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        f.id = id.chars().take(36).collect();
        f.name = name;
        f.keyword = f.keyword.trim().to_ascii_lowercase().chars().take(24).collect();
        f.description = f.description.trim().chars().take(120).collect();
        f.steps.truncate(MAX_FLOW_STEPS);
        for s in &mut f.steps {
            s.value = s.value.chars().take(320).collect();
        }
        if f.steps.is_empty() {
            continue;
        }
        out.push(f);
        if out.len() >= MAX_FLOWS {
            break;
        }
    }
    out
}

pub fn load_or_init_flows() -> Vec<WorkflowDef> {
    let path = flows_file_path();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(parsed) = toml::from_str::<FlowsFile>(&raw) {
            return sanitize_workflows(parsed.flows);
        }
    }
    let defs = default_workflows();
    let _ = save_flows(&defs);
    defs
}

pub fn save_flows(flows: &[WorkflowDef]) -> Result<Vec<WorkflowDef>, String> {
    let clean = sanitize_workflows(flows.to_vec());
    let path = flows_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("无法创建工作流目录: {e}"))?;
    }
    let body = toml::to_string_pretty(&FlowsFile { flows: clean.clone() })
        .map_err(|e| format!("序列化 flows.toml 失败: {e}"))?;
    std::fs::write(&path, body).map_err(|e| format!("写入 flows.toml 失败: {e}"))?;
    Ok(clean)
}

fn percent_encode_uri(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(*b as char);
        } else if *b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn gen_flow_uuid() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let mut h1 = RandomState::new().build_hasher();
    let mut h2 = RandomState::new().build_hasher();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    h1.write_u64(seq);
    h2.write_u64(seq.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let a = h1.finish();
    let b = h2.finish();
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (a >> 32) as u32,
        ((a >> 16) as u16),
        (a as u16) & 0x0FFF,
        (((b >> 48) as u16) & 0x3FFF) | 0x8000,
        b & 0x0000_FFFF_FFFF_FFFF
    )
}

fn expand_flow_vars(tpl: &str, text: &str, input: &str, clip: &str) -> String {
    let now = Local::now();
    let mut out = tpl
        .replace("{{text}}", text)
        .replace("{{input}}", input)
        .replace("{{clipboard}}", clip)
        .replace("{{date}}", &now.format("%Y-%m-%d").to_string())
        .replace("{{datetime}}", &now.format("%Y-%m-%d %H:%M:%S").to_string())
        .replace("{{unix}}", &now.timestamp().to_string());
    if out.contains("{{uuid}}") {
        out = out.replace("{{uuid}}", &gen_flow_uuid());
    }
    out
}

/// Evaluates all pure transforms in a workflow and returns the concrete `FlowRunPlan`.
pub fn run_flow_pipeline(flow: &WorkflowDef, input: &str, clipboard: &str) -> FlowRunPlan {
    let mut text = if !input.trim().is_empty() {
        input.trim().to_string()
    } else {
        clipboard.to_string()
    };
    let mut plan = FlowRunPlan { hide_window: true, ..Default::default() };

    for step in flow.steps.iter().take(MAX_FLOW_STEPS) {
        match step.kind {
            FlowStepKind::ReadClipboard => {
                if input.trim().is_empty() {
                    text = clipboard.to_string();
                }
            }
            FlowStepKind::CaptureRegion => plan.capture_mode = Some("region".into()),
            FlowStepKind::CaptureOcr => plan.capture_mode = Some("ocr".into()),
            FlowStepKind::CaptureTranslate => plan.capture_mode = Some("translate".into()),
            FlowStepKind::Trim => text = text.trim().to_string(),
            FlowStepKind::SingleLine => {
                text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            }
            FlowStepKind::TrimLines => {
                text = text
                    .lines()
                    .map(str::trim_end)
                    .filter(|l| !l.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            FlowStepKind::Uppercase => text = text.to_uppercase(),
            FlowStepKind::Lowercase => text = text.to_lowercase(),
            FlowStepKind::UrlEncode => text = percent_encode_uri(&text),
            FlowStepKind::Template => {
                text = expand_flow_vars(&step.value, &text, input, clipboard);
            }
            FlowStepKind::CopyText => plan.copy_text = Some(text.clone()),
            FlowStepKind::PasteText => {
                plan.copy_text = Some(text.clone());
                plan.paste_after_hide = true;
            }
            FlowStepKind::OpenUrl => {
                let u = expand_flow_vars(&step.value, &text, input, clipboard);
                if crate::is_safe_http_url(&u) {
                    plan.open_url = Some(u);
                }
            }
            FlowStepKind::OpenPath => {
                let p = expand_flow_vars(&step.value, &text, input, clipboard);
                if !p.trim().is_empty() {
                    plan.open_path = Some(p);
                }
            }
            FlowStepKind::LaunchTranslate => {
                let payload = if step.value.trim().is_empty() {
                    text.clone()
                } else {
                    expand_flow_vars(&step.value, &text, input, clipboard)
                };
                plan.launch_translate = Some(payload);
            }
            FlowStepKind::SaveNote => {
                let title_raw = if step.value.trim().is_empty() {
                    text.lines().next().unwrap_or("工作流备忘").trim().chars().take(32).collect()
                } else {
                    expand_flow_vars(&step.value, &text, input, clipboard)
                };
                if !text.trim().is_empty() {
                    plan.save_note_title_body = Some((title_raw, text.clone()));
                }
            }
            FlowStepKind::FillQuery => {
                plan.fill_query = Some(expand_flow_vars(&step.value, &text, input, clipboard));
                plan.hide_window = false;
            }
            FlowStepKind::Toast => {
                plan.toast = Some(expand_flow_vars(&step.value, &text, input, clipboard));
            }
        }
    }
    plan.final_text = text;
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_multi_step_clipboard_clean_and_url_encode_pipeline() {
        let flow = WorkflowDef {
            id: "test_web".into(),
            name: "Clean Search".into(),
            keyword: "csearch".into(),
            description: String::new(),
            enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
                FlowStep { kind: FlowStepKind::SingleLine, value: String::new() },
                FlowStep { kind: FlowStepKind::UrlEncode, value: String::new() },
                FlowStep { kind: FlowStepKind::OpenUrl, value: "https://example.com/?q={{text}}".into() },
                FlowStep { kind: FlowStepKind::Toast, value: "Done {{text}}".into() },
            ],
        };
        let plan = run_flow_pipeline(&flow, "", "  hello\n   rust   world  ");
        assert_eq!(plan.final_text, "hello+rust+world");
        assert_eq!(plan.open_url.as_deref(), Some("https://example.com/?q=hello+rust+world"));
        assert_eq!(plan.toast.as_deref(), Some("Done hello+rust+world"));
    }

    #[test]
    fn sanitizes_duplicate_and_empty_workflows() {
        let bad = vec![
            WorkflowDef { id: "".into(), name: "No ID".into(), keyword: "".into(), description: "".into(), enabled: true, steps: vec![FlowStep { kind: FlowStepKind::Trim, value: "".into() }] },
            WorkflowDef { id: "a".into(), name: "Empty".into(), keyword: "".into(), description: "".into(), enabled: true, steps: vec![] },
            WorkflowDef { id: "ok".into(), name: "Valid".into(), keyword: "OK".into(), description: "".into(), enabled: true, steps: vec![FlowStep { kind: FlowStepKind::Trim, value: "".into() }] },
            WorkflowDef { id: "OK".into(), name: "Dup".into(), keyword: "".into(), description: "".into(), enabled: true, steps: vec![FlowStep { kind: FlowStepKind::Trim, value: "".into() }] },
        ];
        let clean = sanitize_workflows(bad);
        assert_eq!(clean.len(), 1);
        assert_eq!(clean[0].id, "ok");
        assert_eq!(clean[0].keyword, "ok");
    }
}
