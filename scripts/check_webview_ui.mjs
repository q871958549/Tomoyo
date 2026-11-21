// Zero-dependency WebView2 UI runtime smoke gate.
// Assembles all 4 embedded HTML/JS bundles identically to tomoyo-webview-host and executes
// them inside an isolated Node vm context with a lightweight DOM to catch TDZ ReferenceErrors,
// missing DOM hooks, broken event handlers, and click regressions before they ever ship.

import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';

const UI_DIR = path.resolve('crates/tomoyo-webview-host/src/ui');
const readUi = (name) => fs.readFileSync(path.join(UI_DIR, name), 'utf8');

class MockClassList {
  constructor(initial = '') { this.set = new Set(initial.split(/\s+/).filter(Boolean)); }
  add(...names) { names.forEach((n) => n && this.set.add(n)); }
  remove(...names) { names.forEach((n) => this.set.delete(n)); }
  toggle(n, force) {
    if (force === undefined) {
      if (this.set.has(n)) { this.set.delete(n); return false; }
      this.set.add(n); return true;
    }
    if (force) this.set.add(n); else this.set.delete(n);
    return !!force;
  }
  contains(n) { return this.set.has(n); }
}

class MockEl {
  constructor(tag, attrs = {}, doc = null) {
    this.tagName = tag.toUpperCase();
    this.id = attrs.id || '';
    this.attrs = { ...attrs };
    this.dataset = {};
    for (const [k, v] of Object.entries(attrs)) {
      if (k.startsWith('data-')) {
        const camel = k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
        this.dataset[camel] = v;
      }
    }
    this.classList = new MockClassList(attrs.class || '');
    this.style = {};
    this.value = attrs.value || '';
    this.checked = 'checked' in attrs;
    this.disabled = false;
    this.placeholder = attrs.placeholder || '';
    this.textContent = '';
    this._innerHTML = '';
    this.children = [];
    this.listeners = {};
    this.scrollTop = 0;
    this.scrollHeight = 100;
    this._doc = doc;
  }
  get className() { return [...this.classList.set].join(' '); }
  set className(v) { this.classList = new MockClassList(String(v || '')); }
  get innerHTML() { return this._innerHTML; }
  set innerHTML(v) {
    this._innerHTML = String(v || '');
    this.children = [];
    if (this._doc && this._innerHTML.includes('<')) {
      this._doc._indexHtmlSnippet(this._innerHTML, this);
    }
  }
  getAttribute(k) { return k in this.attrs ? this.attrs[k] : null; }
  setAttribute(k, v) { this.attrs[k] = String(v); if (k === 'id') this.id = String(v); }
  appendChild(child) { this.children.push(child); return child; }
  addEventListener(ev, fn) { (this.listeners[ev] ||= []).push(fn); }
  dispatchClick() {
    if (typeof this.onclick === 'function') this.onclick({ preventDefault() {}, stopPropagation() {} });
    (this.listeners.click || []).forEach((fn) => fn({ target: this, preventDefault() {}, stopPropagation() {} }));
  }
  focus() {}
  select() {}
  closest() { return null; }
  querySelectorAll(sel) { return this._doc ? this._doc._matchAll(sel, this) : []; }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
}

function parseAttrs(raw) {
  const out = {};
  const re = /([a-zA-Z0-9_-]+)(?:\s*=\s*"([^"]*)"|\s*=\s*'([^']*)')?/g;
  let m;
  while ((m = re.exec(raw))) out[m[1]] = m[2] ?? m[3] ?? '';
  return out;
}

class MockDocument {
  constructor(html) {
    this.all = [];
    this.byId = new Map();
    this.body = new MockEl('body', {}, this);
    this.documentElement = new MockEl('html', {}, this);
    this.listeners = {};
    this._indexHtmlSnippet(html, null);
  }
  _indexHtmlSnippet(html, parent) {
    const clean = html.replace(/<script[\s\S]*?<\/script>/gi, '').replace(/<style[\s\S]*?<\/style>/gi, '');
    const re = /<([a-zA-Z][a-zA-Z0-9-]*)([^>]*)>/g;
    let m;
    while ((m = re.exec(clean))) {
      const tag = m[1].toLowerCase();
      if (['html', 'head', 'meta', 'title', 'link', 'br', 'hr'].includes(tag)) continue;
      const el = new MockEl(tag, parseAttrs(m[2]), this);
      el._parent = parent;
      this.all.push(el);
      if (el.id) this.byId.set(el.id, el);
    }
  }
  getElementById(id) { return this.byId.get(id) || null; }
  createElement(tag) { return new MockEl(tag, {}, this); }
  addEventListener(ev, fn) { (this.listeners[ev] ||= []).push(fn); }
  _matchAll(sel, scope = null) {
    const pool = scope ? scope.children.concat(this.all.filter((e) => e._parent === scope)) : this.all;
    const s = sel.trim();
    if (s.startsWith('#')) {
      const [idPart, rest] = s.slice(1).split(/\s+/, 2);
      const root = this.byId.get(idPart);
      if (!root) return [];
      if (!rest) return [root];
      return this.all.filter((e) => {
        if (rest.includes('[')) {
          const [t, attr] = rest.split('[');
          const key = attr.replace(']', '');
          return (!t || e.tagName === t.toUpperCase()) && key in e.attrs;
        }
        return e.tagName === rest.toUpperCase();
      });
    }
    if (s === 'nav button') return this.all.filter((e) => e.tagName === 'BUTTON' && 'data-tab' in e.attrs);
    if (s === 'section') return this.all.filter((e) => e.tagName === 'SECTION');
    if (s === '[data-i18n]') return this.all.filter((e) => 'data-i18n' in e.attrs);
    if (s.startsWith('.')) {
      const cls = s.slice(1);
      return pool.filter((e) => e.classList.contains(cls));
    }
    return pool.filter((e) => e.tagName === s.toUpperCase());
  }
  querySelectorAll(sel) { return this._matchAll(sel, null); }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
}

function runBundle(name, html, initGlobals) {
  const scriptMatches = [...html.matchAll(/<script[^>]*>([\s\S]*?)<\/script>/gi)];
  if (!scriptMatches.length) throw new Error(`${name}: no <script> block found`);
  const doc = new MockDocument(html);
  const posted = [];
  const winListeners = {};
  const ctx = {
    console,
    setTimeout: (fn) => { return 1; },
    clearTimeout: () => {},
    Date, Math, JSON, Object, Array, Set, Map, Number, String, Boolean, RegExp, Error, URLSearchParams, encodeURIComponent, decodeURIComponent,
    navigator: { clipboard: { writeText: async () => {} } },
    speechSynthesis: { cancel() {}, speak() {} },
    SpeechSynthesisUtterance: function (t) { this.text = t; },
    document: doc,
    addEventListener: (ev, fn) => { (winListeners[ev] ||= []).push(fn); },
    chrome: {
      webview: {
        postMessage: (msg) => {
          const parsed = typeof msg === 'string' ? JSON.parse(msg) : msg;
          posted.push(parsed);
          if (parsed.cmd === 'report_js_error') {
            throw new Error(`${name}: JS reported error -> ${parsed.message} (line ${parsed.line})`);
          }
        },
        addEventListener: (ev, fn) => { (winListeners['wv_' + ev] ||= []).push(fn); },
      },
    },
    ...initGlobals,
  };
  ctx.window = ctx;
  const context = vm.createContext(ctx);
  for (const m of scriptMatches) {
    vm.runInContext(m[1], context, { filename: name });
  }
  return { ctx, doc, posted, winListeners };
}

// 1. Settings Center
const settingsHtml = readUi('settings.html')
  .replace('/*__SETTINGS_CSS__*/', readUi('settings.css'))
  .replace('/*__SETTINGS_I18N__*/', readUi('i18n.js'))
  .replace('/*__SETTINGS_BOOT_JS__*/', readUi('settings_boot.js'))
  .replace('/*__SETTINGS_STORE_JS__*/', readUi('store_updater.js'))
  .replace('/*__SETTINGS_CONTROLS_JS__*/', readUi('controls.js'))
  .replace('/*__SETTINGS_CLIP_JS__*/', readUi('clip_controls.js'))
  .replace('/*__SETTINGS_TRANSLATE_JS__*/', readUi('translate_controls.js'))
  .replace('/*__SETTINGS_CAPTURE_JS__*/', readUi('capture_controls.js'))
  .replace('/*__SETTINGS_JS__*/', readUi('settings.js'));

const sRes = runBundle('settings.html', settingsHtml, {
  __TOMOYO_SETTINGS_INIT__: {
    version: '1.5.0',
    autostart: true,
    config: {
      language: 'zh', hotkey: 'Alt+Space', theme: 'light', hibernate_delay_secs: 15,
      auto_preview: true, show_filter_bar: true,
      window: { width_dip: 720, max_visible_rows: 8, preview_width_dip: 310 },
      apps: { extra_dirs: [], auto_detect_desktop_organizers: true, extra_scan_depth: 2 },
      everything: { max_global_results: 6, max_dedicated_results: 30, extra_filter: '!*.tmp' },
      clipboard: { persist_sqlite: true, max_persisted_items: 500, hotkey: 'Alt+V', capture_enabled: true, capture_images: true, excluded_apps: ['keepass.exe'] },
      translate: { hotkey: 'Alt+T', target_lang: 'auto', engines: ['google', 'mymemory'] },
      capture: { hotkey: 'Alt+S', save_dir: '', pin_opacity: 100, copy_hex_lowercase: false },
      web_searches: [{ keyword: 'g', name: 'Google', url: 'https://google.com/search?q={q}' }],
      update: { auto_check: true, check_interval_hours: 24, repo: 'q871958549/Tomoyo', last_checked_unix: 0 },
    },
    plugins: [{ id: 'dev.tomoyo.ocr', name: 'OCR', runtime: 'STDIO', keywords: ['ocr'], max_memory_mb: 128, declared_permissions: [], granted_permissions: [] }],
    running_apps: ['code.exe', 'notepad.exe'],
    secrets_status: { deepl: false, openai: true },
    cached_update: { state: 'up_to_date', current: '1.5.0', latest: '1.5.0', checked_at_unix: Math.floor(Date.now() / 1000), note: '' },
    config_path: 'C:\\Tomoyo\\config.toml',
    plugins_dir: 'C:\\Tomoyo\\plugins',
    logs_dir: 'C:\\Tomoyo\\logs',
  },
});

if (!sRes.doc.getElementById('hotkey-recorder').innerHTML.includes('Alt')) {
  throw new Error('settings.html: hotkey-recorder failed to render initial hotkey');
}
if (!sRes.doc.getElementById('cap-hotkey-recorder').innerHTML.includes('Alt')) {
  throw new Error('settings.html: cap-hotkey-recorder failed to render initial capture hotkey');
}
if (!sRes.doc.getElementById('lang-zh').classList.contains('active')) {
  throw new Error('settings.html: lang-zh did not become active on populate()');
}
if (!sRes.doc.getElementById('toast').textContent) {
  throw new Error('settings.html: #toast remained empty after populate()');
}
const navButtons = sRes.doc.querySelectorAll('nav button');
if (navButtons.length < 8) throw new Error(`settings.html: expected >= 8 nav tabs, got ${navButtons.length}`);
for (const btn of navButtons) {
  btn.dispatchClick();
  const sec = sRes.doc.getElementById('tab-' + btn.dataset.tab);
  if (!sec || !sec.classList.contains('active')) {
    throw new Error(`settings.html: clicking nav tab "${btn.dataset.tab}" did not activate #tab-${btn.dataset.tab}`);
  }
}
sRes.ctx.setLanguage('en');
sRes.ctx.pickTheme('dark', true);
sRes.ctx.checkUpdatesNow();
if (!sRes.posted.some((m) => m.cmd === 'save_settings')) throw new Error('settings.html: save_settings not posted');
if (!sRes.posted.some((m) => m.cmd === 'check_updates')) throw new Error('settings.html: check_updates not posted');

// 2. Clipboard Manager
const clipHtml = readUi('clipboard_manager.html').replace('/*__CLIP_JS__*/', readUi('clipboard_manager.js'));
runBundle('clipboard_manager.html', clipHtml, {
  __TOMOYO_CLIP_INIT__: { theme: 'light', capture_enabled: true, items: [], tags: [], focus_key: '' },
});

// 3. Translator Workbench
const trHtml = readUi('translator.html').replace('/*__TRANSLATOR_JS__*/', readUi('translator.js'));
runBundle('translator.html', trHtml, {
  __TOMOYO_TR_INIT__: { theme: 'light', text: '', target_lang: 'auto', engines: ['google'], history: [], wordbook: [] },
});

// 4. OCR Viewer
const ocrHtml = readUi('ocr_viewer.html');
runBundle('ocr_viewer.html', ocrHtml, {
  __TOMOYO_OCR_DATA__: { theme: 'light', text: 'Hello', raw_text: 'Hello', image_url: '', lang: 'zh-Hans-CN' },
});

console.log(`[OK] All 4 WebView UIs booted & passed interactive smoke checks (${navButtons.length} Settings tabs verified, ${sRes.posted.length} IPC commands).`);
