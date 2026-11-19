//! Generates the injected `window.tomoyo` and optional `window.utools` compatibility JS bridge,
//! dispatches WebView2 host messages safely, and implements SHA-256 verified In-App Plugin Store
//! & GitHub Releases Auto-Update checks.

use crate::db::PluginDb;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use tomoyo_plugin_host::{default_plugin_dirs, PluginHost};
use tomoyo_protocol::PluginRegistryIndex;

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum HostMessage {
    CopyText { text: String },
    CopyAndPaste { text: String },
    OpenExternal { url: String },
    OpenPath { path: String },
    DbPut { id: String, doc: Value },
    DbRemove { id: String },
    SaveSettings { config: tomoyo_core::TomoyoConfig, autostart: bool },
    SyncTheme { theme: String },
    TogglePinTopmost,
    CheckUpdates { repo: String },
    FetchStore { sources: Vec<String> },
    InstallPlugin {
        plugin_id: String,
        source: String,
        sha256: String,
    },
    UninstallPlugin { plugin_id: String },
    Close,
}

#[derive(Debug, Default)]
pub struct HostAction {
    pub should_close: bool,
    pub apply_theme: Option<String>,
    pub toggle_topmost: bool,
    pub reply_json: Option<String>,
    pub async_check_updates: Option<String>,
    pub async_fetch_store: Option<Vec<String>>,
}

/// Builds the initialization JavaScript injected before document scripts run.
/// Provides synchronous `utools.db` / `utools.dbStorage`, `utools.setSubInput`,
/// `utools.hideMainWindowPasteText`, and `Ctrl+Shift+T` window pin toggle!
pub fn build_init_script(
    plugin_id: &str,
    initial_query: &str,
    is_dark: bool,
    utools_compat: bool,
    db_snapshot_json: &str,
) -> String {
    let plugin_id_js = serde_json::to_string(plugin_id).unwrap_or_else(|_| "\"\"".into());
    let query_js = serde_json::to_string(initial_query).unwrap_or_else(|_| "\"\"".into());
    let dark_js = if is_dark { "true" } else { "false" };
    let utools_js = if utools_compat { "true" } else { "false" };

    format!(
        r#"(function() {{
  if (window.__TOMOYO_BOOTSTRAPPED__) return;
  window.__TOMOYO_BOOTSTRAPPED__ = true;

  const PLUGIN_ID = {plugin_id_js};
  const INITIAL_QUERY = {query_js};
  const IS_DARK = {dark_js};
  const UTOOLS_COMPAT = {utools_js};
  const _store = {db_snapshot_json};
  const _enterHandlers = [];
  const _readyHandlers = [];
  const _outHandlers = [];
  let _subInputBar = null;

  function post(msg) {{
    try {{
      if (window.chrome && window.chrome.webview) {{
        window.chrome.webview.postMessage(JSON.stringify(msg));
      }}
    }} catch (_) {{}}
  }}

  function ensureSubInput(onChange, placeholder, isFocus) {{
    if (!document.body) {{
      window.addEventListener('DOMContentLoaded', () => ensureSubInput(onChange, placeholder, isFocus), {{ once: true }});
      return;
    }}
    if (_subInputBar) _subInputBar.remove();
    const bar = document.createElement('div');
    bar.id = '__tomoyo_sub_input_bar__';
    bar.style.cssText = 'position:sticky;top:0;z-index:99999;padding:8px 12px;background:' +
      (IS_DARK ? '#181924;border-bottom:1px solid #2c2e40;' : '#ffffff;border-bottom:1px solid #e1e5ef;') +
      'display:flex;align-items:center;gap:8px;';
    const inp = document.createElement('input');
    inp.type = 'text';
    inp.placeholder = placeholder || '输入过滤内容...';
    inp.style.cssText = 'flex:1;padding:6px 10px;border-radius:7px;outline:none;font-size:13px;' +
      (IS_DARK ? 'background:#13141c;color:#f3f4f8;border:1px solid #2c2e40;' : 'background:#f6f7fb;color:#151822;border:1px solid #d5dbe7;');
    inp.addEventListener('input', () => {{
      if (typeof onChange === 'function') {{
        try {{ onChange({{ text: inp.value }}); }} catch (e) {{ console.error(e); }}
      }}
    }});
    bar.appendChild(inp);
    document.body.prepend(bar);
    _subInputBar = bar;
    if (isFocus !== false) inp.focus();
  }}

  function removeSubInput() {{
    if (_subInputBar) {{
      _subInputBar.remove();
      _subInputBar = null;
    }}
  }}

  const db = {{
    put(doc) {{
      if (!doc || typeof doc._id !== 'string') {{
        return {{ id: '', error: true, message: 'doc._id is required' }};
      }}
      const prev = _store[doc._id];
      const nextRevNum = prev && prev._rev ? (parseInt(String(prev._rev).split('-')[0], 10) || 0) + 1 : 1;
      const copy = Object.assign({{}}, doc, {{ _rev: nextRevNum + '-tomoyo' }});
      _store[doc._id] = copy;
      post({{ cmd: 'db_put', id: doc._id, doc: copy }});
      return {{ id: doc._id, rev: copy._rev, ok: true }};
    }},
    get(id) {{
      const v = _store[id];
      return v ? JSON.parse(JSON.stringify(v)) : null;
    }},
    remove(docOrId) {{
      const id = typeof docOrId === 'string' ? docOrId : (docOrId && docOrId._id);
      if (!id || !_store[id]) return {{ id: id || '', error: true, message: 'not_found' }};
      delete _store[id];
      post({{ cmd: 'db_remove', id }});
      return {{ id, ok: true }};
    }},
    allDocs(key) {{
      const keys = Object.keys(_store);
      let filtered = keys;
      if (typeof key === 'string') {{
        filtered = keys.filter(k => k.startsWith(key));
      }} else if (Array.isArray(key)) {{
        filtered = keys.filter(k => key.includes(k));
      }}
      return filtered.map(k => JSON.parse(JSON.stringify(_store[k])));
    }}
  }};

  const dbStorage = {{
    setItem(key, value) {{
      const id = '__kv:' + key;
      db.put({{ _id: id, value }});
    }},
    getItem(key) {{
      const doc = db.get('__kv:' + key);
      return doc ? doc.value : null;
    }},
    removeItem(key) {{
      db.remove('__kv:' + key);
    }}
  }};

  window.tomoyo = {{
    version: '0.4.0',
    platform: 'win32',
    pluginId: PLUGIN_ID,
    initialQuery: INITIAL_QUERY,
    isDarkColors: () => IS_DARK,
    copyText: (text) => {{ post({{ cmd: 'copy_text', text: String(text ?? '') }}); return true; }},
    openExternal: (url) => post({{ cmd: 'open_external', url: String(url ?? '') }}),
    openPath: (path) => post({{ cmd: 'open_path', path: String(path ?? '') }}),
    togglePinTopmost: () => post({{ cmd: 'toggle_pin_topmost' }}),
    close: () => post({{ cmd: 'close' }}),
    onEnter: (cb) => {{ if (typeof cb === 'function') _enterHandlers.push(cb); }},
    db,
    dbStorage
  }};

  if (UTOOLS_COMPAT) {{
    window.utools = {{
      onPluginEnter: (cb) => {{ if (typeof cb === 'function') _enterHandlers.push(cb); }},
      onPluginReady: (cb) => {{ if (typeof cb === 'function') _readyHandlers.push(cb); }},
      onPluginOut: (cb) => {{ if (typeof cb === 'function') _outHandlers.push(cb); }},
      setSubInput: (onChange, placeholder, isFocus) => {{ ensureSubInput(onChange, placeholder, isFocus); return true; }},
      removeSubInput: () => {{ removeSubInput(); return true; }},
      setSubInputValue: (val) => {{
        const inp = _subInputBar && _subInputBar.querySelector('input');
        if (inp) {{ inp.value = String(val ?? ''); inp.dispatchEvent(new Event('input')); }}
        return true;
      }},
      copyText: (text) => window.tomoyo.copyText(text),
      hideMainWindow: () => window.tomoyo.close(),
      hideMainWindowPasteText: (text) => {{ post({{ cmd: 'copy_and_paste', text: String(text ?? '') }}); return true; }},
      outPlugin: () => window.tomoyo.close(),
      shellOpenExternal: (url) => window.tomoyo.openExternal(url),
      shellOpenPath: (path) => window.tomoyo.openPath(path),
      showNotification: (body) => console.info('[tomoyo notification]', body),
      isDarkColors: () => IS_DARK,
      isWindows: () => true,
      isMacOs: () => false,
      isLinux: () => false,
      db,
      dbStorage
    }};
  }}

  window.addEventListener('DOMContentLoaded', () => {{
    const payload = {{ code: PLUGIN_ID, type: 'text', payload: INITIAL_QUERY }};
    _readyHandlers.forEach(fn => {{ try {{ fn(); }} catch (e) {{ console.error(e); }} }});
    _enterHandlers.forEach(fn => {{ try {{ fn(payload); }} catch (e) {{ console.error(e); }} }});
  }});

  window.addEventListener('keydown', (e) => {{
    if (e.ctrlKey && e.shiftKey && (e.key === 'T' || e.key === 't')) {{
      e.preventDefault();
      window.tomoyo.togglePinTopmost();
      return;
    }}
    if (e.key === 'Escape' && !e.defaultPrevented) {{
      window.tomoyo.close();
    }}
  }});
}})();"#
    )
}

pub const WM_APP_CONFIG_SAVED: u32 = 0x8000 + 20;

pub fn handle_host_message(raw: &str, db: &mut PluginDb, notify_hwnd: Option<isize>) -> HostAction {
    let Ok(msg) = serde_json::from_str::<HostMessage>(raw) else {
        return HostAction::default();
    };
    match msg {
        HostMessage::CopyText { text } => {
            set_clipboard_text(&text);
            HostAction::default()
        }
        HostMessage::CopyAndPaste { text } => {
            set_clipboard_text(&text);
            HostAction {
                should_close: true,
                ..Default::default()
            }
        }
        HostMessage::OpenExternal { url } => {
            shell_open(&url);
            HostAction::default()
        }
        HostMessage::OpenPath { path } => {
            shell_open(&path);
            HostAction::default()
        }
        HostMessage::DbPut { id, doc } => {
            db.put(id, doc);
            HostAction::default()
        }
        HostMessage::DbRemove { id } => {
            db.remove(&id);
            HostAction::default()
        }
        HostMessage::SaveSettings { config, autostart } => {
            let theme = config.theme.clone();
            let _ = config.save();
            let _ = tomoyo_core::set_autostart_enabled(autostart);
            if let Some(target) = notify_hwnd {
                notify_parent_reload(target);
            }
            HostAction {
                apply_theme: Some(theme),
                ..Default::default()
            }
        }
        HostMessage::SyncTheme { theme } => HostAction {
            apply_theme: Some(theme),
            ..Default::default()
        },
        HostMessage::TogglePinTopmost => HostAction {
            toggle_topmost: true,
            ..Default::default()
        },
        HostMessage::CheckUpdates { repo } => HostAction {
            async_check_updates: Some(repo),
            ..Default::default()
        },
        HostMessage::FetchStore { sources } => HostAction {
            async_fetch_store: Some(sources),
            ..Default::default()
        },
        HostMessage::InstallPlugin {
            plugin_id,
            source,
            sha256,
        } => {
            let res = install_plugin_from_source(&plugin_id, &source, &sha256, notify_hwnd);
            let evt = match res {
                Ok(dest) => serde_json::json!({
                    "event": "store_action_result",
                    "ok": true,
                    "action": "install",
                    "plugin_id": plugin_id,
                    "message": format!("SHA-256 校验通过，已安装到 {dest}")
                }),
                Err(e) => serde_json::json!({
                    "event": "store_action_result",
                    "ok": false,
                    "action": "install",
                    "plugin_id": plugin_id,
                    "message": e
                }),
            };
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::UninstallPlugin { plugin_id } => {
            let res = uninstall_user_plugin(&plugin_id, notify_hwnd);
            let evt = match res {
                Ok(()) => serde_json::json!({
                    "event": "store_action_result",
                    "ok": true,
                    "action": "uninstall",
                    "plugin_id": plugin_id,
                    "message": "已卸载并热重载主窗口插件列表"
                }),
                Err(e) => serde_json::json!({
                    "event": "store_action_result",
                    "ok": false,
                    "action": "uninstall",
                    "plugin_id": plugin_id,
                    "message": e
                }),
            };
            HostAction {
                reply_json: Some(evt.to_string()),
                ..Default::default()
            }
        }
        HostMessage::Close => HostAction {
            should_close: true,
            ..Default::default()
        },
    }
}

/// Loads plugin registry entries from local `registry/index.json` or HTTPS URLs and annotates
/// which ones are already installed (built-in vs user-removable).
pub fn fetch_store_catalog_json(sources: &[String]) -> String {
    let host = PluginHost::new(default_plugin_dirs());
    let installed_ids: HashSet<String> = host.list_plugins().into_iter().map(|p| p.id).collect();
    host.shutdown_all();

    let user_plugins_dir = tomoyo_core::data_dir().join("plugins");
    let mut seen_ids = HashSet::new();
    let mut items = Vec::new();
    let mut loaded_from = Vec::new();
    let mut errors = Vec::new();

    let effective_sources: Vec<String> = if sources.is_empty() {
        vec!["registry/index.json".into()]
    } else {
        sources.to_vec()
    };

    for src in &effective_sources {
        let parsed = load_registry_source(src);
        match parsed {
            Ok((idx, resolved_origin)) => {
                loaded_from.push(resolved_origin);
                for p in idx.plugins {
                    if !seen_ids.insert(p.id.clone()) {
                        continue;
                    }
                    let slug = p.id.replace('.', "-");
                    let in_user_dir = user_plugins_dir.join(&slug).is_dir();
                    let is_installed = installed_ids.contains(&p.id) || in_user_dir;
                    items.push(serde_json::json!({
                        "id": p.id,
                        "name": p.name,
                        "version": p.version,
                        "description": p.description,
                        "author": p.author,
                        "license": p.license,
                        "runtime_kind": p.runtime_kind,
                        "keywords": p.keywords,
                        "source": p.source,
                        "sha256": p.sha256,
                        "installed": is_installed,
                        "removable": in_user_dir,
                    }));
                }
            }
            Err(e) => errors.push(format!("{src}: {e}")),
        }
    }

    serde_json::json!({
        "event": "store_loaded",
        "items": items,
        "sources": loaded_from,
        "errors": errors,
    })
    .to_string()
}

fn load_registry_source(source: &str) -> Result<(PluginRegistryIndex, String), String> {
    if source.starts_with("https://") || source.starts_with("http://") {
        let resp = tomoyo_core::http::http_get(source, 6000, 2 * 1024 * 1024)?;
        if resp.status_code != 200 {
            return Err(format!("HTTP {}", resp.status_code));
        }
        let idx: PluginRegistryIndex =
            serde_json::from_slice(&resp.body).map_err(|e| e.to_string())?;
        return Ok((idx, source.to_string()));
    }

    for cand in candidate_local_paths(source) {
        if cand.is_file() {
            let raw = std::fs::read_to_string(&cand).map_err(|e| e.to_string())?;
            let idx: PluginRegistryIndex = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            return Ok((idx, cand.to_string_lossy().into_owned()));
        }
    }
    Err("本地索引文件未找到".into())
}

fn candidate_local_paths(rel: &str) -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from(rel)];
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.parent();
        for _ in 0..4 {
            if let Some(dir) = cur {
                out.push(dir.join(rel));
                cur = dir.parent();
            } else {
                break;
            }
        }
    }
    out
}

fn install_plugin_from_source(
    plugin_id: &str,
    source: &str,
    expected_sha256: &str,
    notify_hwnd: Option<isize>,
) -> Result<String, String> {
    if plugin_id.is_empty() || plugin_id.contains("..") || plugin_id.contains(['/', '\\']) {
        return Err("无效的插件 ID".into());
    }
    let src_dir = candidate_local_paths(source)
        .into_iter()
        .find(|p| p.is_dir())
        .ok_or_else(|| format!("未在本地找到插件源码目录 `{source}`（可使用 tomoyo-cli install 安装远程包）"))?;

    let actual_sha = compute_plugin_sha256(&src_dir).map_err(|e| format!("SHA-256 计算失败: {e}"))?;
    if !expected_sha256.is_empty() && !expected_sha256.eq_ignore_ascii_case(&actual_sha) {
        return Err(format!(
            "安全阻断：SHA-256 不匹配！期望 {}，实际 {}",
            &expected_sha256[..expected_sha256.len().min(12)],
            &actual_sha[..12]
        ));
    }

    let slug = plugin_id.replace('.', "-");
    let target_dir = tomoyo_core::data_dir().join("plugins").join(slug);
    copy_plugin_files(&src_dir, &target_dir).map_err(|e| format!("复制文件失败: {e}"))?;

    if let Some(target) = notify_hwnd {
        notify_parent_reload(target);
    }
    Ok(target_dir.to_string_lossy().into_owned())
}

fn uninstall_user_plugin(plugin_id: &str, notify_hwnd: Option<isize>) -> Result<(), String> {
    if plugin_id.is_empty() || plugin_id.contains("..") || plugin_id.contains(['/', '\\']) {
        return Err("无效的插件 ID".into());
    }
    let slug = plugin_id.replace('.', "-");
    let target_dir = tomoyo_core::data_dir().join("plugins").join(slug);
    if !target_dir.is_dir() {
        return Err("该插件属于随包内置示例插件，无法从用户目录卸载".into());
    }
    std::fs::remove_dir_all(&target_dir).map_err(|e| format!("删除目录失败: {e}"))?;
    if let Some(target) = notify_hwnd {
        notify_parent_reload(target);
    }
    Ok(())
}

fn compute_plugin_sha256(dir: &Path) -> Result<String, std::io::Error> {
    let mut files = BTreeMap::<String, PathBuf>::new();
    collect_plugin_files(dir, dir, &mut files)?;
    let mut hasher = Sha256::new();
    for (rel, path) in files {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        let data = std::fs::read(&path)?;
        hasher.update((data.len() as u64).to_le_bytes());
        hasher.update(&data);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn collect_plugin_files(
    root: &Path,
    cur: &Path,
    out: &mut BTreeMap<String, PathBuf>,
) -> Result<(), std::io::Error> {
    for ent in std::fs::read_dir(cur)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name();
        let name_str = name.to_string_lossy();
        if matches!(
            name_str.as_ref(),
            "target" | "node_modules" | "__pycache__" | ".git" | "Cargo.lock"
        ) {
            continue;
        }
        if path.is_dir() {
            collect_plugin_files(root, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(rel, path);
        }
    }
    Ok(())
}

fn copy_plugin_files(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    let mut files = BTreeMap::new();
    collect_plugin_files(src, src, &mut files)?;
    std::fs::create_dir_all(dst)?;
    for (rel, path) in files {
        let target = dst.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&path, &target)?;
    }
    Ok(())
}

#[cfg(windows)]
fn notify_parent_reload(raw_hwnd: isize) {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    if raw_hwnd != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw_hwnd as *mut c_void)),
                WM_APP_CONFIG_SAVED,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

#[cfg(not(windows))]
fn notify_parent_reload(_raw_hwnd: isize) {}

#[cfg(windows)]
fn shell_open(target: &str) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(not(windows))]
fn shell_open(_target: &str) {}

#[cfg(windows)]
fn set_clipboard_text(text: &str) {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * 2;
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, byte_len) {
            let ptr = GlobalLock(hmem) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(hmem);
                let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

#[cfg(not(windows))]
fn set_clipboard_text(_text: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_tomoyo_and_utools_bridge() {
        let script = build_init_script("dev.test.json", "{\"a\":1}", true, true, "{}");
        assert!(script.contains("window.tomoyo ="));
        assert!(script.contains("window.utools ="));
        assert!(script.contains("onPluginEnter"));
        assert!(script.contains("setSubInput"));
        assert!(script.contains("hideMainWindowPasteText"));
        assert!(script.contains("dbStorage"));
    }
}
