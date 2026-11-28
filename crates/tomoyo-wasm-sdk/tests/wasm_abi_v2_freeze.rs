//! Contract freeze tests for Tomoyo WASM Plugin ABI v2 (`tomoyo-wasm-sdk`).

use tomoyo_wasm_sdk::{
    export_tomoyo_wasm_plugin, host, ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse,
    QueryContext, QueryResponse, ResultCategory, ResultItem, TomoyoWasmPlugin, WASM_ABI_VERSION,
};

struct ContractWasmPlugin;

impl TomoyoWasmPlugin for ContractWasmPlugin {
    fn query(ctx: &QueryContext) -> Vec<ResultItem> {
        vec![ResultItem {
            id: "wasm-echo".into(),
            provider_id: "dev.tomoyo.contract".into(),
            title: format!("echo:{}", ctx.trimmed),
            subtitle: "wasm-abi-v2".into(),
            category: ResultCategory::Plugin,
            score: 100,
            icon: Default::default(),
            badge: Some("WASM".into()),
            copy_text: Some(ctx.trimmed.clone()),
            actions: vec![ActionSpec {
                id: "copy".into(),
                label: "Copy".into(),
                shortcut: None,
                kind: ActionKind::CopyText {
                    text: ctx.trimmed.clone(),
                    paste_immediately: false,
                },
            }],
            preview: None,
        }]
    }

    fn execute(req: &ExecuteRequest) -> ExecuteResponse {
        ExecuteResponse {
            hide_window: false,
            toast: Some(format!("exec:{}", req.action_id)),
            ..Default::default()
        }
    }
}

export_tomoyo_wasm_plugin!(ContractWasmPlugin);

#[test]
fn freezes_wasm_abi_v2_exports_trait_contract_and_host_stubs() {
    assert_eq!(WASM_ABI_VERSION, 2);

    // Freeze exact exported C-ABI signatures expected by Tomoyo's `wasmi` host runtime.
    let _alloc: extern "C" fn(i32) -> i32 = tomoyo_alloc;
    let _free: extern "C" fn(i32, i32) = tomoyo_free;
    let _query: extern "C" fn(i32, i32) -> i64 = tomoyo_query;
    let _execute: extern "C" fn(i32, i32) -> i64 = tomoyo_execute;

    let ctx = QueryContext::new("abc", 7, 10);
    let resp = QueryResponse {
        items: ContractWasmPlugin::query(&ctx),
    };
    let roundtrip_q: QueryResponse =
        serde_json::from_slice(&serde_json::to_vec(&resp).unwrap()).unwrap();
    assert_eq!(roundtrip_q.items.len(), 1);
    assert_eq!(roundtrip_q.items[0].title, "echo:abc");

    let exec_req = ExecuteRequest::new("run", "wasm-echo", serde_json::json!({}));
    let exec_resp = ContractWasmPlugin::execute(&exec_req);
    let roundtrip_e: ExecuteResponse =
        serde_json::from_slice(&serde_json::to_vec(&exec_resp).unwrap()).unwrap();
    assert!(!roundtrip_e.hide_window);
    assert_eq!(roundtrip_e.toast.as_deref(), Some("exec:run"));

    assert_eq!(host::now_unix(), 0);
    assert_eq!(host::now_ms(), 0);
    assert!(host::kv_get("k").is_none());
    assert!(!host::kv_set("k", "v"));
    host::log("abi-v2");
}
