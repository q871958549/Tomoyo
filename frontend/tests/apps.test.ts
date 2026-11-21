import { describe, expect, it, beforeEach } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import SettingsApp from '../src/apps/settings/SettingsApp.svelte';
import TranslatorApp from '../src/apps/translator/TranslatorApp.svelte';
import ClipboardApp from '../src/apps/clipboard/ClipboardApp.svelte';
import OcrApp from '../src/apps/ocr/OcrApp.svelte';
import { buildComboFromEvent } from '../src/lib/hotkeys';

function setupMockWebView() {
  const posted: Array<Record<string, unknown>> = [];
  const listeners: Array<(ev: { data: unknown }) => void> = [];
  window.chrome = {
    webview: {
      postMessage: (raw: string) => {
        const parsed = JSON.parse(raw) as Record<string, unknown>;
        posted.push(parsed);
        if (parsed.cmd === 'report_js_error') {
          throw new Error(`Reported JS error: ${String(parsed.message)}`);
        }
      },
      addEventListener: (_type, fn) => { listeners.push(fn); },
      removeEventListener: (_type, fn) => {
        const idx = listeners.indexOf(fn);
        if (idx >= 0) listeners.splice(idx, 1);
      },
    },
  };
  return {
    posted,
    emit: (data: Record<string, unknown>) => listeners.forEach((fn) => fn({ data })),
  };
}

describe('Tomoyo Svelte 5 WebView UIs', () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="app"></div>';
    document.body.className = '';
  });

  it('boots SettingsApp, renders hotkey conflict badge, and switches all 10 tabs', () => {
    const wv = setupMockWebView();
    window.__TOMOYO_SETTINGS_INIT__ = {
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
        capture: {
          hotkey: 'Alt+1',
          save_dir: '',
          pin_opacity: 100,
          copy_hex_lowercase: false,
          tool_hotkeys: { rect: 'R', ellipse: 'E', arrow: 'A', pen: 'B', highlighter: 'H', mosaic: 'M', step: 'N', text: 'X', scroll: 'L', pin: 'P', ocr: 'O', translate: 'T' },
        },
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
    };

    const app = mount(SettingsApp, { target: document.getElementById('app')! });
    flushSync();

    const navBtns = document.querySelectorAll<HTMLButtonElement>('nav button[data-tab]');
    expect(navBtns.length).toBe(11);

    // Click every tab and assert zero errors + tab activates
    for (const btn of navBtns) {
      btn.click();
      flushSync();
      expect(btn.classList.contains('is-active')).toBe(true);
      if (btn.dataset.tab === 'search') {
        expect(document.body.textContent).toContain('已被系统或其他软件占用（Alt+C）');
      }
      if (btn.dataset.tab === 'translate') {
        expect(document.querySelectorAll('input[type="password"]').length).toBe(2);
      }
      if (btn.dataset.tab === 'web') {
        const vals = [...document.querySelectorAll<HTMLInputElement>('input')].map((el) => el.value);
        expect(vals).toContain('Google');
      }
    }

    expect(wv.posted.some((m) => m.cmd === 'check_hotkey_health')).toBe(true);
    expect(wv.posted.some((m) => m.cmd === 'audit_hotkeys')).toBe(true);
    expect(buildComboFromEvent(new KeyboardEvent('keydown', { code: 'F1' })).combo).toBe('F1');
    expect(buildComboFromEvent(new KeyboardEvent('keydown', { code: 'PrintScreen' })).combo).toBe('PrintScreen');
    expect(buildComboFromEvent(new KeyboardEvent('keydown', { code: 'KeyA' })).needMod).toBe(true);
    unmount(app);
  });

  it('boots TranslatorApp, ClipboardApp, and OcrApp cleanly', () => {
    setupMockWebView();

    window.__TOMOYO_TR_INIT__ = { theme: 'dark', text: '', target_lang: 'auto', engines: ['google'], history: [], wordbook: [] };
    const tr = mount(TranslatorApp, { target: document.getElementById('app')! });
    flushSync();
    expect(document.querySelector('textarea')).not.toBeNull();
    unmount(tr);

    document.body.innerHTML = '<div id="app"></div>';
    window.__TOMOYO_CLIP_INIT__ = {
      theme: 'light', capture_enabled: true, total: 1, tags: [{ tag: 'work', count: 1 }],
      items: [{ key: 'k1', kind: 'text', subtype: 'text', preview: 'hello', text: 'hello', ts: 1700000000, pinned: false, tag: 'work', note: '', source_app: 'code.exe' }],
    };
    const cb = mount(ClipboardApp, { target: document.getElementById('app')! });
    flushSync();
    expect(document.body.textContent).toContain('剪贴板工作台');
    unmount(cb);

    document.body.innerHTML = '<div id="app"></div>';
    window.__TOMOYO_OCR_DATA__ = { theme: 'midnight', text: 'Line 1\nLine 2', raw_text: 'Line 1\nLine 2', lang_tag: 'zh-Hans-CN', src_dims: [100, 50], proc_dims: [200, 100], elapsed_ms: 42 };
    const ocr = mount(OcrApp, { target: document.getElementById('app')! });
    flushSync();
    expect(document.body.textContent).toContain('OCR 审校与编辑');
    unmount(ocr);
  });
});
