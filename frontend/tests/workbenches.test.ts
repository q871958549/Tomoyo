import { beforeEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import TranslatorApp from '../src/apps/translator/TranslatorApp.svelte';
import ClipboardApp from '../src/apps/clipboard/ClipboardApp.svelte';
import OcrApp from '../src/apps/ocr/OcrApp.svelte';
import FlowsTab from '../src/apps/settings/FlowsTab.svelte';
import type { WorkflowDef } from '../src/lib/types';

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

describe('Tomoyo Workbenches & FlowsTab interactions', () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="app"></div>';
    document.body.className = '';
  });

  it('TranslatorApp translates, saves word, opens TranslatorDrawer, and deletes word via Protocol v3 callPlugin', async () => {
    setupMockWebView();
    const calls: Array<{ method: string; params: Record<string, unknown> }> = [];
    window.__TOMOYO_TR_INIT__ = undefined;
    window.tomoyo = {
      theme: 'dark',
      initialQuery: 'serendipity',
      callPlugin: (async (method: string, params?: unknown) => {
        const p = (params ?? {}) as Record<string, unknown>;
        calls.push({ method, params: p });
        if (method === 'get_init') {
          return {
            text: 'serendipity',
            target_lang: 'zh-Hans',
            engines: ['google'],
            history: [{ source: 'hello', translated: '你好', engine: 'google', target_lang: 'zh-Hans' }],
            wordbook: [{ word: 'ephemeral', translation: '短暂的', phonetic: 'ɪˈfemərəl', note: 'google' }],
          };
        }
        if (method === 'translate') {
          return {
            data: {
              source_text: 'serendipity',
              target_lang: 'zh-Hans',
              results: [{
                engine_name: 'Google',
                ok: true,
                translated: '意外发现珍奇事物的运气',
                detected_lang: 'en',
                phonetic: 'ˌserənˈdɪpəti',
                elapsed_ms: 85,
              }],
            },
            store: {
              history: [{ source: 'serendipity', translated: '意外发现珍奇事物的运气', engine: 'Google', target_lang: 'zh-Hans' }],
              wordbook: [{ word: 'ephemeral', translation: '短暂的', phonetic: 'ɪˈfemərəl', note: 'google' }],
            },
          };
        }
        if (method === 'save_word') {
          return {
            ok: true,
            history: [],
            wordbook: [
              { word: 'serendipity', translation: '意外发现珍奇事物的运气', phonetic: 'ˌserənˈdɪpəti', note: 'Google' },
              { word: 'ephemeral', translation: '短暂的', phonetic: 'ɪˈfemərəl', note: 'google' },
            ],
          };
        }
        if (method === 'delete_word') {
          return { ok: true, history: [], wordbook: [] };
        }
        return { ok: true, history: [], wordbook: [] };
      }) as <T = unknown>(method: string, params?: unknown) => Promise<T>,
      onEnter: () => {},
    };

    const app = mount(TranslatorApp, { target: document.getElementById('app')! });
    await new Promise((r) => setTimeout(r, 10));
    flushSync();

    expect(calls.some((c) => c.method === 'get_init')).toBe(true);
    expect(calls.some((c) => c.method === 'translate' && c.params.text === 'serendipity')).toBe(true);
    expect(document.body.textContent).toContain('意外发现珍奇事物的运气');

    const saveBtn = document.querySelector<HTMLButtonElement>('button[title="加入生词本"]');
    expect(saveBtn).not.toBeNull();
    saveBtn?.click();
    await new Promise((r) => setTimeout(r, 5));
    flushSync();
    expect(calls.some((c) => c.method === 'save_word' && c.params.word === 'serendipity')).toBe(true);

    const wbBtn = [...document.querySelectorAll<HTMLButtonElement>('header button')].find((b) => b.textContent?.includes('生词本'));
    expect(wbBtn).toBeDefined();
    wbBtn?.click();
    flushSync();
    expect(document.body.textContent).toContain('ephemeral');

    const delBtn = [...document.querySelectorAll<HTMLButtonElement>('aside.drawer button')].find((b) => b.textContent?.trim() === '移除');
    expect(delBtn).toBeDefined();
    delBtn?.click();
    await new Promise((r) => setTimeout(r, 5));
    flushSync();
    expect(calls.some((c) => c.method === 'delete_word')).toBe(true);

    window.tomoyo = undefined;
    unmount(app);
  });

  it('ClipboardApp filters by category/tag, pins, copies, and deletes items via Hybrid RPC', async () => {
    const wv = setupMockWebView();
    const calls: Array<{ method: string; params: unknown }> = [];
    const sampleItems = [
      { key: 'k1', kind: 'text', subtype: 'text', preview: 'hello\nworld', text: 'hello\nworld', ts: 1700000000, pinned: false, tag: 'work', note: 'memo', source_app: 'code.exe' },
      { key: 'k2', kind: 'text', subtype: 'link', preview: 'https://tomoyo.dev', text: 'https://tomoyo.dev', ts: 1700000010, pinned: true, tag: '', note: '', source_app: 'msedge.exe' },
    ];
    window.__TOMOYO_CLIP_INIT__ = {
      theme: 'light',
      capture_enabled: true,
      total: 2,
      tags: [{ tag: 'work', count: 1 }],
      items: sampleItems,
    };
    window.tomoyo = {
      theme: 'light',
      callPlugin: (async (method: string, params?: unknown) => {
        calls.push({ method, params });
        if (method === 'list') {
          return { total: 2, capture_enabled: true, tags: [{ tag: 'work', count: 1 }], items: sampleItems };
        }
        if (method === 'toggle_pin') return { ok: true, action: 'pin', pinned: true, key: 'k1' };
        return { ok: true };
      }) as <T = unknown>(method: string, params?: unknown) => Promise<T>,
    };

    const app = mount(ClipboardApp, { target: document.getElementById('app')! });
    flushSync();

    // Filter by tag `#work`
    const tagBtn = [...document.querySelectorAll<HTMLButtonElement>('aside.cb-rail button')].find((b) => b.textContent?.includes('work'));
    expect(tagBtn).toBeDefined();
    tagBtn?.click();
    await new Promise((r) => setTimeout(r, 5));
    flushSync();
    expect(calls.some((c) => c.method === 'list' && (c.params as { query?: string })?.query === '#work')).toBe(true);

    // Single-line copy emits merged text
    const singleBtn = [...document.querySelectorAll<HTMLButtonElement>('.det-head button')].find((b) => b.textContent?.includes('单行'));
    singleBtn?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'copy_text' && m.text === 'hello world')).toBe(true);

    // Toggle pin
    const pinBtn = [...document.querySelectorAll<HTMLButtonElement>('.det-head button')].find((b) => b.textContent?.includes('置顶'));
    pinBtn?.click();
    await new Promise((r) => setTimeout(r, 5));
    flushSync();
    expect(calls.some((c) => c.method === 'toggle_pin' && (c.params as { id_key?: string })?.id_key === 'k1')).toBe(true);

    // Keyboard ergonomics: Ctrl+F focuses search, ArrowDown cycles to k2, Enter copies k2
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', ctrlKey: true }));
    flushSync();
    expect(document.activeElement?.tagName).toBe('INPUT');

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
    flushSync();
    expect(document.querySelector('[data-clip-key="k2"]')?.classList.contains('is-sel')).toBe(true);

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'copy_text' && m.text === 'https://tomoyo.dev')).toBe(true);

    window.tomoyo = undefined;
    unmount(app);
  });

  it('OcrApp merges single line, toggles raw text, copies, and launches translator', () => {
    const wv = setupMockWebView();
    window.__TOMOYO_OCR_DATA__ = {
      theme: 'midnight',
      text: '第一行\n\n第二行',
      raw_text: '第 一 行\n\n第 二 行',
      lang_tag: 'zh-Hans-CN',
      src_dims: [120, 60],
      proc_dims: [240, 120],
      elapsed_ms: 35,
    };

    const app = mount(OcrApp, { target: document.getElementById('app')! });
    flushSync();

    const findBtn = (label: string) =>
      [...document.querySelectorAll<HTMLButtonElement>('header button')].find((b) => b.textContent?.includes(label));

    findBtn('合并单行')?.click();
    flushSync();
    expect(document.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe('第一行 第二行');

    findBtn('复制全文')?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'copy_text' && m.text === '第一行 第二行')).toBe(true);

    findBtn('一键翻译')?.click();
    flushSync();
    expect(wv.posted.some((m) => m.cmd === 'open_plugin_ui' && m.plugin_id === 'dev.tomoyo.translate' && m.query === '第一行 第二行')).toBe(true);

    findBtn('原始输出')?.click();
    flushSync();
    expect(document.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe('第 一 行\n\n第 二 行');

    unmount(app);
  });

  it('FlowsTab creates a new custom workflow cleanly and persists via onSaveFlows', () => {
    setupMockWebView();
    let saved: WorkflowDef[] = [];
    const app = mount(FlowsTab, {
      target: document.getElementById('app')!,
      props: {
        lang: 'zh',
        flows: [],
        flowsPath: 'C:\\Tomoyo\\flows.toml',
        onSaveFlows: (next: WorkflowDef[]) => { saved = next; },
      },
    });
    flushSync();

    const nameInput = document.querySelector<HTMLInputElement>('input[placeholder*="工作流名称"]');
    expect(nameInput).not.toBeNull();
    nameInput!.value = 'AI 润色剪贴板';
    nameInput!.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();

    const kwInput = document.querySelector<HTMLInputElement>('input[placeholder*="快捷触发词"]');
    kwInput!.value = 'polish';
    kwInput!.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();

    const saveBtn = document.querySelector<HTMLButtonElement>('button.btn-add-flow');
    expect(saveBtn?.disabled).toBe(false);
    saveBtn?.click();
    flushSync();

    expect(saved.length).toBe(1);
    expect(saved[0].name).toBe('AI 润色剪贴板');
    expect(saved[0].keyword).toBe('polish');
    expect(saved[0].steps.length).toBe(3);

    unmount(app);
  });
});
