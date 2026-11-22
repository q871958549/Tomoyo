//! Generates the injected `window.tomoyo` and optional `window.utools` compatibility JS bridge.

/// Builds the initialization JavaScript injected before document scripts run.
/// Provides synchronous `utools.db` / `utools.dbStorage`, `utools.setSubInput`,
/// `utools.hideMainWindowPasteText`, and `Ctrl+Shift+T` window pin toggle.
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
    let ver_js = serde_json::to_string(tomoyo_core::CURRENT_VERSION).unwrap_or_else(|_| "\"0.5.0\"".into());

    let platform_js = if cfg!(target_os = "linux") {
    "'linux'"
  } else if cfg!(target_os = "macos") {
    "'darwin'"
  } else {
    "'win32'"
  };
  let is_win = cfg!(windows);
  let is_mac = cfg!(target_os = "macos");
  let is_lin = cfg!(target_os = "linux");

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

  window.addEventListener('error', (e) => {{
    post({{ cmd: 'report_js_error', message: String(e.message || 'script error'), source: PLUGIN_ID, line: e.lineno || 0 }});
  }});
  window.addEventListener('unhandledrejection', (e) => {{
    post({{ cmd: 'report_js_error', message: String(e.reason || 'unhandled rejection'), source: PLUGIN_ID, line: 0 }});
  }});

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
      if (typeof key === 'string') filtered = keys.filter(k => k.startsWith(key));
      else if (Array.isArray(key)) filtered = keys.filter(k => key.includes(k));
      return filtered.map(k => JSON.parse(JSON.stringify(_store[k])));
    }}
  }};

  const dbStorage = {{
    setItem(key, value) {{ db.put({{ _id: '__kv:' + key, value }}); }},
    getItem(key) {{ const doc = db.get('__kv:' + key); return doc ? doc.value : null; }},
    removeItem(key) {{ db.remove('__kv:' + key); }}
  }};

  window.tomoyo = {{
    version: {ver_js},
    platform: {platform_js},
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
      isWindows: () => {is_win},
      isMacOs: () => {is_mac},
      isLinux: () => {is_lin},
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
