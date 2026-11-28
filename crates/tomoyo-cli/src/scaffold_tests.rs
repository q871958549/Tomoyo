use super::*;
use tomoyo_protocol::PluginManifest;

#[test]
fn external_rust_and_wasm_scaffolds_use_git_fallback_and_valid_manifests() {
    let base = std::env::temp_dir().join(format!("tomoyo_scaf_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    let py_dir = base.join("py");
    let rs_dir = base.join("rs");
    let ws_dir = base.join("ws");
    let wv_dir = base.join("wv");
    for d in [&py_dir, &rs_dir, &ws_dir, &wv_dir] {
        std::fs::create_dir_all(d).unwrap();
    }
    scaffold_python(&py_dir, "Py Demo", "py-demo").unwrap();
    scaffold_rust(&rs_dir, "Rs Demo", "rs-demo", None).unwrap();
    scaffold_wasm(&ws_dir, "Ws Demo", "ws-demo", None).unwrap();
    scaffold_webview(&wv_dir, "Wv Demo", "wv-demo").unwrap();

    for d in [&py_dir, &rs_dir, &ws_dir, &wv_dir] {
        let raw = std::fs::read_to_string(d.join("tomoyo-plugin.toml")).unwrap();
        PluginManifest::from_toml_str(&raw).unwrap();
    }
    let rs_cargo = std::fs::read_to_string(rs_dir.join("Cargo.toml")).unwrap();
    assert!(rs_cargo.contains("https://github.com/q871958549/Tomoyo"));
    assert!(wv_dir.join("tomoyo.d.ts").is_file());

    let _ = std::fs::remove_dir_all(&base);
}
