//! Settings Center visual CRUD for `snippets.toml` & interactive Flows 2.0 step-by-step dry-run.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tomoyo_core::{sync_root_dir, FlowStepKind, WorkflowDef};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetRow {
    pub keyword: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub template: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SnippetsDoc {
    #[serde(default)]
    snippets: Vec<SnippetRow>,
}

pub fn snippets_file_path() -> PathBuf {
    sync_root_dir().join("snippets.toml")
}

pub fn load_snippets_list() -> Vec<SnippetRow> {
    load_snippets_at(&snippets_file_path())
}

fn load_snippets_at(path: &Path) -> Vec<SnippetRow> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    toml::from_str::<SnippetsDoc>(&raw)
        .map(|d| d.snippets)
        .unwrap_or_default()
}

fn save_snippets_at(path: &Path, items: &[SnippetRow]) -> Result<Vec<SnippetRow>, String> {
    let mut clean: Vec<SnippetRow> = items
        .iter()
        .filter_map(|row| {
            let kw = row.keyword.trim().to_string();
            let title = row.title.trim().to_string();
            let tpl = row.template.trim().to_string();
            if kw.is_empty() || tpl.is_empty() {
                return None;
            }
            let tags = row
                .tags
                .iter()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect();
            Some(SnippetRow {
                keyword: kw,
                title: if title.is_empty() { row.keyword.trim().into() } else { title },
                tags,
                template: tpl,
            })
        })
        .take(500)
        .collect();
    // Deduplicate case-insensitively while preserving user order.
    let mut seen = std::collections::HashSet::new();
    clean.retain(|r| seen.insert(r.keyword.to_ascii_lowercase()));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let body = toml::to_string_pretty(&SnippetsDoc {
        snippets: clean.clone(),
    })
    .map_err(|e| format!("序列化 snippets.toml 失败: {e}"))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("写入失败: {e}"))?;
    if std::fs::rename(&tmp, path).is_err() {
        std::fs::copy(&tmp, path).map_err(|e| format!("覆盖失败: {e}"))?;
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(clean)
}

pub fn save_snippets_reply(snippets: &[SnippetRow]) -> (String, bool) {
    match save_snippets_at(&snippets_file_path(), snippets) {
        Ok(clean) => (
            serde_json::json!({
                "event": "snippets_saved",
                "ok": true,
                "snippets": clean,
                "message": format!("已保存 {} 条短语模板并热重载生效", clean.len())
            })
            .to_string(),
            true,
        ),
        Err(e) => (
            serde_json::json!({
                "event": "snippets_saved",
                "ok": false,
                "message": e
            })
            .to_string(),
            false,
        ),
    }
}

fn expand_preview_vars(tpl: &str, text: &str, clip: &str) -> String {
    let now = Local::now();
    let mut out = tpl
        .replace("{{date}}", &now.format("%Y-%m-%d").to_string())
        .replace("{{datetime}}", &now.format("%Y-%m-%d %H:%M:%S").to_string())
        .replace("{{unix}}", &now.timestamp().to_string())
        .replace("{{clipboard}}", clip)
        .replace("{{text}}", text)
        .replace("{{input}}", clip);
    if out.contains("{{uuid}}") {
        out = out.replace("{{uuid}}", "00000000-0000-4000-8000-000000000000");
    }
    out
}

pub fn dry_run_flow_reply(flow: &WorkflowDef, sample_clip: &str) -> String {
    let clip = if sample_clip.is_empty() { "  Tomoyo\n   Shell  " } else { sample_clip };
    let mut text = clip.to_string();
    let mut trace = Vec::with_capacity(flow.steps.len());
    let mut effects = Vec::new();

    for (i, step) in flow.steps.iter().enumerate() {
        let out_str = match step.kind {
            FlowStepKind::ReadClipboard => {
                text = clip.to_string();
                text.clone()
            }
            FlowStepKind::CaptureRegion => {
                effects.push("区域截图 (dry-run)".into());
                "[截图区域 PNG]".into()
            }
            FlowStepKind::CaptureOcr => {
                effects.push("截图+OCR (dry-run)".into());
                text = format!("[OCR 文本: {}]", text.trim());
                text.clone()
            }
            FlowStepKind::CaptureTranslate => {
                effects.push("截图+翻译 (dry-run)".into());
                "[翻译浮窗]".into()
            }
            FlowStepKind::Trim => {
                text = text.trim().to_string();
                text.clone()
            }
            FlowStepKind::SingleLine => {
                text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                text.clone()
            }
            FlowStepKind::TrimLines => {
                text = text
                    .lines()
                    .map(str::trim_end)
                    .filter(|l| !l.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                text.clone()
            }
            FlowStepKind::Uppercase => {
                text = text.to_uppercase();
                text.clone()
            }
            FlowStepKind::Lowercase => {
                text = text.to_lowercase();
                text.clone()
            }
            FlowStepKind::UrlEncode => {
                text = text
                    .bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() {
                            format!("{}", b as char)
                        } else if b == b' ' {
                            "+".into()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect();
                text.clone()
            }
            FlowStepKind::Template => {
                text = expand_preview_vars(&step.value, &text, clip);
                text.clone()
            }
            FlowStepKind::LlmTransform => {
                let p = expand_preview_vars(&step.value, &text, clip);
                text = format!("[LLM({p}) <- \"{}\"]", text.chars().take(48).collect::<String>());
                text.clone()
            }
            FlowStepKind::CopyText => {
                effects.push(format!("复制: {}", text.chars().take(40).collect::<String>()));
                text.clone()
            }
            FlowStepKind::PasteText => {
                effects.push(format!("贴回: {}", text.chars().take(40).collect::<String>()));
                text.clone()
            }
            FlowStepKind::OpenUrl => {
                let u = expand_preview_vars(&step.value, &text, clip);
                effects.push(format!("打开 URL: {u}"));
                u
            }
            FlowStepKind::OpenPath => {
                let p = expand_preview_vars(&step.value, &text, clip);
                effects.push(format!("打开路径: {p}"));
                p
            }
            FlowStepKind::LaunchTranslate => {
                let q = if step.value.trim().is_empty() {
                    text.clone()
                } else {
                    expand_preview_vars(&step.value, &text, clip)
                };
                effects.push(format!("划词翻译: {q}"));
                q
            }
            FlowStepKind::SaveNote => {
                effects.push(format!("存为备忘: {}", step.value.trim()));
                text.clone()
            }
            FlowStepKind::FillQuery => {
                let q = expand_preview_vars(&step.value, &text, clip);
                effects.push(format!("预填搜索: {q}"));
                q
            }
            FlowStepKind::Toast => {
                let t = expand_preview_vars(&step.value, &text, clip);
                effects.push(format!("Toast: {t}"));
                t
            }
        };
        trace.push(serde_json::json!({
            "index": i + 1,
            "kind": step.kind.label_en(),
            "output": out_str.chars().take(120).collect::<String>(),
        }));
    }

    serde_json::json!({
        "event": "flow_dry_run_result",
        "ok": true,
        "flow_id": flow.id,
        "trace": trace,
        "final_text": text,
        "effects": effects,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomoyo_core::FlowStep;

    #[test]
    fn snippets_roundtrip_and_flow_dry_run_work() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_wv_snip_{}.toml", std::process::id()));
        let saved = save_snippets_at(
            &tmp,
            &[
                SnippetRow { keyword: "hi".into(), title: "Hi".into(), tags: vec!["chat".into()], template: "Hello {{date}}".into() },
                SnippetRow { keyword: "HI".into(), title: "Dup".into(), tags: vec![], template: "ignored".into() },
            ],
        )
        .unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(load_snippets_at(&tmp).len(), 1);
        let _ = std::fs::remove_file(&tmp);

        let wf = WorkflowDef {
            id: "w1".into(), name: "Clean".into(), keyword: "cl".into(), hotkey: String::new(),
            description: String::new(), enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::SingleLine, value: String::new() },
                FlowStep { kind: FlowStepKind::Uppercase, value: String::new() },
                FlowStep { kind: FlowStepKind::CopyText, value: String::new() },
            ],
        };
        let raw = dry_run_flow_reply(&wf, "  a\n  b  ");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["final_text"], "A B");
        assert_eq!(v["trace"].as_array().unwrap().len(), 3);
    }
}
