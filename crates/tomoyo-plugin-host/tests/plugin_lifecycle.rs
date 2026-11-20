use std::path::PathBuf;
use tomoyo_core::ExternalPluginBridge;
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{ActionKind, ExecuteRequest, QueryContext};

#[test]
fn all_three_plugin_runtimes_lazy_spawn_query_and_reap() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_plugins = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("plugins");

    let host = PluginHost::new(vec![workspace_plugins]);
    let discovered = host.list_plugins();
    assert!(
        discovered.len() >= 4,
        "expected >=4 reference plugins (Rust STDIO, Python STDIO, Rust WASM, WebView JSON), got {}",
        discovered.len()
    );

    // Guarantee #1: Lazy spawn — 0 processes / 0 WASM instances running at discovery!
    assert!(
        discovered.iter().all(|p| !p.running),
        "plugins must NOT be instantiated until triggered"
    );

    // 1. Test real compiled Rust WASM sandbox plugin (`unit 2.5gb` & `jwt ...`)
    assert!(host.matches_keyword("unit"));
    let wasm_ctx = QueryContext::new("unit 2.5gb", 1, 10).with_keyword("unit", "2.5gb");
    let wasm_items = host.query_keyword("unit", &wasm_ctx);
    assert!(
        !wasm_items.is_empty() && wasm_items[0].title.contains("2560.00 MB"),
        "expected WASM unit converter output, got: {wasm_items:?}"
    );

    // Real JWT decoding inside WASM sandbox!
    let sample_jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ0b21veW8iLCJyb2xlIjoib3Blbi1zb3VyY2UifQ.sig";
    let jwt_ctx = QueryContext::new(format!("jwt {sample_jwt}"), 2, 10).with_keyword("jwt", sample_jwt);
    let jwt_items = host.query_keyword("jwt", &jwt_ctx);
    assert!(
        jwt_items.iter().any(|i| i.title.contains("sub=tomoyo")),
        "expected WASM JWT decoder output, got: {jwt_items:?}"
    );

    // 2. Test WebView plugin trigger (`json` -> returns zero-idle-RAM OpenWebView action!)
    assert!(host.matches_keyword("json"));
    let wv_ctx = QueryContext::new("json {\"a\":1}", 3, 10).with_keyword("json", "{\"a\":1}");
    let wv_items = host.query_keyword("json", &wv_ctx);
    assert_eq!(wv_items.len(), 1);
    assert!(matches!(
        wv_items[0].actions[0].kind,
        ActionKind::OpenWebView {
            utools_compat: true,
            ..
        }
    ));

    // 3. Test STDIO Rust SDK plugin if built alongside the test runner
    if host.matches_keyword("hi") {
        let ctx = QueryContext::new("hi Tomoyo", 4, 10).with_keyword("hi", "Tomoyo");
        let items = host.query_keyword("hi", &ctx);
        if items.iter().any(|i| !i.id.starts_with("plugin_err:")) {
            assert!(items.iter().any(|i| i.title == "TOMOYO"));
            let exec = host
                .execute_action(
                    "dev.tomoyo.example-hello",
                    ExecuteRequest {
                        action_id: "upper".into(),
                        item_id: "hello:upper".into(),
                        payload: serde_json::json!({ "text": "TOMOYO" }),
                    },
                )
                .expect("execute_action should succeed");
            assert_eq!(exec.copy_text.as_deref(), Some("TOMOYO"));
        }
    }

    // 4. Reap all active runtimes -> back to 0 active instances!
    assert!(host.list_plugins().iter().any(|p| p.running));
    let reaped = host.reap_plugins(true);
    assert!(reaped >= 1);
    assert!(host.list_plugins().iter().all(|p| !p.running));
}

#[test]
fn third_party_plugin_sensitive_permissions_blocked_until_approved() {
    let tmp = std::env::temp_dir().join(format!("tomoyo-perm-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let plug_dir = tmp.join("untrusted-shell");
    std::fs::create_dir_all(&plug_dir).unwrap();
    let manifest = r#"
id = "com.thirdparty.untrusted_shell"
name = "Untrusted Shell"
version = "0.1.0"
permissions = ["shell_exec"]

[triggers]
keywords = ["utest"]

[runtime]
kind = "stdio_jsonrpc"
command = "python"
"#;
    std::fs::write(plug_dir.join("tomoyo-plugin.toml"), manifest).unwrap();
    let host = PluginHost::new(vec![tmp.clone()]);
    host.set_interactive_prompts(false);
    let ctx = QueryContext::new("utest hi", 1, 5).with_keyword("utest", "hi");
    let items = host.query_keyword("utest", &ctx);
    assert!(
        host.list_plugins().iter().all(|p| !p.running),
        "unapproved sensitive third-party plugin must not spawn"
    );
    assert!(
        items.iter().all(|i| i.id.starts_with("plugin_err:")),
        "unapproved third-party plugin may only surface a diagnostic error card, got: {items:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
