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
    const origHealth = {
      main: { spec: 'Alt+Space', active: 'Alt+Space', ok: true, disabled: false, reason: '' },
      clipboard: { spec: 'Alt+C', active: '', ok: false, disabled: false, reason: '已被系统或其他软件占用（Alt+C）' },
      translate: { spec: 'Alt+T', active: '', ok: false, disabled: false, reason: '已被系统或其他软件占用（Alt+T）' },
      capture: { spec: 'Alt+1', active: 'Alt+1', ok: true, disabled: false, reason: '' },
      pin: { spec: 'F3', active: 'F3', ok: true, disabled: false, reason: '' },
    };
    window.__TOMOYO_SETTINGS_INIT__ = {
      version: '1.8.0',
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
          tool_hotkeys: { rect: 'R', ellipse: 'E', arrow: 'A', pen: 'B', highlighter: 'H', mosaic: 'M', step: 'N', text: 'X', scroll: 'L', pin: 'P', ocr: 'O', ocr_copy: 'Shift+C', translate: 'T' },
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

    const navBtns = document.querySelectorAll<HTMLButtonElement>('nav button[data-tab]');
    expect(navBtns.length).toBe(12);
    expect(document.querySelector<HTMLImageElement>('img.brand-logo')?.getAttribute('src')).toBeTruthy();

    // Bottom conflict bar must be a clickable button pointing directly to the conflicted slot!
    const conflictBtn = document.querySelector<HTMLButtonElement>('button#toast.conflict-bar-btn');
    expect(conflictBtn).not.toBeNull();
    expect(conflictBtn?.textContent).toContain('Alt+C');
    expect(document.querySelector('button[data-tab="hotkeys"] .nav-badge')?.textContent).toBe('2');
    conflictBtn?.click();
    flushSync();
    expect(document.querySelector('button[data-tab="hotkeys"]')?.classList.contains('is-active')).toBe(true);
    expect(document.body.textContent).toContain('高优先级强行接管冲突热键');
    // Direct 1-click rescue button + fullscreen avoidance card must render on Hotkeys tab!
    expect(document.body.textContent).toContain('全屏游戏与全屏视频时自动避让热键');
    const rescueClip = document.querySelector<HTMLButtonElement>('button.btn-rescue-clipboard');
    expect(rescueClip).not.toBeNull();
    rescueClip?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'save_settings' && (m.config as { override_conflict_hotkeys?: boolean })?.override_conflict_hotkeys === true)).toBe(true);

    // Simulate enabling high-priority override: conflict bar disappears and overridden pill appears
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
    expect(document.body.textContent).toContain('已高优先级接管');

    // Restore original conflict health before iterating all tabs
    wv.emit({ event: 'hotkey_health', health: origHealth });
    flushSync();

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
      if (btn.dataset.tab === 'capture') {
        expect(document.body.textContent).toContain('全局一键贴图快捷键');
        expect(document.body.textContent).toContain('全局截图提取文字快捷键');
        expect(document.body.textContent).toContain('Shift+C');
        expect(document.body.textContent).toContain('F3');
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

    // Modular plugin switches: disabling Translate hides its sidebar tab & drops its conflict badge
    document.querySelector<HTMLButtonElement>('button[data-tab="plugins"]')?.click();
    flushSync();
    expect(document.body.textContent).toContain('官方功能插件');
    const trSwitch = document.querySelector<HTMLButtonElement>('button[role="switch"][aria-label="多源划词翻译"]');
    expect(trSwitch).not.toBeNull();
    trSwitch?.click();
    flushSync();
    expect(document.querySelector('button[data-tab="translate"]')).toBeNull();
    expect(document.querySelector('button[data-tab="hotkeys"] .nav-badge')?.textContent).toBe('1');

    // Protocol v2 declarative plugin settings & hotkey drawer
    const cfgBtn = document.querySelector<HTMLButtonElement>('button[data-config-plugin="dev.tomoyo.ocr"]');
    expect(cfgBtn).not.toBeNull();
    cfgBtn?.click();
    flushSync();
    expect(document.querySelector('[data-plugin-drawer="dev.tomoyo.ocr"]')).not.toBeNull();
    const rawSw = document.querySelector<HTMLButtonElement>('button[role="switch"][aria-label="dev.tomoyo.ocr:raw_output"]');
    expect(rawSw).not.toBeNull();
    rawSw?.click();
    flushSync();

    // User-extensible search filter tabs: one-click preset adds a custom filter tab and saves config
    document.querySelector<HTMLButtonElement>('button[data-tab="search"]')?.click();
    flushSync();
    const codePreset = document.querySelector<HTMLButtonElement>('button[data-preset-tab="code"]');
    expect(codePreset).not.toBeNull();
    codePreset?.click();
    flushSync();
    expect(document.body.textContent).toContain('@code');
    const lastSave = [...wv.posted].reverse().find((m) => m.cmd === 'save_settings');
    const savedCfg = (lastSave?.config ?? {}) as {
      custom_filters?: Array<{ id: string }>;
      disabled_plugins?: string[];
      plugin_settings?: Record<string, Record<string, unknown>>;
    };
    expect(savedCfg.custom_filters?.some((t) => t.id === 'code')).toBe(true);
    expect(savedCfg.disabled_plugins).toContain('dev.tomoyo.translate');
    expect(savedCfg.plugin_settings?.['dev.tomoyo.ocr']?.raw_output).toBe(true);

    // Declarative Action Flows tab: toggling a flow emits `save_flows` cleanly
    document.querySelector<HTMLButtonElement>('button[data-tab="flows"]')?.click();
    flushSync();
    expect(document.body.textContent).toContain('剪贴板去换行纯文本直贴');
    const flowSw = document.querySelector<HTMLButtonElement>('button[role="switch"][aria-label="剪贴板去换行纯文本直贴"]');
    expect(flowSw).not.toBeNull();
    flowSw?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'save_flows')).toBe(true);
    const hkBtn = document.querySelector<HTMLButtonElement>('button[data-flow-hk="clip_clean_paste"]');
    expect(hkBtn).not.toBeNull();
    hkBtn?.click();
    flushSync();
    expect(document.querySelector('[data-flow-hk-drawer="clip_clean_paste"]')).not.toBeNull();

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
