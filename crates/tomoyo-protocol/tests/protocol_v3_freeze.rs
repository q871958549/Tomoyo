//! Contract freeze & backward-compatibility test suite for Tomoyo Plugin Protocol v3.

use serde_json::json;
use std::collections::BTreeMap;
use tomoyo_protocol::*;

#[test]
fn freezes_protocol_version_and_jsonrpc_method_names() {
    assert_eq!(PROTOCOL_VERSION, 3);
    assert_eq!(METHOD_HANDSHAKE, "tomoyo/handshake");
    assert_eq!(METHOD_QUERY, "tomoyo/query");
    assert_eq!(METHOD_EXECUTE, "tomoyo/execute");
    assert_eq!(METHOD_EVENT, "tomoyo/event");
    assert_eq!(METHOD_CALL, "tomoyo/call");
    assert_eq!(METHOD_SHUTDOWN, "tomoyo/shutdown");
}

#[test]
fn maintains_v1_manifest_and_query_backward_compatibility() {
    let v1_toml = r#"
id = "dev.tomoyo.legacy-v1"
name = "Legacy v1"
version = "1.0.0"

[runtime]
kind = "stdio_jsonrpc"
command = "legacy-bin"
"#;
    let m = PluginManifest::from_toml_str(v1_toml).unwrap();
    assert_eq!(m.id, "dev.tomoyo.legacy-v1");
    assert!(m.ui.is_none());
    assert!(m.hotkeys.is_empty());
    assert!(m.tray_actions.is_empty());
    assert!(m.file_actions.is_empty());
    assert!(m.text_actions.is_empty());

    let ctx_v1_json = json!({
        "raw": "hello world",
        "trimmed": "hello world"
    });
    let ctx: QueryContext = serde_json::from_value(ctx_v1_json).unwrap();
    assert_eq!(ctx.limit, 20);
    assert!(ctx.plugin_secrets.is_empty());
    assert!(ctx.clipboard.is_none());
}

#[test]
fn roundtrips_frozen_v3_wire_messages_and_rich_cards() {
    let hs = HandshakeResult {
        protocol_version: PROTOCOL_VERSION,
        name: "Contract Probe".into(),
        version: "3.5.0".into(),
    };
    let hs_val = serde_json::to_value(&hs).unwrap();
    let hs_de: HandshakeResult = serde_json::from_value(hs_val).unwrap();
    assert_eq!(hs_de.protocol_version, 3);

    let item = ResultItem {
        id: "item-1".into(),
        provider_id: "dev.tomoyo.ai".into(),
        title: "Semantic Match".into(),
        subtitle: "notes/architecture.md".into(),
        category: ResultCategory::Plugin,
        score: 9200,
        icon: IconSpec::Symbol("AI".into()),
        badge: Some("BM25".into()),
        copy_text: Some("excerpt".into()),
        actions: vec![ActionSpec {
            id: "run".into(),
            label: "Open".into(),
            shortcut: Some("Enter".into()),
            kind: ActionKind::PluginExecute {
                plugin_id: "dev.tomoyo.ai".into(),
                action_id: "open".into(),
                payload: json!({"path": "notes/architecture.md"}),
            },
        }],
        preview: Some(PreviewSpec::RichCard {
            title: "Architecture".into(),
            subtitle: "Markdown Note".into(),
            badges: vec!["local".into(), "offline".into()],
            rows: vec![("Engine".into(), "BM25+CJK".into())],
            body: "Zero resident RAM".into(),
        }),
    };
    let qr = QueryResponse { items: vec![item] };
    let qr_json = serde_json::to_string(&qr).unwrap();
    let qr_de: QueryResponse = serde_json::from_str(&qr_json).unwrap();
    assert_eq!(qr_de.items.len(), 1);
    assert!(matches!(qr_de.items[0].preview, Some(PreviewSpec::RichCard { .. })));

    let mut secrets = BTreeMap::new();
    secrets.insert("api_key".into(), "tok-secret".into());
    let exec_req = ExecuteRequest {
        action_id: "call".into(),
        item_id: "item-1".into(),
        payload: json!({"q": "ping"}),
        plugin_settings: BTreeMap::new(),
        plugin_secrets: secrets,
    };
    let exec_req_de: ExecuteRequest =
        serde_json::from_str(&serde_json::to_string(&exec_req).unwrap()).unwrap();
    assert_eq!(exec_req_de.plugin_secrets.get("api_key").map(String::as_str), Some("tok-secret"));

    let exec_resp = ExecuteResponse {
        hide_window: false,
        refresh_query: true,
        toast: Some("Opened".into()),
        open_webview: Some(OpenWebViewRequest {
            entry: Some("ui/index.html".into()),
            title: Some("Inspector".into()),
            initial_query: "test".into(),
            width: Some(820),
            height: Some(600),
        }),
        view: Some(ViewModel::Detail {
            title: "Result".into(),
            body: "OK".into(),
        }),
        ..Default::default()
    };
    let exec_resp_de: ExecuteResponse =
        serde_json::from_str(&serde_json::to_string(&exec_resp).unwrap()).unwrap();
    assert!(!exec_resp_de.hide_window);
    assert!(exec_resp_de.refresh_query);
    assert!(exec_resp_de.open_webview.is_some());
    assert!(matches!(exec_resp_de.view, Some(ViewModel::Detail { .. })));

    let ev = PluginEventPayload::new(PluginEventKind::HotkeyTriggered, json!({"selected_text": "foo"}));
    let ev_de: PluginEventPayload =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    assert_eq!(ev_de.event, PluginEventKind::HotkeyTriggered);

    let call = PluginCallRequest {
        method: "ping".into(),
        params: json!({"n": 1}),
        ..Default::default()
    };
    let call_de: PluginCallRequest =
        serde_json::from_str(&serde_json::to_string(&call).unwrap()).unwrap();
    assert_eq!(call_de.method, "ping");
}
