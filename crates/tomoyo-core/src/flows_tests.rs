use super::*;

#[test]
fn evaluates_multi_step_clipboard_clean_and_url_encode_pipeline() {
    let flow = WorkflowDef {
        id: "test_web".into(),
        name: "Clean Search".into(),
        keyword: "csearch".into(),
        hotkey: "Alt+Shift+S".into(),
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
        WorkflowDef { id: "".into(), name: "No ID".into(), keyword: "".into(), hotkey: "".into(), description: "".into(), enabled: true, steps: vec![FlowStep { kind: FlowStepKind::Trim, value: "".into() }] },
        WorkflowDef { id: "a".into(), name: "Empty".into(), keyword: "".into(), hotkey: "".into(), description: "".into(), enabled: true, steps: vec![] },
        WorkflowDef { id: "ok".into(), name: "Valid".into(), keyword: "OK".into(), hotkey: "Alt+Shift+V".into(), description: "".into(), enabled: true, steps: vec![FlowStep { kind: FlowStepKind::Trim, value: "".into() }] },
        WorkflowDef { id: "OK".into(), name: "Dup".into(), keyword: "".into(), hotkey: "not-a-key".into(), description: "".into(), enabled: true, steps: vec![FlowStep { kind: FlowStepKind::Trim, value: "".into() }] },
    ];
    let clean = sanitize_workflows(bad);
    assert_eq!(clean.len(), 1);
    assert_eq!(clean[0].id, "ok");
    assert_eq!(clean[0].keyword, "ok");
    assert_eq!(clean[0].hotkey, "Alt+Shift+V");
}

#[test]
fn inline_input_overrides_read_clipboard_and_feeds_llm_transform() {
    let flow = WorkflowDef {
        id: "ai_param".into(),
        name: "AI Param".into(),
        keyword: "aisum".into(),
        hotkey: String::new(),
        description: String::new(),
        enabled: true,
        steps: vec![
            FlowStep { kind: FlowStepKind::ReadClipboard, value: String::new() },
            FlowStep { kind: FlowStepKind::LlmTransform, value: "Summarize:".into() },
            FlowStep { kind: FlowStepKind::CopyText, value: String::new() },
        ],
    };
    let plan_inline = run_flow_pipeline_with_llm(&flow, "inline note", "stale clip", |p, t| {
        Ok(format!("{p} {t}"))
    })
    .unwrap();
    assert_eq!(plan_inline.copy_text.as_deref(), Some("Summarize: inline note"));

    let plan_clip = run_flow_pipeline_with_llm(&flow, "", "fresh clip", |p, t| {
        Ok(format!("{p} {t}"))
    })
    .unwrap();
    assert_eq!(plan_clip.copy_text.as_deref(), Some("Summarize: fresh clip"));
}
