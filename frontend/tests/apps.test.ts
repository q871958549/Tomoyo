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
      main: { spec: 'Alt+Space', active: '', ok: false, disabled: false, reason: '已被系统或其他软件占用（Alt+Space）' },
      clipboard: { spec: '', active: '', ok: true, disabled: true, reason: '' },
      translate: { spec: '', active: '', ok: true, disabled: true, reason: '' },
      capture: { spec: '', active: '', ok: true, disabled: true, reason: '' },
      pin: { spec: '', active: '', ok: true, disabled: true, reason: '' },
    };
    window.__TOMOYO_SETTINGS_INIT__ = {
      version: '3.0.0',
      platform: 'windows',
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
        id: 'dev.tomoyo.clipboard', name: '剪贴板管理中心', version: '3.0.0', runtime: 'STDIO', keywords: ['cb', 'clip'], max_memory_mb: 96,
        declared_permissions: ['clipboard'], granted_permissions: ['clipboard'],
        has_ui: true, tray_actions_count: 1, file_actions_count: 0,
        declared_hotkeys: [{ id: 'open_cb', label: '剪贴板历史快捷搜索', default_hotkey: 'Alt+V', trigger_query: 'cb ' }],
        settings_schema: [
          { key: 'capture_enabled', label: '后台自动采集剪贴板', section: '采集与持久化', type: 'bool', default: true },
        ],
      }, {
        id: 'dev.tomoyo.ocr', name: 'OCR', version: '3.0.0', runtime: 'STDIO', keywords: ['ocr'], max_memory_mb: 128,
        declared_permissions: ['selection_read'], granted_permissions: ['selection_read'],
        has_ui: true, tray_actions_count: 1, file_actions_count: 1,
        declared_hotkeys: [{ id: 'quick_ocr', label: '快速识别截图', default_hotkey: '', trigger_query: 'ocr ' }],
        settings_schema: [
          { key: 'default_lang', label: '默认语言', section: '基础识别', type: 'select', default: 'auto', options: [{ value: 'auto', label: '自动' }, { value: 'en', label: '英文' }] },
          { key: 'raw_output', label: '保留原始空格', section: '基础识别', type: 'bool', default: false },
          { key: 'cloud_api_key', label: '云端增强密钥', section: '凭据与路径', type: 'secret', placeholder: 'sk-ocr-...' },
          { key: 'export_dir', label: '导出目录', section: '凭据与路径', type: 'dir', placeholder: '/tmp' },
          { key: 'extra_hotkey', label: '二次识别热键', type: 'hotkey', visible_if: { key: 'raw_output', equals: true } },
        ],
      }],
      flows: [{
        id: 'clip_clean_paste', name: '剪贴板去换行纯文本直贴', keyword: 'cleanpaste',
        description: '合并单行后直贴', enabled: true,
        steps: [{ kind: 'read_clipboard', value: '' }, { kind: 'single_line', value: '' }, { kind: 'paste_text', value: '' }],
      }],
      flows_path: 'C:\\Tomoyo\\flows.toml',
      running_apps: ['code.exe', 'notepad.exe'],
      secrets_status: { deepl: false, openai: true, plugins: { 'plugin:dev.tomoyo.ocr:cloud_api_key': true } },
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
    const brandSrc = document.querySelector<HTMLImageElement>('img.brand-logo')?.getAttribute('src');
    expect(brandSrc).toBeTruthy();
    const titlebarMark = document.querySelector<HTMLImageElement>('header.titlebar img.mark');
    expect(titlebarMark).not.toBeNull();
    expect(titlebarMark?.getAttribute('src')).toBe(brandSrc);
    expect(document.querySelector('header.titlebar svg.mark')).toBeNull();

    // Hotkey conflict bar + Hotkey Doctor rescue flow
    const conflictBtn = document.querySelector<HTMLButtonElement>('button#toast.conflict-bar-btn');
    expect(conflictBtn).not.toBeNull();
    expect(conflictBtn?.textContent).toContain('Alt+Space');
    expect(document.querySelector('button[data-tab="hotkeys"] .nav-badge')?.textContent).toBe('1');
    conflictBtn?.click();
    flushSync();
    expect(document.querySelector('button[data-tab="hotkeys"]')?.classList.contains('is-active')).toBe(true);
    expect(document.body.textContent).toContain('高优先级强行接管冲突热键');
    document.querySelector<HTMLButtonElement>('button.btn-rescue-main')?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'save_settings' && (m.config as { override_conflict_hotkeys?: boolean })?.override_conflict_hotkeys === true)).toBe(true);

    // Restoring conflict badge clears when fixed
    wv.emit({
      event: 'hotkey_health',
      health: {
        main: { spec: 'Alt+Space', active: 'Alt+Space', ok: true, disabled: false, overridden: true, reason: '已高优先级接管生效' },
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

    // Unified Plugin Center renders all installed Protocol v3 plugins uniformly (zero hardcoded OFFICIAL_PLUGINS!)
    document.querySelector<HTMLButtonElement>('button[data-tab="plugins"]')?.click();
    flushSync();
    expect(document.body.textContent).toContain('已安装插件（2）');

    document.querySelector<HTMLButtonElement>('button[data-config-plugin="dev.tomoyo.clipboard"]')?.click();
    flushSync();
    expect(document.querySelector('[data-plugin-drawer="dev.tomoyo.clipboard"]')).not.toBeNull();
    expect(document.body.textContent).toContain('剪贴板历史快捷搜索');

    const cbSwitch = document.querySelector<HTMLButtonElement>('button[role="switch"][aria-label="剪贴板管理中心"]');
    cbSwitch?.click();
    flushSync();

    document.querySelector<HTMLButtonElement>('button[data-config-plugin="dev.tomoyo.ocr"]')?.click();
    flushSync();
    expect(document.querySelector('[data-plugin-drawer="dev.tomoyo.ocr"]')).not.toBeNull();
    // Uninstalled optional plugins (e.g. Quiet.md) must NOT render ghost cards in Installed Plugins!
    expect(document.body.textContent).not.toContain('Quiet.md Markdown 编辑器');
    // Protocol v3 drawer renders section headers, encrypted secret status, conditional fields & path picker!
    expect(document.body.textContent).toContain('凭据与路径');
    expect(document.body.textContent).toContain('已加密保存');
    expect(document.body.textContent).toContain('Hybrid UI');
    expect(document.body.textContent).not.toContain('二次识别热键');
    document.querySelector<HTMLButtonElement>('button[role="switch"][aria-label="dev.tomoyo.ocr:raw_output"]')?.click();
    flushSync();
    expect(document.body.textContent).toContain('二次识别热键');
    wv.emit({ event: 'plugin_path_picked', plugin_id: 'dev.tomoyo.ocr', key: 'export_dir', ok: true, cancelled: false, path: '/tmp/ocr-out' });
    flushSync();
    const inlineBtn = [...document.querySelectorAll<HTMLButtonElement>('[data-plugin-drawer="dev.tomoyo.ocr"] .t-seg button')].find((b) => b.textContent?.includes('搜索结果内嵌'));
    inlineBtn?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'save_settings' && (m.config as { plugin_display_modes?: Record<string, string> })?.plugin_display_modes?.['dev.tomoyo.ocr'] === 'inline')).toBe(true);
    expect(wv.posted.some((m) => m.cmd === 'save_settings' && (m.config as { plugin_settings?: Record<string, Record<string, unknown>> })?.plugin_settings?.['dev.tomoyo.ocr']?.export_dir === '/tmp/ocr-out')).toBe(true);

    // Scalable StoreTab search + pagination + multi-platform compatibility guard
    const storeSeg = [...document.querySelectorAll<HTMLButtonElement>('.subbar button')].find((b) => b.textContent?.includes('插件市场'));
    storeSeg?.click();
    flushSync();
    wv.emit({
      event: 'store_loaded',
      total: 10000,
      catalog_total: 10000,
      page: 1,
      total_pages: 417,
      category_counts: { docs: 1200, dev: 3400 },
      sources: ['registry/index.json'],
      items: [{
        id: 'dev.tomoyo.markdown-editor',
        name: 'Quiet.md Markdown 编辑器',
        version: '2.6.0',
        description: '专业 Markdown 编辑器',
        runtime_kind: 'stdio_jsonrpc',
        category: 'docs',
        display_mode: 'window',
        source: 'plugins/official-md-editor',
        sha256: '0'.repeat(64),
        platform_compatible: true,
      }, {
        id: 'community.macos.only',
        name: 'macOS QuickLook Bridge',
        version: '1.0.0',
        description: 'macOS only helper',
        runtime_kind: 'stdio_jsonrpc',
        category: 'system',
        display_mode: 'window',
        source: '',
        sha256: '',
        platform_compatible: false,
      }],
    });
    flushSync();
    expect(document.body.textContent).toContain('10000');
    expect(document.body.textContent).toContain('417');
    expect(document.body.textContent).toContain('Quiet.md Markdown 编辑器');
    expect(document.body.textContent).toContain('暂不支持当前系统');
    expect(document.querySelector('[data-starter-packs]')).not.toBeNull();
    expect(document.body.textContent).toContain('按需装机套件');
    document.querySelector<HTMLButtonElement>('[data-pack="writing-ocr"] button')?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'install_plugin' && m.plugin_id === 'dev.tomoyo.markdown-editor')).toBe(true);

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
    expect(savedCfg.disabled_plugins).toContain('dev.tomoyo.clipboard');
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
      capture: { hotkey: 'Alt+S', pin_hotkey: 'F3', ocr_hotkey: 'F4', record_hotkey: '' },
    };
    expect(assignCoreHotkeyWithSwap(mockCfg, 'capture', 'F4', {
      main: true, clipboard: true, translate: true, capture: true, pin: true, ocr: true, record: true,
    })).toBe('ocr');
  });
});
