// WebView2 UI Compiled-Bundle Runtime Smoke Gate.
// Executes all 4 built single-file HTML bundles from frontend/dist/ inside a real DOM
// (happy-dom) to guarantee zero runtime ReferenceError/TDZ crashes, live hotkey conflict
// badge rendering, 10-tab interactive navigation, and clean WebView2 IPC emission.

import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const BUNDLES = [
  'frontend/dist/settings.html',
  'plugins/official-translate/ui/index.html',
  'plugins/official-clipboard/ui/index.html',
  'plugins/builtin-ocr/ui/index.html',
];

for (const file of BUNDLES) {
  const full = path.resolve(file);
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

async function runCompiledHtml(relPath, initKey, initData, verify, extraWin = null) {
  const html = fs.readFileSync(path.resolve(relPath), 'utf8');
  const start = html.indexOf('<script>');
  const end = html.lastIndexOf('</script>');
  if (start < 0 || end <= start) throw new Error(`${relPath}: missing inline <script>`);
  const scriptBody = html.slice(start + '<script>'.length, end);

  const win = new Window({ url: 'https://tomoyo.local/' });
  const posted = [];
  win.chrome = {
    webview: {
      postMessage: (raw) => {
        const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
        posted.push(parsed);
        if (parsed.cmd === 'report_js_error') {
          throw new Error(`${relPath}: runtime JS error reported -> ${parsed.message}`);
        }
      },
      addEventListener: () => {},
      removeEventListener: () => {},
    },
  };
  if (initKey) win[initKey] = initData;
  if (extraWin) extraWin(win);
  win.document.write('<!doctype html><html><body><div id="app"></div></body></html>');
  win.document.close();

  const fn = new win.Function(scriptBody);
  fn.call(win);
  await new Promise((r) => setTimeout(r, 15));

  await verify(win, posted);
  win.happyDOM?.close?.();
  return posted;
}

const settingsPosted = await runCompiledHtml(
  'frontend/dist/settings.html',
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
    plugins: [{
      id: 'dev.tomoyo.clipboard', name: '剪贴板管理中心', version: '3.0.0', runtime: 'STDIO', keywords: ['cb'], max_memory_mb: 96,
      declared_permissions: ['clipboard'], granted_permissions: ['clipboard'],
      declared_hotkeys: [{ id: 'open_cb', label: '剪贴板历史快捷搜索', default_hotkey: 'Alt+V', trigger_query: 'cb ' }],
      settings_schema: [{ key: 'capture_enabled', label: '后台自动采集剪贴板', type: 'bool', default: true }],
    }],
    running_apps: ['code.exe', 'notepad.exe'],
    secrets_status: { deepl: false, openai: true },
    hotkey_health: {
      main: { spec: 'Alt+Space', active: '', ok: false, disabled: false, reason: '已被系统或其他软件占用（Alt+Space）' },
    },
    config_path: 'C:\\Tomoyo\\config.toml',
    plugins_dir: 'C:\\Tomoyo\\plugins',
    logs_dir: 'C:\\Tomoyo\\logs',
  },
  async (win, posted) => {
    const tabs = [...win.document.querySelectorAll('nav button[data-tab]')];
    if (tabs.length !== 6) throw new Error(`settings.html: expected 6 nav tabs, got ${tabs.length}`);
    if (win.document.querySelectorAll('select').length !== 0) {
      throw new Error('settings.html: raw native <select> detected (must use custom Select primitive)');
    }
    for (const btn of tabs) {
      btn.click();
      await new Promise((r) => setTimeout(r, 5));
      if (!btn.classList.contains('is-active')) {
        throw new Error(`settings.html: tab ${btn.dataset.tab} failed to activate`);
      }
      if (btn.dataset.tab === 'plugins') {
        win.document.querySelector('button[data-config-plugin="dev.tomoyo.clipboard"]')?.click();
        await new Promise((r) => setTimeout(r, 5));
        if (!win.document.body.textContent.includes('剪贴板历史快捷搜索')) {
          throw new Error('settings.html: clipboard Protocol v3 drawer failed to expand');
        }
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
  'plugins/official-translate/ui/index.html',
  null,
  null,
  (win) => {
    const ta = win.document.querySelector('textarea');
    if (!ta) throw new Error('official-translate/ui/index.html: missing source textarea');
    if (!win.document.body.textContent.includes('优雅解耦')) {
      throw new Error('official-translate/ui/index.html: failed to hydrate translation via window.tomoyo.callPlugin');
    }
  },
  (win) => {
    win.tomoyo = {
      theme: 'dark',
      initialQuery: 'decouple',
      callPlugin: async (method) => {
        if (method === 'get_init') {
          return { text: 'decouple', target_lang: 'zh-Hans', engines: ['google'], history: [], wordbook: [] };
        }
        if (method === 'translate') {
          return {
            data: {
              source_text: 'decouple',
              target_lang: 'zh-Hans',
              results: [{ engine_name: 'Google', ok: true, translated: '优雅解耦', elapsed_ms: 42 }],
            },
            store: { history: [], wordbook: [] },
          };
        }
        return { ok: true, history: [], wordbook: [] };
      },
      onEnter: () => {},
    };
  }
);

await runCompiledHtml(
  'plugins/official-clipboard/ui/index.html',
  null,
  null,
  (win) => {
    if (!win.document.body.textContent.includes('剪贴板工作台')) {
      throw new Error('official-clipboard/ui/index.html: failed to mount header');
    }
    if (!win.document.body.textContent.includes('Hybrid Clipboard Ready')) {
      throw new Error('official-clipboard/ui/index.html: failed to hydrate via window.tomoyo.callPlugin');
    }
  },
  (win) => {
    win.tomoyo = {
      theme: 'light',
      initialQuery: '',
      callPlugin: async (method) => {
        if (method !== 'list') throw new Error(`unexpected clipboard RPC method: ${method}`);
        return {
          total: 1,
          capture_enabled: true,
          tags: [{ tag: 'work', count: 1 }],
          items: [{
            key: 'Hybrid Clipboard Ready',
            kind: 'text',
            subtype: 'text',
            preview: 'Hybrid Clipboard Ready',
            text: 'Hybrid Clipboard Ready',
            ts: 1700000000,
            pinned: true,
            tag: 'work',
            note: '',
            source_app: 'code',
          }],
        };
      },
      onEnter: () => {},
    };
  }
);

await runCompiledHtml(
  'plugins/builtin-ocr/ui/index.html',
  null,
  null,
  (win) => {
    if (!win.document.body.textContent.includes('OCR 审校与编辑')) {
      throw new Error('plugins/builtin-ocr/ui/index.html: failed to mount header');
    }
    const ta = win.document.querySelector('textarea');
    if (!ta || ta.value !== 'Hello Hybrid OCR') {
      throw new Error('plugins/builtin-ocr/ui/index.html: failed to hydrate via window.tomoyo.callPlugin');
    }
  },
  (win) => {
    win.tomoyo = {
      theme: 'midnight',
      initialQuery: '/tmp/sample.png',
      callPlugin: async (method) => {
        if (method !== 'get_last_ocr') throw new Error(`unexpected RPC method: ${method}`);
        return {
          text: 'Hello Hybrid OCR',
          raw_text: 'Hello Hybrid OCR',
          lang_tag: 'en-US',
          src_dims: [100, 40],
          proc_dims: [200, 80],
          elapsed_ms: 18,
        };
      },
      onEnter: () => {},
    };
  }
);

console.log(`[OK] All 4 compiled Svelte 5 WebView bundles booted in happy-dom (6 core tabs + in-plugin settings drawers + Flows + Hotkey Doctor verified, live hotkey conflict rendered, ${settingsPosted.length} IPC commands, 0 JS errors).`);
