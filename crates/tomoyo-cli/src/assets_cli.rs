//! Personal Asset (`snippets.toml`) & Flows 2.0 step-by-step dry-run CLI (`tomoyo-cli snippets|flows`).

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tomoyo_core::{
    is_safe_http_url, load_or_init_flows, sync_root_dir, FlowRunPlan, FlowStepKind, WorkflowDef,
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnippetRow {
    keyword: String,
    title: String,
    #[serde(default)]
    tags: Vec<String>,
    template: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SnippetsDoc {
    #[serde(default)]
    snippets: Vec<SnippetRow>,
}

pub fn cmd_snippets(args: &[String]) -> i32 {
    let path = sync_root_dir().join("snippets.toml");
    let Some(sub) = args.first().map(String::as_str) else {
        eprintln!("Usage:\n  tomoyo-cli snippets list\n  tomoyo-cli snippets add <keyword> --title <title> --template <tpl> [--tags a,b]\n  tomoyo-cli snippets remove <keyword>\n  tomoyo-cli snippets test <keyword> [--clip <text>]");
        return 1;
    };
    match sub {
        "list" => {
            let doc = load_snippets_at(&path);
            if doc.snippets.is_empty() {
                println!("No snippets configured at {}", path.display());
            } else {
                println!("Snippets ({} in {}):", doc.snippets.len(), path.display());
                for s in &doc.snippets {
                    println!("  - {:<12} | {} | {}", s.keyword, s.title, s.template.replace('\n', " "));
                }
            }
            0
        }
        "add" => {
            let Some(kw) = args.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()) else {
                eprintln!("Missing <keyword>");
                return 1;
            };
            let Some(title) = crate::flag_val(&args[2..], "--title") else {
                eprintln!("Missing --title <title>");
                return 1;
            };
            let Some(tpl) = crate::flag_val(&args[2..], "--template") else {
                eprintln!("Missing --template <tpl>");
                return 1;
            };
            let tags = crate::flag_val(&args[2..], "--tags")
                .map(|r| r.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
                .unwrap_or_default();
            match upsert_snippet_at(&path, SnippetRow { keyword: kw.into(), title: title.into(), tags, template: tpl.into() }) {
                Ok(()) => { println!("[OK] Saved snippet `{kw}` -> {}", path.display()); 0 }
                Err(e) => { eprintln!("Error: {e}"); 1 }
            }
        }
        "remove" => {
            let Some(kw) = args.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()) else {
                eprintln!("Missing <keyword>");
                return 1;
            };
            match remove_snippet_at(&path, kw) {
                Ok(true) => { println!("[OK] Removed snippet `{kw}`"); 0 }
                Ok(false) => { eprintln!("Snippet `{kw}` not found"); 1 }
                Err(e) => { eprintln!("Error: {e}"); 1 }
            }
        }
        "test" => {
            let Some(kw) = args.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()) else {
                eprintln!("Missing <keyword>");
                return 1;
            };
            let clip = crate::flag_val(&args[2..], "--clip").unwrap_or("sample-clipboard-text");
            let doc = load_snippets_at(&path);
            let Some(s) = doc.snippets.iter().find(|x| x.keyword.eq_ignore_ascii_case(kw)) else {
                eprintln!("Snippet `{kw}` not found");
                return 1;
            };
            println!("{}", expand_snippet_template(&s.template, clip));
            0
        }
        _ => 1,
    }
}

pub fn cmd_flows(args: &[String]) -> i32 {
    let Some(sub) = args.first().map(String::as_str) else {
        eprintln!("Usage:\n  tomoyo-cli flows list\n  tomoyo-cli flows dry-run <id-or-keyword> [--input <text>] [--clip <text>]");
        return 1;
    };
    let flows = load_or_init_flows();
    match sub {
        "list" => {
            println!("Configured Workflows ({}):", flows.len());
            for f in &flows {
                let chain: Vec<_> = f.steps.iter().map(|s| s.kind.label_en()).collect();
                let hk = if f.hotkey.is_empty() { "-" } else { f.hotkey.as_str() };
                println!("  - {:<14} (kw={}, hk={}, enabled={}) : {}", f.id, f.keyword, hk, f.enabled, chain.join(" -> "));
            }
            0
        }
        "dry-run" => {
            let Some(target) = args.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()) else {
                eprintln!("Missing <id-or-keyword>");
                return 1;
            };
            let Some(flow) = flows.iter().find(|f| f.id.eq_ignore_ascii_case(target) || f.keyword.eq_ignore_ascii_case(target)) else {
                eprintln!("Workflow `{target}` not found");
                return 1;
            };
            let input = crate::flag_val(&args[2..], "--input").unwrap_or("");
            let clip = crate::flag_val(&args[2..], "--clip").unwrap_or("Sample clipboard text");
            let (trace, plan) = dry_run_flow_trace(flow, input, clip);
            println!("=== Dry-Run Trace: {} ({}) ===", flow.name, flow.id);
            for line in trace { println!("  {line}"); }
            println!(
                "Side-effects: copy={}, paste={}, url={:?}, path={:?}, capture={:?}, note={}, fill_query={:?}, toast={:?}",
                plan.copy_text.is_some(), plan.paste_after_hide, plan.open_url, plan.open_path,
                plan.capture_mode, plan.save_note_title_body.is_some(), plan.fill_query, plan.toast
            );
            0
        }
        _ => 1,
    }
}

fn load_snippets_at(path: &Path) -> SnippetsDoc {
    let Ok(raw) = std::fs::read_to_string(path) else { return SnippetsDoc::default(); };
    toml::from_str(&raw).unwrap_or_default()
}

fn save_snippets_at(path: &Path, doc: &SnippetsDoc) -> Result<(), String> {
    if let Some(p) = path.parent() { std::fs::create_dir_all(p).map_err(|e| format!("{e}"))?; }
    let body = toml::to_string_pretty(doc).map_err(|e| format!("serialize snippets: {e}"))?;
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, body).map_err(|e| format!("{e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{e}"))?;
    Ok(())
}

fn upsert_snippet_at(path: &Path, row: SnippetRow) -> Result<(), String> {
    let mut doc = load_snippets_at(path);
    if let Some(existing) = doc.snippets.iter_mut().find(|s| s.keyword.eq_ignore_ascii_case(&row.keyword)) {
        *existing = row;
    } else {
        doc.snippets.push(row);
    }
    save_snippets_at(path, &doc)
}

fn remove_snippet_at(path: &Path, keyword: &str) -> Result<bool, String> {
    let mut doc = load_snippets_at(path);
    let before = doc.snippets.len();
    doc.snippets.retain(|s| !s.keyword.eq_ignore_ascii_case(keyword));
    let removed = doc.snippets.len() != before;
    if removed { save_snippets_at(path, &doc)?; }
    Ok(removed)
}

fn expand_snippet_template(tpl: &str, clip: &str) -> String {
    let now = Local::now();
    let mut out = tpl
        .replace("{{date}}", &now.format("%Y-%m-%d").to_string())
        .replace("{{datetime}}", &now.format("%Y-%m-%d %H:%M:%S").to_string())
        .replace("{{unix}}", &now.timestamp().to_string())
        .replace("{{clipboard}}", clip);
    if out.contains("{{uuid}}") {
        out = out.replace("{{uuid}}", &Uuid::new_v4().to_string());
    }
    out
}

pub(crate) fn dry_run_flow_trace(flow: &WorkflowDef, input: &str, clipboard: &str) -> (Vec<String>, FlowRunPlan) {
    let mut text = if !input.trim().is_empty() { input.trim().to_string() } else { clipboard.to_string() };
    let mut plan = FlowRunPlan { hide_window: true, ..Default::default() };
    let mut trace = Vec::new();
    for (i, step) in flow.steps.iter().enumerate() {
        let vars = |tpl: &str, cur: &str| {
            expand_snippet_template(tpl, clipboard).replace("{{text}}", cur).replace("{{input}}", input)
        };
        match step.kind {
            FlowStepKind::ReadClipboard => { if input.trim().is_empty() { text = clipboard.to_string(); } }
            FlowStepKind::CaptureRegion => plan.capture_mode = Some("region".into()),
            FlowStepKind::CaptureOcr => plan.capture_mode = Some("ocr".into()),
            FlowStepKind::CaptureTranslate => plan.capture_mode = Some("translate".into()),
            FlowStepKind::Trim => text = text.trim().to_string(),
            FlowStepKind::SingleLine => text = text.split_whitespace().collect::<Vec<_>>().join(" "),
            FlowStepKind::TrimLines => text = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n"),
            FlowStepKind::Uppercase => text = text.to_uppercase(),
            FlowStepKind::Lowercase => text = text.to_lowercase(),
            FlowStepKind::UrlEncode => text = text.bytes().map(|b| if b.is_ascii_alphanumeric() { format!("{}", b as char) } else if b == b' ' { "+".into() } else { format!("%{b:02X}") }).collect(),
            FlowStepKind::Template => text = vars(&step.value, &text),
            FlowStepKind::LlmTransform => { let p = vars(&step.value, &text); text = format!("[DRY-RUN LLM({p}) <- \"{}\"]", text.chars().take(48).collect::<String>()); }
            FlowStepKind::CopyText => plan.copy_text = Some(text.clone()),
            FlowStepKind::PasteText => { plan.copy_text = Some(text.clone()); plan.paste_after_hide = true; }
            FlowStepKind::OpenUrl => { let u = vars(&step.value, &text); if is_safe_http_url(&u) { plan.open_url = Some(u); } }
            FlowStepKind::OpenPath => { let p = vars(&step.value, &text); if !p.trim().is_empty() { plan.open_path = Some(p); } }
            FlowStepKind::LaunchTranslate => plan.launch_translate = Some(if step.value.trim().is_empty() { text.clone() } else { vars(&step.value, &text) }),
            FlowStepKind::SaveNote => if !text.trim().is_empty() { plan.save_note_title_body = Some((step.value.clone(), text.clone())); },
            FlowStepKind::FillQuery => { plan.fill_query = Some(vars(&step.value, &text)); plan.hide_window = false; }
            FlowStepKind::Toast => plan.toast = Some(vars(&step.value, &text)),
        }
        trace.push(format!("[{}] {:<16} -> {:?}", i + 1, step.kind.label_en(), text.chars().take(72).collect::<String>()));
    }
    plan.final_text = text;
    (trace, plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomoyo_core::FlowStep;

    #[test]
    fn snippets_crud_expand_and_flows_dry_run_trace() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_cli_snip_{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        upsert_snippet_at(&tmp, SnippetRow { keyword: "sig".into(), title: "Signature".into(), tags: vec!["mail".into()], template: "-- {{date}} {{clipboard}}".into() }).unwrap();
        let doc = load_snippets_at(&tmp);
        assert_eq!(doc.snippets.len(), 1);
        assert!(expand_snippet_template(&doc.snippets[0].template, "Timmy").ends_with("Timmy"));
        assert!(remove_snippet_at(&tmp, "sig").unwrap());
        let _ = std::fs::remove_file(&tmp);

        let flow = WorkflowDef {
            id: "dry".into(), name: "DryTest".into(), keyword: "dry".into(), hotkey: String::new(),
            description: String::new(), enabled: true,
            steps: vec![
                FlowStep { kind: FlowStepKind::SingleLine, value: String::new() },
                FlowStep { kind: FlowStepKind::LlmTransform, value: "Summarize".into() },
                FlowStep { kind: FlowStepKind::CopyText, value: String::new() },
            ],
        };
        let (trace, plan) = dry_run_flow_trace(&flow, "  hello\n   world  ", "");
        assert_eq!(trace.len(), 3);
        assert!(trace[0].contains("hello world"));
        assert!(plan.copy_text.unwrap().contains("DRY-RUN LLM(Summarize)"));
    }
}
