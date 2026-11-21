//! Built-in Declarative Lightweight Workflow Provider (`flow` / `wf` + keyword / Pinyin match).
//!
//! Zero script runtime, zero background polling threads: loads `<sync_root>/flows.toml`
//! and executes pure-Rust pipelines via `InternalCommandKind::RunFlow`.

use std::sync::{Arc, RwLock};
use tomoyo_core::{
    flows_file_path, load_or_init_flows, score_match, IndexedText, Lang, Provider, TomoyoConfig,
    WorkflowDef,
};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, PreviewSpec, QueryContext,
    ResultCategory, ResultItem,
};

struct IndexedFlow {
    flow: WorkflowDef,
    indexed: IndexedText,
}

pub struct FlowsProvider {
    config: Arc<RwLock<TomoyoConfig>>,
    entries: RwLock<Vec<IndexedFlow>>,
}

impl FlowsProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        Self {
            config,
            entries: RwLock::new(index_flows(load_or_init_flows())),
        }
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }
}

fn index_flows(flows: Vec<WorkflowDef>) -> Vec<IndexedFlow> {
    flows
        .into_iter()
        .filter(|f| f.enabled)
        .map(|flow| {
            let search = format!("{} {} {} {} flow 工作流", flow.keyword, flow.id, flow.name, flow.description);
            let indexed = IndexedText::new(&search);
            IndexedFlow { flow, indexed }
        })
        .collect()
}

fn step_chain_summary(flow: &WorkflowDef, zh: bool) -> String {
    flow.steps
        .iter()
        .map(|s| if zh { s.kind.label_zh() } else { s.kind.label_en() })
        .collect::<Vec<_>>()
        .join(" → ")
}

impl Provider for FlowsProvider {
    fn id(&self) -> &'static str {
        "builtin.flows"
    }

    fn name(&self) -> &'static str {
        "Declarative Action Flows"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["flow", "wf"]
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let dedicated = ctx.keyword.is_some();
        let needle = if dedicated { ctx.args.trim() } else { ctx.trimmed.as_str() };
        if !dedicated && needle.chars().count() < 2 {
            return Vec::new();
        }
        let lang = self.lang();
        let zh = lang == Lang::Zh;
        let Ok(list) = self.entries.read() else {
            return Vec::new();
        };
        let flows_path = flows_file_path().to_string_lossy().into_owned();
        let (first_tok, rest_input) = match needle.split_once(char::is_whitespace) {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (needle, ""),
        };

        let mut out = Vec::new();
        for item in list.iter() {
            let f = &item.flow;
            let (score, input_arg) = if dedicated && needle.is_empty() {
                (8_600, "")
            } else if !f.keyword.is_empty() && f.keyword.eq_ignore_ascii_case(first_tok) {
                (9_850, rest_input)
            } else if f.id.eq_ignore_ascii_case(first_tok) {
                (9_750, rest_input)
            } else if let Some(s) = score_match(needle, &item.indexed) {
                (if dedicated { s + 1_300 } else { s }, "")
            } else {
                continue;
            };

            let chain = step_chain_summary(f, zh);
            let sub = if !input_arg.is_empty() {
                let preview_arg: String = input_arg.chars().take(28).collect();
                let ell = if input_arg.chars().count() > 28 { "…" } else { "" };
                if zh {
                    format!("⚡ 行内参数「{preview_arg}{ell}」· {chain}")
                } else {
                    format!("⚡ Input \"{preview_arg}{ell}\" · {chain}")
                }
            } else if f.description.is_empty() {
                chain.clone()
            } else {
                format!("{} · {chain}", f.description)
            };
            let mut kv = vec![
                (lang.pick("流程步骤", "Pipeline").into(), chain),
                (
                    lang.pick("触发词", "Keyword").into(),
                    if f.keyword.is_empty() { format!("flow {}", f.id) } else { f.keyword.clone() },
                ),
                (
                    lang.pick("输入来源", "Input").into(),
                    if !input_arg.is_empty() {
                        lang.pick("行内参数（优先于剪贴板）", "Inline argument (overrides clipboard)").into()
                    } else {
                        lang.pick("剪贴板回退（可在触发词后追加文本参数）", "Clipboard fallback (append text to override)").into()
                    },
                ),
            ];
            for (idx, step) in f.steps.iter().enumerate() {
                let lbl = if zh { step.kind.label_zh() } else { step.kind.label_en() };
                let val = if step.value.is_empty() { lbl.to_string() } else { format!("{lbl}: {}", step.value) };
                kv.push((format!("#{}", idx + 1), val));
            }

            out.push(ResultItem {
                id: format!("flow:{}", f.id),
                provider_id: self.id().into(),
                title: f.name.clone(),
                subtitle: sub,
                category: ResultCategory::Plugin,
                score,
                icon: IconSpec::Monogram("WF".into()),
                badge: Some(if f.hotkey.is_empty() { format!("{} 步", f.steps.len()) } else { f.hotkey.clone() }),
                copy_text: Some(f.name.clone()),
                actions: vec![
                    ActionSpec {
                        id: "run_flow".into(),
                        label: lang.pick("立即执行工作流", "Run Workflow").into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::InternalCommand {
                            command: InternalCommandKind::RunFlow {
                                flow_id: f.id.clone(),
                                input: input_arg.to_string(),
                            },
                        },
                    },
                    ActionSpec {
                        id: "edit_flows".into(),
                        label: lang.pick("编辑 flows.toml", "Edit flows.toml").into(),
                        shortcut: Some("Ctrl+Enter".into()),
                        kind: ActionKind::LaunchPath {
                            path: flows_path.clone(),
                            args: None,
                            working_dir: None,
                        },
                    },
                ],
                preview: Some(PreviewSpec::KeyValue {
                    title: format!("Flow · {}", f.name),
                    entries: kv,
                }),
            });
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.score));
        out.truncate(ctx.limit.min(20));
        out
    }

    fn reload(&self) {
        let fresh = index_flows(load_or_init_flows());
        if let Ok(mut guard) = self.entries.write() {
            *guard = fresh;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomoyo_core::default_workflows;

    #[test]
    fn passes_inline_arguments_and_shows_preview_for_parameterized_flow_queries() {
        let cfg = Arc::new(RwLock::new(TomoyoConfig::default()));
        let prov = FlowsProvider {
            config: cfg,
            entries: RwLock::new(index_flows(default_workflows())),
        };
        let ctx = QueryContext::new("aisum summarize this paragraph", 1, 10);
        let hits = prov.query(&ctx);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].id, "flow:clip_ai_summary");
        assert!(hits[0].subtitle.contains("summarize this paragraph"));
        match &hits[0].actions[0].kind {
            ActionKind::InternalCommand { command: InternalCommandKind::RunFlow { flow_id, input } } => {
                assert_eq!(flow_id, "clip_ai_summary");
                assert_eq!(input, "summarize this paragraph");
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }
}
