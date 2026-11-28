use super::*;

#[test]
fn full_event_query_rpc_and_privacy_filter_lifecycle() {
    let tmp = std::env::temp_dir().join(format!("tomoyo_clip_plug_ut_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let plug = ClipboardPlugin::with_db_path(tmp.clone());

    let mut settings = BTreeMap::new();
    settings.insert("excluded_apps".into(), json!("keepass.exe, bitwarden"));
    let mut secret_ev = PluginEventPayload::new(
        PluginEventKind::ClipboardText,
        json!({ "text": "super-secret-password", "source_app": "keepass.exe" }),
    );
    secret_ev.plugin_settings = settings.clone();
    let _ = plug.on_event(&secret_ev);

    let mut ok_ev = PluginEventPayload::new(
        PluginEventKind::ClipboardText,
        json!({ "text": "https://github.com/q871958549/Tomoyo", "source_app": "code" }),
    );
    ok_ev.plugin_settings = settings;
    let _ = plug.on_event(&ok_ev);

    let list = plug
        .call(&PluginCallRequest {
            method: "list".into(),
            params: json!({ "query": "" }),
            plugin_settings: BTreeMap::new(),
            plugin_secrets: BTreeMap::new(),
        })
        .unwrap();
    assert_eq!(list["total"], 1);
    assert_eq!(list["items"][0]["subtype"], "link");

    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
    let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
}
