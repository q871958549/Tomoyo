//! Ephemeral Multi-Engine Translator Workbench (`__tomoyo_translator__`) bootstrap & handlers.

use serde_json::{json, Value};
use tomoyo_core::translate::{delete_secret, has_secret, set_secret, TranslateStore};
use tomoyo_core::{logs_dir, TomoyoConfig};

pub const TRANSLATOR_ENTRY: &str = "__tomoyo_translator__";

const TRANSLATOR_HTML_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/translator.html.gz"));

pub fn is_translator_entry(entry: &str) -> bool {
    entry == TRANSLATOR_ENTRY
}

pub fn assemble_translator_html() -> String {
    crate::inflate_embedded_html(TRANSLATOR_HTML_GZ)
}

pub fn read_last_translate_req() -> (String, String) {
    let path = logs_dir().join("last_translate_req.json");
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let v: Value = serde_json::from_str(&raw).unwrap_or_default();
    let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
    let target_lang = v
        .get("target_lang")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    (text, target_lang)
}

pub fn load_translator_bootstrap(theme: &str) -> String {
    let cfg = TomoyoConfig::load_or_create();
    let store = TranslateStore::load_default();
    let (text, req_tgt) = read_last_translate_req();
    let target_lang = if req_tgt.is_empty() {
        cfg.translate.target_lang.clone()
    } else {
        req_tgt
    };
    let payload = json!({
        "theme": theme,
        "text": text,
        "target_lang": target_lang,
        "engines": cfg.translate.engines,
        "history": store.history,
        "wordbook": store.wordbook,
    });
    format!(
        "window.__TOMOYO_TR_INIT__ = {};",
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
    )
}

#[cfg(any(windows, target_os = "linux"))]
pub fn read_latest_translate_req_event() -> String {
    let (text, target_lang) = read_last_translate_req();
    json!({
        "event": "translate_reload_req",
        "text": text,
        "target_lang": target_lang,
    })
    .to_string()
}

pub fn open_or_spawn_translator(text: &str, target_lang: &str, notify_hwnd: Option<isize>) {
    let req_dir = logs_dir();
    let _ = std::fs::create_dir_all(&req_dir);
    let req = json!({ "text": text, "target_lang": target_lang });
    let _ = std::fs::write(req_dir.join("last_translate_req.json"), req.to_string());

    #[cfg(windows)]
    {
        use crate::single_instance::{
            activate_window_with_msg, TRANSLATOR_WINDOW_CLASS, WM_APP_ACTIVATE_TRANSLATOR,
        };
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
        unsafe {
            if let Ok(hwnd) = FindWindowW(TRANSLATOR_WINDOW_CLASS, PCWSTR::null())
                && !hwnd.is_invalid() {
                    activate_window_with_msg(hwnd, WM_APP_ACTIVATE_TRANSLATOR);
                    return;
                }
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let theme = TomoyoConfig::load_or_create().theme;
        let mut cmd = std::process::Command::new(exe);
        cmd.args([
            "--plugin-id",
            "builtin.translate",
            "--title",
            "Tomoyo 多源翻译",
            "--entry",
            TRANSLATOR_ENTRY,
            "--theme",
            &theme,
            "--width",
            "780",
            "--height",
            "620",
        ]);
        if theme.eq_ignore_ascii_case("light") {
            cmd.arg("--light");
        }
        if let Some(h) = notify_hwnd {
            cmd.args(["--notify-hwnd", &h.to_string()]);
        }
        let _ = cmd.spawn();
    }
}

pub fn handle_tr_word_save(word: &str, translation: &str, phonetic: &str, note: &str) -> String {
    let mut store = TranslateStore::load_default();
    let ok = store.save_word(word, translation, phonetic, note);
    if ok {
        store.save_default();
    }
    json!({
        "event": "tr_store_updated",
        "action": "save_word",
        "ok": ok,
        "history": store.history,
        "wordbook": store.wordbook,
    })
    .to_string()
}

pub fn handle_tr_word_delete(word: &str) -> String {
    let mut store = TranslateStore::load_default();
    let ok = store.delete_word(word);
    if ok {
        store.save_default();
    }
    json!({
        "event": "tr_store_updated",
        "action": "delete_word",
        "ok": ok,
        "history": store.history,
        "wordbook": store.wordbook,
    })
    .to_string()
}

pub fn handle_tr_history_clear() -> String {
    let mut store = TranslateStore::load_default();
    store.clear_history();
    store.save_default();
    json!({
        "event": "tr_store_updated",
        "action": "clear_history",
        "ok": true,
        "history": store.history,
        "wordbook": store.wordbook,
    })
    .to_string()
}

pub fn secrets_status_json() -> Value {
    json!({
        "deepl": has_secret("deepl_api_key"),
        "openai": has_secret("openai_api_key"),
        "plugins": crate::plugin_rpc::collect_plugin_secrets_status(),
    })
}

pub fn handle_set_secret(key: &str, value: &str) -> String {
    match set_secret(key, value) {
        Ok(()) => json!({
            "event": "secrets_status",
            "ok": true,
            "message": if cfg!(windows) {
                "API 密钥已通过 Windows DPAPI 加密保存（不写入明文配置）"
            } else {
                "API 密钥已通过系统密钥环 / 本地加密保存（不写入明文配置）"
            },
            "status": secrets_status_json(),
        })
        .to_string(),
        Err(e) => json!({
            "event": "secrets_status",
            "ok": false,
            "message": e,
            "status": secrets_status_json(),
        })
        .to_string(),
    }
}

pub fn handle_delete_secret(key: &str) -> String {
    match delete_secret(key) {
        Ok(()) => json!({
            "event": "secrets_status",
            "ok": true,
            "message": "已清除保存的 API 密钥",
            "status": secrets_status_json(),
        })
        .to_string(),
        Err(e) => json!({
            "event": "secrets_status",
            "ok": false,
            "message": e,
            "status": secrets_status_json(),
        })
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translator_html_inlines_script_cleanly() {
        let html = assemble_translator_html();
        assert!(!html.contains("/*__TRANSLATOR_JS__*/"));
        assert!(html.contains("__TOMOYO_TR_INIT__"));
        assert!(html.contains("extra_dict"));
    }
}
