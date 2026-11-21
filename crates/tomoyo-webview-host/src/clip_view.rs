//! Ephemeral CopyQ-grade Clipboard Workbench (`__tomoyo_clipboard_manager__`) backend &
//! running process enumeration for the Settings Center Privacy App Searchable Combobox.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tomoyo_core::{data_dir, TomoyoConfig};
use tomoyo_providers::{ClipboardEntry, ClipboardPayload, ClipboardStore, ParsedClipQuery};

pub const CLIP_MANAGER_ENTRY: &str = "__tomoyo_clipboard_manager__";

const CLIP_HTML_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/clipboard.html.gz"));

pub fn is_clip_manager_entry(entry: &str) -> bool {
    entry == CLIP_MANAGER_ENTRY
}

pub fn assemble_clip_manager_html() -> String {
    crate::inflate_embedded_html(CLIP_HTML_GZ)
}

pub struct ClipManagerBootstrap {
    pub init_script: String,
    pub images_dir: PathBuf,
}

fn open_default_store() -> ClipboardStore {
    let cfg = TomoyoConfig::load_or_create();
    ClipboardStore::open(Some(data_dir().join("clipboard.db")), cfg.clipboard.max_persisted_items)
}

pub fn load_clip_manager_bootstrap(theme: &str, focus_key: &str) -> ClipManagerBootstrap {
    let store = open_default_store();
    let cfg = TomoyoConfig::load_or_create();
    let images_dir = data_dir().join("clipboard-images");
    let _ = std::fs::create_dir_all(&images_dir);
    let payload = build_list_payload(&store, "", theme, focus_key, cfg.clipboard.capture_enabled);
    let init_script = format!(
        "window.__TOMOYO_CLIP_INIT__ = {};",
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
    );
    ClipManagerBootstrap { init_script, images_dir }
}

pub fn handle_clip_list(query: &str) -> String {
    let store = open_default_store();
    let cfg = TomoyoConfig::load_or_create();
    let payload = build_list_payload(&store, query, &cfg.theme, "", cfg.clipboard.capture_enabled);
    serde_json::json!({ "event": "clip_list_loaded", "data": payload }).to_string()
}

pub fn handle_clip_toggle_pin(id_key: &str) -> (String, bool) {
    let store = open_default_store();
    let pinned = store.toggle_pin(id_key);
    let evt = serde_json::json!({ "event": "clip_mutated", "action": "pin", "pinned": pinned, "key": id_key });
    (evt.to_string(), true)
}

pub fn handle_clip_update(id_key: &str, text: Option<String>, tag: &str, note: &str) -> (String, bool) {
    let store = open_default_store();
    match store.update_item(id_key, text, tag, note) {
        Ok(new_key) => (
            serde_json::json!({ "event": "clip_mutated", "ok": true, "action": "update", "key": new_key }).to_string(),
            true,
        ),
        Err(e) => (
            serde_json::json!({ "event": "clip_mutated", "ok": false, "action": "update", "message": e }).to_string(),
            false,
        ),
    }
}

pub fn handle_clip_delete(id_key: &str) -> (String, bool) {
    let store = open_default_store();
    let ok = store.delete_item(id_key);
    (serde_json::json!({ "event": "clip_mutated", "ok": ok, "action": "delete" }).to_string(), ok)
}

pub fn handle_clip_clear_unpinned() -> (String, bool) {
    let store = open_default_store();
    let n = store.clear_unpinned();
    (serde_json::json!({ "event": "clip_mutated", "ok": true, "action": "clear", "count": n }).to_string(), n > 0)
}

pub fn handle_clip_export() -> String {
    let store = open_default_store();
    let json = store.export_json();
    serde_json::json!({ "event": "clip_exported", "json": json }).to_string()
}

pub fn handle_clip_import(raw_json: &str) -> (String, bool) {
    let store = open_default_store();
    match store.import_json(raw_json) {
        Ok(n) => (
            serde_json::json!({ "event": "clip_mutated", "ok": true, "action": "import", "count": n }).to_string(),
            n > 0,
        ),
        Err(e) => (
            serde_json::json!({ "event": "clip_mutated", "ok": false, "action": "import", "message": e }).to_string(),
            false,
        ),
    }
}

fn build_list_payload(
    store: &ClipboardStore,
    query: &str,
    theme: &str,
    focus_key: &str,
    capture_enabled: bool,
) -> serde_json::Value {
    let parsed = ParsedClipQuery::parse(query);
    let entries = store.query_filtered(&parsed, 200);
    let items: Vec<serde_json::Value> = entries.into_iter().map(entry_to_json).collect();
    let tags: Vec<serde_json::Value> = store
        .list_tags()
        .into_iter()
        .map(|(tag, count)| serde_json::json!({ "tag": tag, "count": count }))
        .collect();
    serde_json::json!({
        "theme": theme,
        "focus_key": focus_key,
        "total": store.item_count(),
        "capture_enabled": capture_enabled,
        "tags": tags,
        "items": items,
    })
}

fn entry_to_json(e: ClipboardEntry) -> serde_json::Value {
    let subtype = e.subtype().as_str().to_string();
    let (kind, text, image_url, image_path, width, height, files) = match e.payload {
        ClipboardPayload::Text { text, .. } => ("text", text, None, None, 0, 0, Vec::new()),
        ClipboardPayload::Image { path, width, height } => {
            let url = Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|f| format!("https://tomoyo-clip-img.local/{f}"));
            ("image", String::new(), url, Some(path), width, height, Vec::new())
        }
        ClipboardPayload::Files { paths } => ("files", paths.join("\n"), None, None, 0, 0, paths),
    };
    serde_json::json!({
        "key": e.key,
        "kind": kind,
        "subtype": subtype,
        "preview": e.preview_line,
        "text": text,
        "image_url": image_url,
        "image_path": image_path,
        "width": width,
        "height": height,
        "files": files,
        "ts": e.captured_at_unix,
        "pinned": e.pinned,
        "tag": e.tag,
        "note": e.note,
        "source_app": e.source_app,
    })
}

/// Enumerates curated privacy apps + currently running Windows desktop `.exe` names
/// for the Settings Center Searchable Combobox.
pub fn enumerate_running_app_exes() -> Vec<String> {
    let mut set = BTreeSet::new();
    for preset in [
        "keepass.exe",
        "keepassxc.exe",
        "bitwarden.exe",
        "1password.exe",
        "enpass.exe",
        "lastpass.exe",
        "mstsc.exe",
        "todesk.exe",
        "anydesk.exe",
        "wechat.exe",
        "qq.exe",
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "code.exe",
    ] {
        set.insert(preset.to_string());
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        unsafe {
            if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut pe = PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                if Process32FirstW(snap, &mut pe).is_ok() {
                    loop {
                        let len = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                        let name = String::from_utf16_lossy(&pe.szExeFile[..len]).to_ascii_lowercase();
                        if name.ends_with(".exe")
                            && !matches!(name.as_str(), "svchost.exe" | "conhost.exe" | "csrss.exe" | "smss.exe" | "services.exe" | "lsass.exe" | "dwm.exe" | "tomoyo.exe" | "tomoyo-webview-host.exe")
                        {
                            set.insert(name);
                        }
                        if Process32NextW(snap, &mut pe).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snap);
            }
        }
    }
    set.into_iter().take(120).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_workbench_html_inlines_js_cleanly() {
        let html = assemble_clip_manager_html();
        assert!(html.contains("__TOMOYO_CLIP_INIT__"));
        assert!(html.contains("clip_update"));
        assert!(html.contains("clip_clear_unpinned"));
        assert!(!html.contains("/*__CLIP_JS__*/"));
    }
}
