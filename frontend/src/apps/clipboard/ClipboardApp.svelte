<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Code2, Copy, Download, ExternalLink, FileText, FolderOpen, Hash, Image, Languages, Layers,
    Link2, Palette, Pin, Save, Settings, Star, Trash2, Upload, WrapText, X,
  } from '@lucide/svelte';
  import { applyThemeClass, installErrorTelemetry, postHost } from '../../lib/ipc';
  import type { ContextMenuItem } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import ContextMenu from '../../lib/ui/ContextMenu.svelte';
  import Input from '../../lib/ui/Input.svelte';

  interface ClipItem {
    key: string; kind: 'text' | 'image' | 'files'; subtype: string; preview: string;
    text?: string; image_url?: string; image_path?: string; ts: number;
    pinned: boolean; tag: string; note: string; source_app: string;
  }
  interface TagCount { tag: string; count: number; }
  interface ClipBoot {
    theme?: string; focus_key?: string; total?: number; capture_enabled?: boolean;
    items?: ClipItem[]; tags?: TagCount[];
  }
  interface ClipMutateRes {
    ok?: boolean; action?: string; key?: string; pinned?: boolean; count?: number; message?: string;
  }

  const initialBoot = (window.__TOMOYO_CLIP_INIT__ ?? {}) as ClipBoot;

  const typeFilters = [
    { tok: '', label: '全部记录', icon: Layers },
    { tok: '@pin', label: '置顶收藏', icon: Star },
    { tok: '@text', label: '纯文本', icon: FileText },
    { tok: '@img', label: '图片截图', icon: Image },
    { tok: '@link', label: '网址链接', icon: Link2 },
    { tok: '@file', label: '文件目录', icon: FolderOpen },
    { tok: '@code', label: '代码 JSON', icon: Code2 },
    { tok: '@color', label: '色值', icon: Palette },
  ];

  let items = $state<ClipItem[]>(initialBoot.items || []);
  let tags = $state<TagCount[]>(initialBoot.tags || []);
  let total = $state(initialBoot.total ?? (initialBoot.items?.length || 0));
  let captureEnabled = $state(initialBoot.capture_enabled !== false);
  let activeFilter = $state('');
  let query = $state('');
  let selectedKey = $state(initialBoot.focus_key || initialBoot.items?.[0]?.key || '');
  let draftText = $state('');
  let draftTag = $state('');
  let draftNote = $state('');
  let clearArmed = $state(false);
  let importOpen = $state(false);
  let importJson = $state('');
  let toast = $state('剪贴板工作台就绪 · Ctrl+F 搜索 · ↑↓ 切换 · Ctrl+S 保存');
  let ctx = $state<{ open: boolean; x: number; y: number; item: ClipItem | null }>({ open: false, x: 0, y: 0, item: null });
  let searchEl: HTMLInputElement | undefined = $state();
  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  let current = $derived(items.find((i) => i.key === selectedKey) ?? null);

  $effect(() => {
    if (current) {
      draftText = current.text || '';
      draftTag = current.tag || '';
      draftNote = current.note || '';
    }
  });

  function fmtTime(ts: number): string {
    if (!ts) return '-';
    const d = new Date(ts * 1000);
    const pad = (n: number) => String(n).padStart(2, '0');
    return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  async function reloadList(focusKey?: string) {
    if (!window.tomoyo?.callPlugin) return;
    const q = [activeFilter, query.trim()].filter(Boolean).join(' ');
    try {
      const d = await window.tomoyo.callPlugin<ClipBoot>('list', {
        query: q,
        focus_key: focusKey ?? '',
      });
      items = Array.isArray(d?.items) ? d.items : [];
      tags = Array.isArray(d?.tags) ? d.tags : [];
      total = d?.total ?? items.length;
      captureEnabled = d?.capture_enabled !== false;
      if (focusKey && items.some((i) => i.key === focusKey)) selectedKey = focusKey;
      else if (!items.some((i) => i.key === selectedKey)) selectedKey = items[0]?.key || '';
    } catch (err) {
      toast = `✗ 加载剪贴板失败：${String(err)}`;
    }
  }

  function onSearchInput() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => { void reloadList(); }, 130);
  }

  async function runMutation(method: string, params: Record<string, unknown> = {}) {
    if (!window.tomoyo?.callPlugin) return;
    try {
      const msg = await window.tomoyo.callPlugin<ClipMutateRes>(method, params);
      if (typeof msg?.key === 'string' && msg.key) selectedKey = msg.key;
      if (msg?.ok === false) toast = `✗ ${msg.message || '操作失败'}`;
      else if (msg?.action === 'update') toast = '✓ 已保存修改并同步剪贴板库';
      else if (msg?.action === 'delete') toast = '✓ 已删除记录';
      else if (msg?.action === 'clear') toast = `✓ 已清理 ${msg.count || 0} 条非置顶记录`;
      else if (msg?.action === 'import') toast = `✓ 已导入 ${msg.count || 0} 条记录`;
      else if (msg?.action === 'pin') toast = msg.pinned ? '★ 已置顶收藏' : '已取消置顶';
      await reloadList();
    } catch (err) {
      toast = `✗ 操作失败：${String(err)}`;
    }
  }

  function saveDetail() {
    if (!current) return;
    void runMutation('update', {
      id_key: current.key,
      text: current.kind === 'text' ? draftText : null,
      tag: draftTag.trim(),
      note: draftNote.trim(),
    });
  }

  async function exportBackup() {
    if (!window.tomoyo?.callPlugin) return;
    try {
      const res = await window.tomoyo.callPlugin<{ json?: string }>('export', {});
      postHost({ cmd: 'copy_text', text: String(res?.json || '[]') });
      toast = '✓ JSON 备份已复制到剪贴板';
    } catch (err) {
      toast = `✗ 导出失败：${String(err)}`;
    }
  }

  function copyItem(it: ClipItem | null, singleLine = false) {
    if (!it) return;
    if (it.kind === 'image' && it.image_path) {
      postHost({ cmd: 'copy_text', text: it.image_path });
      toast = '✓ 已复制图片路径';
      return;
    }
    let txt = it.key === current?.key ? draftText : (it.text || '');
    if (singleLine) txt = txt.split(/\s+/).filter(Boolean).join(' ');
    postHost({ cmd: 'copy_text', text: txt });
    toast = singleLine ? '✓ 已复制单行纯文本' : '✓ 已复制到剪贴板';
  }

  function translateItem(it: ClipItem | null) {
    if (!it || it.kind !== 'text') return;
    const txt = (it.key === current?.key ? draftText : (it.text || '')).trim();
    if (!txt) return;
    postHost({ cmd: 'open_plugin_ui', plugin_id: 'dev.tomoyo.translate', query: txt });
    toast = `已发送 ${txt.length} 字到 Tomoyo 翻译`;
  }

  function clearUnpinned() {
    if (!clearArmed) {
      clearArmed = true;
      setTimeout(() => { clearArmed = false; }, 3500);
      return;
    }
    clearArmed = false;
    void runMutation('clear_unpinned');
  }

  let ctxItems = $derived.by<ContextMenuItem[]>(() => {
    const it = ctx.item;
    if (!it) return [];
    return [
      { id: 'copy', label: '复制内容', shortcut: '双击' },
      { id: 'single', label: '合并单行复制', disabled: it.kind === 'image' },
      { id: 'translate', label: '发送到多源翻译', disabled: it.kind !== 'text' },
      { id: 'pin', label: it.pinned ? '取消置顶' : '置顶收藏', checked: it.pinned },
      { id: 'sep', label: '', separator: true },
      { id: 'delete', label: '删除记录', danger: true },
    ];
  });

  function onCtxSelect(id: string) {
    const it = ctx.item;
    if (!it) return;
    if (id === 'copy') copyItem(it, false);
    else if (id === 'single') copyItem(it, true);
    else if (id === 'translate') translateItem(it);
    else if (id === 'pin') void runMutation('toggle_pin', { id_key: it.key });
    else if (id === 'delete') void runMutation('delete', { id_key: it.key });
  }

  onMount(() => {
    installErrorTelemetry('clip_workbench');
    applyThemeClass(initialBoot.theme || window.tomoyo?.theme || 'dark');

    if (window.tomoyo?.callPlugin && !initialBoot.items) {
      void reloadList(window.tomoyo.initialQuery || '');
    }
    window.tomoyo?.onEnter?.((action) => {
      void reloadList(action?.payload || '');
    });

    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const inTextarea = target?.tagName === 'TEXTAREA';
      const inOtherInput = target?.tagName === 'INPUT' && target !== searchEl;
      if (e.ctrlKey && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        searchEl?.focus();
        searchEl?.select();
      } else if (e.ctrlKey && e.key.toLowerCase() === 's') {
        e.preventDefault();
        saveDetail();
      } else if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 't') {
        e.preventDefault();
        postHost({ cmd: 'toggle_pin_topmost' });
      } else if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && !inTextarea && !inOtherInput && !importOpen && items.length > 0) {
        e.preventDefault();
        const curIdx = items.findIndex((i) => i.key === selectedKey);
        const delta = e.key === 'ArrowDown' ? 1 : -1;
        const nextIdx = curIdx < 0 ? 0 : (curIdx + delta + items.length) % items.length;
        selectedKey = items[nextIdx].key;
        document.querySelector(`[data-clip-key="${CSS.escape(selectedKey)}"]`)?.scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'Enter' && !inTextarea && !inOtherInput && !importOpen && current) {
        e.preventDefault();
        copyItem(current, false);
      } else if (e.key === 'Escape' && !e.defaultPrevented) {
        if (importOpen) importOpen = false;
        else postHost({ cmd: 'close' });
      }
    };
    window.addEventListener('keydown', onKey);
    return () => { window.removeEventListener('keydown', onKey); };
  });
</script>

<div class="wb-shell">
  <header class="wb-head">
    <div class="cb-head-l">
      <strong>剪贴板工作台</strong>
      <span class="t-pill">{total} 条</span>
      <span class="t-pill" style:color={captureEnabled ? 'var(--ok)' : 'var(--warn)'}>{captureEnabled ? '采集中' : '已暂停'}</span>
    </div>
    <div class="cb-head-r">
      <Button variant="secondary" size="sm" onclick={exportBackup}><Download size={13} />导出 JSON</Button>
      <Button variant="secondary" size="sm" onclick={() => (importOpen = true)}><Upload size={13} />导入</Button>
      <Button variant="danger" size="sm" onclick={clearUnpinned}>{clearArmed ? '确认清空' : '清空非置顶'}</Button>
      <Button variant="icon" size="sm" title="剪贴板插件设置" onclick={() => postHost({ cmd: 'open_settings', target: 'plugins:dev.tomoyo.clipboard' })}><Settings size={14} /></Button>
      <Button variant="icon" size="sm" title="置顶" onclick={() => postHost({ cmd: 'toggle_pin_topmost' })}><Pin size={14} /></Button>
      <Button variant="icon" size="sm" title="关闭" onclick={() => postHost({ cmd: 'close' })}><X size={14} /></Button>
    </div>
  </header>

  <div class="cb-main">
    <aside class="cb-rail">
      <div class="rail-grp">分类</div>
      {#each typeFilters as f}
        {@const Icon = f.icon}
        <button
          type="button"
          class="rail-btn {activeFilter === f.tok ? 'is-active' : ''}"
          onclick={() => { activeFilter = f.tok; void reloadList(); }}
        >
          <Icon size={14} /><span>{f.label}</span>
        </button>
      {/each}

      <div class="rail-grp" style="margin-top:12px">标签</div>
      {#each tags as t}
        {@const tok = '#' + t.tag}
        <button
          type="button"
          class="rail-btn {activeFilter === tok ? 'is-active' : ''}"
          onclick={() => { activeFilter = tok; void reloadList(); }}
        >
          <Hash size={13} /><span style="flex:1;overflow:hidden;text-overflow:ellipsis">{t.tag}</span><span class="cnt">{t.count}</span>
        </button>
      {:else}
        <div class="t-desc" style="padding:4px 8px">暂无标签</div>
      {/each}
    </aside>

    <section class="cb-list-pane">
      <div class="cb-search">
        <Input type="search" size="sm" bind:inputEl={searchEl} bind:value={query} placeholder="搜索内容、拼音或 #标签… (Ctrl+F)" oninput={onSearchInput} />
      </div>
      <div class="cb-scroll">
        {#each items as it}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div
            class="cb-card {it.key === selectedKey ? 'is-sel' : ''}"
            data-clip-key={it.key}
            onclick={() => (selectedKey = it.key)}
            ondblclick={() => copyItem(it, false)}
            oncontextmenu={(e) => { e.preventDefault(); selectedKey = it.key; ctx = { open: true, x: e.clientX, y: e.clientY, item: it }; }}
          >
            <div class="cb-card-top">
              <span class="t-pill">{it.subtype.toUpperCase()}</span>
              <span class="cb-ts">{fmtTime(it.ts)}</span>
            </div>
            <div class="cb-prev">{it.pinned ? '★ ' : ''}{it.preview}</div>
            {#if it.kind === 'image' && it.image_url}
              <img class="cb-thumb" src={it.image_url} alt="" loading="lazy" />
            {/if}
            <div class="cb-meta">
              {#if it.tag}<span class="t-pill">#{it.tag}</span>{/if}
              {#if it.source_app}<span>{it.source_app}</span>{/if}
              {#if it.note}<span>· {it.note}</span>{/if}
            </div>
          </div>
        {:else}
          <div class="wb-empty">当前条件下无剪贴板记录</div>
        {/each}
      </div>
    </section>

    <section class="cb-detail">
      {#if !current}
        <div class="wb-empty" style="margin:auto">选择左侧条目查看或编辑详情</div>
      {:else}
        <div class="det-head">
          <div class="cb-head-l">
            <span class="t-pill">{current.subtype.toUpperCase()}</span>
            <span class="t-desc">{current.source_app || '未知来源'} · {fmtTime(current.ts)}</span>
          </div>
          <div class="cb-head-r">
            <Button variant="primary" size="sm" onclick={() => copyItem(current, false)}><Copy size={13} />复制</Button>
            {#if current.kind !== 'image'}
              <Button variant="secondary" size="sm" onclick={() => copyItem(current, true)}><WrapText size={13} />单行</Button>
            {/if}
            {#if current.kind === 'text'}
              <Button variant="secondary" size="sm" onclick={() => translateItem(current)}><Languages size={13} />翻译</Button>
            {/if}
            <Button variant="secondary" size="sm" active={current.pinned} onclick={() => void runMutation('toggle_pin', { id_key: current.key })}>
              <Star size={13} />{current.pinned ? '取消置顶' : '置顶'}
            </Button>
            <Button variant="danger" size="sm" onclick={() => void runMutation('delete', { id_key: current.key })}><Trash2 size={13} /></Button>
          </div>
        </div>

        <div class="det-meta">
          <Input size="sm" bind:value={draftTag} placeholder="标签（如 work / api）" maxlength={24} />
          <Input size="sm" bind:value={draftNote} placeholder="备注说明…" maxlength={120} />
          <Button variant="secondary" size="sm" onclick={saveDetail}><Save size={13} />保存</Button>
        </div>

        {#if current.kind === 'image'}
          <div class="img-stage">
            <img src={current.image_url || ''} alt="Preview" />
          </div>
          <div class="img-bar">
            <Button variant="secondary" size="sm" onclick={() => current.image_path && postHost({ cmd: 'open_path', path: current.image_path })}>
              <ExternalLink size={13} />打开原图文件
            </Button>
          </div>
        {:else}
          <textarea class="det-ta" bind:value={draftText} readonly={current.kind === 'files'} spellcheck="false"></textarea>
        {/if}
      {/if}
    </section>
  </div>

  <footer class="wb-foot">
    <span class="wb-foot-toast">{toast}</span>
    <span>Ctrl+F 搜索 · ↑↓ 切换 · Enter/双击复制 · Ctrl+S 保存 · Esc 退出</span>
  </footer>
</div>

{#if importOpen}
  <div class="modal-bg">
    <div class="t-card modal-box">
      <div class="det-head" style="border-bottom:1px solid var(--border);padding-bottom:10px">
        <strong>导入 JSON 剪贴板备份</strong>
        <Button variant="icon" size="sm" onclick={() => (importOpen = false)}><X size={14} /></Button>
      </div>
      <textarea class="modal-ta" bind:value={importJson} placeholder="粘贴导出的 JSON 数组…"></textarea>
      <div style="display:flex;justify-content:flex-end;gap:8px">
        <Button variant="secondary" size="sm" onclick={() => (importOpen = false)}>取消</Button>
        <Button variant="primary" size="sm" onclick={() => { if (importJson.trim()) { void runMutation('import', { json: importJson.trim() }); importJson = ''; importOpen = false; } }}>确认导入</Button>
      </div>
    </div>
  </div>
{/if}

<ContextMenu open={ctx.open} x={ctx.x} y={ctx.y} items={ctxItems} onselect={onCtxSelect} onclose={() => (ctx.open = false)} />

<style>
  .cb-head-l, .cb-head-r { display: flex; align-items: center; gap: 7px; }
  .cb-main { flex: 1; display: flex; min-height: 0; overflow: hidden; }
  .cb-rail {
    width: 158px; flex-shrink: 0; background: var(--sidebar); border-right: 1px solid var(--border);
    padding: 10px 8px; overflow-y: auto; display: flex; flex-direction: column; gap: 2px;
  }
  .rail-grp { font-size: 11px; font-weight: 600; color: var(--muted); padding: 4px 8px; }
  .rail-btn {
    display: flex; align-items: center; gap: 8px; height: 30px; padding: 0 9px; border: 0;
    border-radius: 6px; background: transparent; color: var(--text-secondary); font-family: inherit;
    font-size: 12.5px; text-align: left; white-space: nowrap; cursor: pointer;
  }
  .rail-btn:hover:not(.is-active) { background: var(--hover); color: var(--text); }
  .rail-btn.is-active { background: var(--accent-soft); color: var(--accent); font-weight: 500; }
  .cnt { font-size: 11px; opacity: 0.75; }
  .cb-list-pane {
    width: 320px; flex-shrink: 0; border-right: 1px solid var(--border); background: var(--canvas);
    display: flex; flex-direction: column; min-height: 0;
  }
  .cb-search { padding: 8px 10px; border-bottom: 1px solid var(--border); background: var(--surface); }
  .cb-scroll { flex: 1; overflow-y: auto; padding: 8px; display: flex; flex-direction: column; gap: 6px; }
  .cb-card {
    background: var(--surface); border: 1px solid var(--border); border-radius: 9px;
    padding: 8px 10px; cursor: pointer; transition: border-color 0.12s ease;
  }
  .cb-card:hover { border-color: var(--border-strong); }
  .cb-card.is-sel { border-color: var(--accent); box-shadow: 0 0 0 2px var(--ring); }
  .cb-card-top { display: flex; justify-content: space-between; align-items: center; margin-bottom: 5px; }
  .cb-ts { font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .cb-prev { font-size: 12.5px; line-height: 1.45; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; word-break: break-word; }
  .cb-thumb { margin-top: 6px; max-height: 64px; max-width: 100%; border-radius: 5px; object-fit: cover; }
  .cb-meta { margin-top: 5px; display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--muted); flex-wrap: wrap; }
  .cb-detail { flex: 1; display: flex; flex-direction: column; min-width: 0; background: var(--surface); }
  .det-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 10px 14px; border-bottom: 1px solid var(--border); flex-wrap: wrap; }
  .det-meta { display: grid; grid-template-columns: 150px 1fr auto; gap: 8px; padding: 10px 14px; border-bottom: 1px solid var(--border); background: var(--canvas); }
  .det-ta {
    flex: 1; width: 100%; border: 0; outline: none; resize: none; padding: 14px; background: transparent;
    color: var(--text); font-family: var(--font-mono); font-size: 13px; line-height: 1.65;
  }
  .img-stage { flex: 1; display: grid; place-items: center; overflow: auto; padding: 16px; background: var(--img-stage); }
  .img-stage img { max-width: 100%; max-height: 100%; border-radius: 8px; box-shadow: var(--shadow-pop); }
  .img-bar { padding: 10px 14px; border-top: 1px solid var(--border); display: flex; justify-content: flex-end; }
  .modal-bg { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.45); display: grid; place-items: center; z-index: 90; }
  .modal-box { width: 520px; max-width: 92vw; padding: 14px 16px; display: flex; flex-direction: column; gap: 10px; }
  .modal-ta {
    width: 100%; height: 180px; border: 1px solid var(--border); border-radius: 8px; padding: 10px;
    background: var(--canvas); color: var(--text); font-family: var(--font-mono); font-size: 12px; outline: none;
  }
</style>
