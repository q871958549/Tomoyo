// WebView2 UI Compiled-Bundle Runtime Smoke Gate.
// Executes all 4 built single-file HTML bundles from frontend/dist/ inside a real DOM
// (happy-dom) to guarantee zero runtime ReferenceError/TDZ crashes, live hotkey conflict
// badge rendering, 10-tab interactive navigation, and clean WebView2 IPC emission.

import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const DIST_DIR = path.resolve('frontend/dist');
const BUNDLES = ['settings.html', 'translator.html', 'clipboard.html', 'ocr.html'];

for (const file of BUNDLES) {
  const full = path.join(DIST_DIR, file);
  if (!fs.existsSync(full)) {
    throw new Error(`Missing built bundle: ${full}. Run \`cd frontend && npm run build\`.`);
  }
  const html = fs.readFileSync(full, 'utf8');
  if (!html.includes('<script>') || !html.includes('<style')) {
    throw new Error(`${file}: expected inlined classic <script> and <style> blocks`);
  }
  if (/<script[^>]+src=/i.test(html)) {
    throw new Error(`${file}: external <script src=...> forbidden in NavigateToString bundle`);
  }
}

const happyDomPkg = path.resolve('frontend/node_modules/happy-dom/lib/index.js');
if (!fs.existsSync(happyDomPkg)) {
  console.log('[OK] Verified 4 single-file WebView bundles statically (install frontend deps for full DOM execution).');
  process.exit(0);
}

const { Window } = await import(pathToFileURL(happyDomPkg).href);

async function runCompiledHtml(name, initKey, initData, verify) {
  const html = fs.readFileSync(path.join(DIST_DIR, name), 'utf8');
  const start = html.indexOf('<script>');
  const end = html.lastIndexOf('</script>');
  if (start < 0 || end <= start) throw new Error(`${name}: missing inline <script>`);
  const scriptBody = html.slice(start + '<script>'.length, end);

  const win = new Window({ url: 'https://tomoyo.local/' });
  const posted = [];
  win.chrome = {
    webview: {
      postMessage: (raw) => {
        const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
        posted.push(parsed);
        if (parsed.cmd === 'report_js_error') {
          throw new Error(`${name}: runtime JS error reported -> ${parsed.message}`);
        }
      },
      addEventListener: () => {},
      removeEventListener: () => {},
    },
  };
  win[initKey] = initData;
  win.document.write('<!doctype html><html><body><div id="app"></div></body></html>');
  win.document.close();

  const fn = new win.Function(scriptBody);
  fn.call(win);
  await new Promise((r) => setTimeout(r, 10));

  await verify(win, posted);
  win.happyDOM?.close?.();
  return posted;
}

const settingsPosted = await runCompiledHtml(
  'settings.html',
  '__TOMOYO_SETTINGS_INIT__',
  {
    version: '1.6.0',
    autostart: true,
    config: {
      language: 'zh',
      hotkey: 'Alt+Space',
      theme: 'light',
      hibernate_delay_secs: 15,
      auto_preview: true,
      show_filter_bar: true,
      window: { width_dip: 720, max_visible_rows: 8, preview_width_dip: 310, custom_x: null, custom_y: null },
      apps: { extra_dirs: [], auto_detect_desktop_organizers: true, extra_scan_depth: 2 },
      everything: { max_global_results: 6, max_dedicated_results: 30, extra_filter: '!*.tmp' },
      clipboard: { persist_sqlite: true, max_persisted_items: 500, hotkey: 'Alt+C', capture_enabled: true, capture_images: true, excluded_apps: ['keepass.exe'] },
      translate: { hotkey: 'Alt+T', target_lang: 'auto', engines: ['google', 'mymemory'], openai_base_url: 'https://api.openai.com/v1', openai_model: 'gpt-4o-mini', ollama_url: 'http://127.0.0.1:11434', ollama_model: 'qwen2.5:7b' },
      capture: { hotkey: 'Alt+1', save_dir: '', pin_opacity: 100, copy_hex_lowercase: false },
      update: { auto_check: true, check_interval_hours: 24, repo: 'q871958549/Tomoyo', last_checked_unix: 0 },
      web_searches: [{ keyword: 'g', name: 'Google', url: 'https://www.google.com/search?q={q}' }],
    },
    plugins: [{ id: 'dev.tomoyo.ocr', name: 'OCR', runtime: 'STDIO', keywords: ['ocr'], max_memory_mb: 128, declared_permissions: [], granted_permissions: [] }],
    running_apps: ['code.exe', 'notepad.exe'],
    secrets_status: { deepl: false, openai: true },
    hotkey_health: {
      main: { spec: 'Alt+Space', active: 'Alt+Space', ok: true, disabled: false, reason: '' },
      clipboard: { spec: 'Alt+C', active: '', ok: false, disabled: false, reason: '已被系统或其他软件占用（Alt+C）' },
      translate: { spec: 'Alt+T', active: '', ok: false, disabled: false, reason: '已被系统或其他软件占用（Alt+T）' },
      capture: { spec: 'Alt+1', active: 'Alt+1', ok: true, disabled: false, reason: '' },
    },
    config_path: 'C:\\Tomoyo\\config.toml',
    plugins_dir: 'C:\\Tomoyo\\plugins',
    logs_dir: 'C:\\Tomoyo\\logs',
  },
  async (win, posted) => {
    const tabs = [...win.document.querySelectorAll('nav button[data-tab]')];
    if (tabs.length !== 12) throw new Error(`settings.html: expected 12 nav tabs, got ${tabs.length}`);
    if (win.document.querySelectorAll('select').length !== 0) {
      throw new Error('settings.html: raw native <select> detected (must use custom Select primitive)');
    }
    for (const btn of tabs) {
      btn.click();
      await new Promise((r) => setTimeout(r, 5));
      if (!btn.classList.contains('is-active')) {
        throw new Error(`settings.html: tab ${btn.dataset.tab} failed to activate`);
      }
      if (btn.dataset.tab === 'search' && !win.document.body.textContent.includes('Alt+C')) {
        throw new Error('settings.html: clipboard hotkey conflict badge not rendered');
      }
    }
    if (!posted.some((m) => m.cmd === 'check_hotkey_health')) {
      throw new Error('settings.html: missing initial check_hotkey_health IPC');
    }
    if (!posted.some((m) => m.cmd === 'audit_hotkeys')) {
      throw new Error('settings.html: Hotkey Doctor tab failed to trigger audit_hotkeys IPC');
    }
  }
);

await runCompiledHtml(
  'translator.html',
  '__TOMOYO_TR_INIT__',
  { theme: 'dark', text: '', target_lang: 'auto', engines: ['google'], history: [], wordbook: [] },
  (win) => {
    if (!win.document.querySelector('textarea')) throw new Error('translator.html: missing source textarea');
  }
);

await runCompiledHtml(
  'clipboard.html',
  '__TOMOYO_CLIP_INIT__',
  { theme: 'light', capture_enabled: true, total: 0, items: [], tags: [], focus_key: '' },
  (win) => {
    if (!win.document.body.textContent.includes('剪贴板工作台')) {
      throw new Error('clipboard.html: failed to mount header');
    }
  }
);

await runCompiledHtml(
  'ocr.html',
  '__TOMOYO_OCR_DATA__',
  { theme: 'midnight', text: 'Hello OCR', raw_text: 'Hello OCR', lang_tag: 'en-US', src_dims: [100, 40], proc_dims: [200, 80], elapsed_ms: 18 },
  (win) => {
    if (!win.document.body.textContent.includes('OCR 审校与编辑')) {
      throw new Error('ocr.html: failed to mount header');
    }
  }
);

console.log(`[OK] All 4 compiled Svelte 5 WebView bundles booted in happy-dom (12 Settings tabs + Flows + Hotkey Doctor verified, live hotkey conflict rendered, ${settingsPosted.length} IPC commands, 0 JS errors).`);
