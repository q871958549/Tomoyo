import { describe, expect, it, beforeEach } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import SettingsApp from '../src/apps/settings/SettingsApp.svelte';
import TranslatorApp from '../src/apps/translator/TranslatorApp.svelte';
import ClipboardApp from '../src/apps/clipboard/ClipboardApp.svelte';
import OcrApp from '../src/apps/ocr/OcrApp.svelte';
import { assignCoreHotkeyWithSwap, buildComboFromEvent } from '../src/lib/hotkeys';

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

  it('boots slim 6-tab SettingsApp, expands in-plugin drawers, and switches sub-tabs cleanly', async () => {
    const wv = setupMockWebView();
    const origHealth = {
      main: { spec: 'Alt+Space', active: 'Alt+Space', ok: true, disabled: false, reason: '' },
      clipboard: { spec: 'Alt+C', active: '', ok: false, disabled: false, reason: '已被系统或其他软件占用（Alt+C）' },
      translate: { spec: 'Alt+T', active: '', ok: false, disabled: false, reason: '已被系统或其他软件占用（Alt+T）' },
      capture: { spec: 'Alt+1', active: 'Alt+1', ok: true, disabled: false, reason: '' },
      pin: { spec: 'F3', active: 'F3', ok: true, disabled: false, reason: '' },
    };
    window.__TOMOYO_SETTINGS_INIT__ = {
      version: '2.5.1',
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
          pin_hotkey: 'F3',
          ocr_hotkey: '',
          save_dir: '',
          pin_opacity: 100,
          copy_hex_lowercase: false,
          tool_hotkeys: { rect: 'R', ellipse: 'E', arrow: 'A', pen: 'B', highlighter: 'H', mosaic: 'M', step: 'N', text: 'X', scroll: 'L', pin: 'P', ocr: 'O', ocr_copy: 'Shift+C', translate: 'T', record: 'V' },
        },
        update: { auto_check: true, check_interval_hours: 24, repo: 'q871958549/Tomoyo', last_checked_unix: 0 },
        web_searches: [{ keyword: 'g', name: 'Google', url: 'https://www.google.com/search?q={q}' }],
      },
      plugins: [{
        id: 'dev.tomoyo.ocr', name: 'OCR', runtime: 'STDIO', keywords: ['ocr'], max_memory_mb: 128,
        declared_permissions: [], granted_permissions: [],
        declared_hotkeys: [{ id: 'quick_ocr', label: '快速识别截图', default_hotkey: '', trigger_query: 'ocr ' }],
        settings_schema: [
          { key: 'default_lang', label: '默认语言', type: 'select', default: 'auto', options: [{ value: 'auto', label: '自动' }, { value: 'en', label: '英文' }] },
          { key: 'raw_output', label: '保留原始空格', type: 'bool', default: false },
        ],
      }],
      flows: [{
        id: 'clip_clean_paste', name: '剪贴板去换行纯文本直贴', keyword: 'cleanpaste',
        description: '合并单行后直贴', enabled: true,
        steps: [{ kind: 'read_clipboard', value: '' }, { kind: 'single_line', value: '' }, { kind: 'paste_text', value: '' }],
      }],
      flows_path: 'C:\\Tomoyo\\flows.toml',
      running_apps: ['code.exe', 'notepad.exe'],
      secrets_status: { deepl: false, openai: true },
      hotkey_health: origHealth,
      config_path: 'C:\\Tomoyo\\config.toml',
      plugins_dir: 'C:\\Tomoyo\\plugins',
      logs_dir: 'C:\\Tomoyo\\logs',
    };

    const app = mount(SettingsApp, { target: document.getElementById('app')! });
    flushSync();
    await new Promise((r) => setTimeout(r, 40));
    expect(wv.posted.some((m) => m.cmd === 'window_ready')).toBe(true);

    const navBtns = document.querySelectorAll<HTMLButtonElement>('nav button[data-tab]');
    expect(navBtns.length).toBe(6);
    expect(document.querySelector<HTMLImageElement>('img.brand-logo')?.getAttribute('src')).toBeTruthy();

    // Hotkey conflict bar + Hotkey Doctor rescue flow
    const conflictBtn = document.querySelector<HTMLButtonElement>('button#toast.conflict-bar-btn');
    expect(conflictBtn).not.toBeNull();
    expect(conflictBtn?.textContent).toContain('Alt+C');
    expect(document.querySelector('button[data-tab="hotkeys"] .nav-badge')?.textContent).toBe('2');
    conflictBtn?.click();
    flushSync();
    expect(document.querySelector('button[data-tab="hotkeys"]')?.classList.contains('is-active')).toBe(true);
    expect(document.body.textContent).toContain('高优先级强行接管冲突热键');
    document.querySelector<HTMLButtonElement>('button.btn-rescue-clipboard')?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'save_settings' && (m.config as { override_conflict_hotkeys?: boolean })?.override_conflict_hotkeys === true)).toBe(true);

    // Restoring conflict badge clears when fixed
    wv.emit({
      event: 'hotkey_health',
      health: {
        main: { spec: 'Alt+Space', active: 'Alt+Space', ok: true, disabled: false, overridden: false, reason: '' },
        clipboard: { spec: 'Alt+C', active: 'Alt+C', ok: true, disabled: false, overridden: true, reason: '已高优先级接管生效' },
        translate: { spec: 'Alt+T', active: 'Alt+T', ok: true, disabled: false, overridden: false, reason: '' },
        capture: { spec: 'Alt+1', active: 'Alt+1', ok: true, disabled: false, overridden: false, reason: '' },
        pin: { spec: 'F3', active: 'F3', ok: true, disabled: false, overridden: false, reason: '' },
      },
    });
    flushSync();
    expect(document.querySelector('button#toast.conflict-bar-btn')).toBeNull();
    wv.emit({ event: 'hotkey_health', health: origHealth });
    flushSync();

    // Click all 6 top-level tabs and verify each activates cleanly
    for (const btn of navBtns) {
      btn.click();
      flushSync();
      expect(btn.classList.contains('is-active')).toBe(true);
    }

    // Plugin Center consolidates Clipboard / Translate / Capture / OCR / Notes inside module drawers!
    document.querySelector<HTMLButtonElement>('button[data-tab="plugins"]')?.click();
    flushSync();
    expect(document.body.textContent).toContain('官方功能插件');

    document.querySelector<HTMLButtonElement>('button[data-config-plugin="dev.tomoyo.clipboard"]')?.click();
    flushSync();
    expect(document.body.textContent).toContain('已被系统或其他软件占用（Alt+C）');

    document.querySelector<HTMLButtonElement>('button[data-config-plugin="dev.tomoyo.translate"]')?.click();
    flushSync();
    expect(document.querySelectorAll('input[type="password"]').length).toBe(2);

    document.querySelector<HTMLButtonElement>('button[data-config-plugin="dev.tomoyo.capture"]')?.click();
    flushSync();
    expect(document.body.textContent).toContain('全局一键贴图快捷键');
    expect(document.body.textContent).toContain('F3');

    const trSwitch = document.querySelector<HTMLButtonElement>('button[role="switch"][aria-label="多源划词翻译"]');
    trSwitch?.click();
    flushSync();
    expect(document.querySelector('button[data-tab="hotkeys"] .nav-badge')?.textContent).toBe('1');

    document.querySelector<HTMLButtonElement>('button[data-config-plugin="dev.tomoyo.ocr"]')?.click();
    flushSync();
    expect(document.querySelector('[data-plugin-drawer="dev.tomoyo.ocr"]')).not.toBeNull();
    document.querySelector<HTMLButtonElement>('button[role="switch"][aria-label="dev.tomoyo.ocr:raw_output"]')?.click();
    flushSync();

    // Search & Web sub-tabs
    document.querySelector<HTMLButtonElement>('button[data-tab="search"]')?.click();
    flushSync();
    document.querySelector<HTMLButtonElement>('.fc-preset-sel button.t-select-trigger')?.click();
    flushSync();
    document.querySelector<HTMLButtonElement>('button[data-opt="code"]')?.click();
    flushSync();
    expect(document.body.textContent).toContain('@code');

    // Appearance & Typography controls (zero native <select> elements per UI standards)
    document.querySelector<HTMLButtonElement>('button[data-tab="core"]')?.click();
    flushSync();
    const appSeg = [...document.querySelectorAll<HTMLButtonElement>('.subbar button')].find((b) => b.textContent?.includes('外观与窗口'));
    appSeg?.click();
    flushSync();
    expect(document.body.textContent).toContain('搜索结果字体与现代排版');
    expect(document.querySelectorAll('select').length).toBe(0);
    const boldBtn = [...document.querySelectorAll<HTMLButtonElement>('.t-seg button')].find((b) => b.textContent?.includes('粗体 700'));
    boldBtn?.click();
    flushSync();

    document.querySelector<HTMLButtonElement>('button[data-tab="search"]')?.click();
    flushSync();
    const webSeg = [...document.querySelectorAll<HTMLButtonElement>('.subbar button')].find((b) => b.textContent?.includes('快捷网页搜索'));
    webSeg?.click();
    flushSync();
    expect([...document.querySelectorAll<HTMLInputElement>('input')].map((el) => el.value)).toContain('Google');

    // System Updates & Logs sub-tab exposes update.log
    document.querySelector<HTMLButtonElement>('button[data-tab="system"]')?.click();
    flushSync();
    const logsSeg = [...document.querySelectorAll<HTMLButtonElement>('.subbar button')].find((b) => b.textContent?.includes('诊断与日志'));
    logsSeg?.click();
    flushSync();
    expect(document.body.textContent).toContain('更新日志');
    const clearBtn = [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('归档清理'));
    clearBtn?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'clear_diagnostic_logs')).toBe(true);

    // Flows tab
    document.querySelector<HTMLButtonElement>('button[data-tab="flows"]')?.click();
    flushSync();
    document.querySelector<HTMLButtonElement>('button[role="switch"][aria-label="剪贴板去换行纯文本直贴"]')?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'save_flows')).toBe(true);

    const lastSave = [...wv.posted].reverse().find((m) => m.cmd === 'save_settings');
    const savedCfg = (lastSave?.config ?? {}) as {
      custom_filters?: Array<{ id: string }>;
      disabled_plugins?: string[];
      plugin_settings?: Record<string, Record<string, unknown>>;
    };
    expect(savedCfg.custom_filters?.some((t) => t.id === 'code')).toBe(true);
    expect(savedCfg.disabled_plugins).toContain('dev.tomoyo.translate');
    expect(savedCfg.plugin_settings?.['dev.tomoyo.ocr']?.raw_output).toBe(true);

    unmount(app);
  });

  it('boots TranslatorApp, ClipboardApp, and OcrApp cleanly', () => {
    const wv = setupMockWebView();

    window.__TOMOYO_TR_INIT__ = { theme: 'dark', text: '', target_lang: 'auto', engines: ['google'], history: [], wordbook: [] };
    const tr = mount(TranslatorApp, { target: document.getElementById('app')! });
    flushSync();
    expect(document.querySelector('textarea')).not.toBeNull();
    document.querySelector<HTMLButtonElement>('button[title="翻译插件设置"]')?.click();
    expect(wv.posted.some((m) => m.cmd === 'open_settings' && m.target === 'plugins:dev.tomoyo.translate')).toBe(true);
    unmount(tr);

    document.body.innerHTML = '<div id="app"></div>';
    window.__TOMOYO_CLIP_INIT__ = {
      theme: 'light', capture_enabled: true, total: 1, tags: [{ tag: 'work', count: 1 }],
      items: [{ key: 'k1', kind: 'text', subtype: 'text', preview: 'hello', text: 'hello', ts: 1700000000, pinned: false, tag: 'work', note: '', source_app: 'code.exe' }],
    };
    const cb = mount(ClipboardApp, { target: document.getElementById('app')! });
    flushSync();
    expect(document.body.textContent).toContain('剪贴板工作台');
    document.querySelector<HTMLButtonElement>('button[title="剪贴板插件设置"]')?.click();
    expect(wv.posted.some((m) => m.cmd === 'open_settings' && m.target === 'plugins:dev.tomoyo.clipboard')).toBe(true);
    unmount(cb);

    document.body.innerHTML = '<div id="app"></div>';
    window.__TOMOYO_OCR_DATA__ = { theme: 'midnight', text: 'Line 1\nLine 2', raw_text: 'Line 1\nLine 2', lang_tag: 'zh-Hans-CN', src_dims: [100, 50], proc_dims: [200, 100], elapsed_ms: 42 };
    const ocr = mount(OcrApp, { target: document.getElementById('app')! });
    flushSync();
    expect(document.body.textContent).toContain('OCR 审校与编辑');
    unmount(ocr);

    expect(buildComboFromEvent(new KeyboardEvent('keydown', { code: 'F1' })).combo).toBe('F1');
    const mockCfg = {
      hotkey: 'Alt+Space',
      clipboard: { hotkey: 'Alt+V' },
      translate: { hotkey: 'Alt+T' },
      capture: { hotkey: 'Alt+S', pin_hotkey: 'F3', ocr_hotkey: 'F4' },
    };
    expect(assignCoreHotkeyWithSwap(mockCfg, 'capture', 'F4', {
      main: true, clipboard: true, translate: true, capture: true, pin: true, ocr: true,
    })).toBe('ocr');
  });
});
