<script lang="ts">
  import { untrack } from 'svelte';
  import { ChevronLeft, ChevronRight, Download, ExternalLink, Info, RefreshCw, ShieldAlert, Trash2, X } from '@lucide/svelte';
  import { localizePermission, localizeRuntime } from '../../lib/defaults';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import Button from '../../lib/ui/Button.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import StarterPacksCard from './StarterPacksCard.svelte';

  export interface StoreItem {
    id: string;
    name: string;
    version: string;
    description: string;
    author?: string;
    license?: string;
    homepage?: string;
    runtime_kind: string;
    category?: string;
    display_mode?: string;
    icon_url?: string;
    keywords?: string[];
    tags?: string[];
    permissions?: string[];
    source: string;
    sha256: string;
    platform_compatible?: boolean;
    supported_platforms?: string[];
    min_tomoyo_version?: string;
    downloads?: number;
    updated_at?: string;
    installed?: boolean;
    removable?: boolean;
    installed_version?: string;
    update_available?: boolean;
  }

  export interface StoreQueryParams {
    query: string;
    category: string;
    runtime: string;
    sort: string;
    page: number;
    force_refresh?: boolean;
  }

  export interface StorePageMeta {
    total: number;
    catalogTotal: number;
    page: number;
    totalPages: number;
    categoryCounts: Record<string, number>;
  }

  interface Props {
    lang: Lang;
    items: StoreItem[];
    installedIds?: string[];
    meta: StorePageMeta;
    statusText: string;
    loading: boolean;
    detail?: StoreItem | null;
    detailLoading?: boolean;
    onQuery: (params: StoreQueryParams) => void;
    onInspectDetail?: (pluginId: string) => void;
    onCloseDetail?: () => void;
  }

  let { lang, items, installedIds = [], meta, statusText, loading, detail = null, detailLoading = false, onQuery, onInspectDetail, onCloseDetail }: Props = $props();
  let isEn = $derived(lang === 'en');
  let catalogById = $state<Record<string, StoreItem>>({});

  $effect(() => {
    if (items.length === 0) return;
    const next: Record<string, StoreItem> = { ...untrack(() => catalogById) };
    for (const it of items) next[it.id.toLowerCase()] = it;
    catalogById = next;
  });

  let query = $state('');
  let category = $state('all');
  let runtime = $state('all');
  let sort = $state('default');
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  const CATS = [
    { id: 'all', zh: '全部', en: 'All' },
    { id: 'productivity', zh: '效率', en: 'Productivity' },
    { id: 'docs', zh: '文档笔记', en: 'Docs & Notes' },
    { id: 'dev', zh: '开发调试', en: 'Developer' },
    { id: 'ai', zh: 'AI 与识字', en: 'AI & OCR' },
    { id: 'media', zh: '图像媒体', en: 'Media' },
    { id: 'system', zh: '系统', en: 'System' },
  ];

  function emitQuery(page = 1, force_refresh = false) { onQuery({ query, category, runtime, sort, page, force_refresh }); }
  function onSearchInput() { if (debounceTimer) clearTimeout(debounceTimer); debounceTimer = setTimeout(() => emitQuery(1), 160); }
  function seedHue(id: string): number { let h = 215; for (let i = 0; i < id.length; i++) h = (h * 33 + id.charCodeAt(i)) % 360; return h; }
  function modeText(mode?: string): string { return mode === 'inline' ? (isEn ? 'Inline' : '结果内嵌') : mode === 'window' ? (isEn ? 'Window' : '独立窗口') : ''; }
  function isHighRisk(perm: string): boolean { return ['shell_exec', 'filesystem_write', 'network'].includes(perm); }
  function installOrUpgrade(p: StoreItem) { postHost({ cmd: 'install_plugin', plugin_id: p.id, source: p.source, sha256: p.sha256 }); }
</script>

<div class="tab-head-bar">
  <div>
    <h2>{tr(lang, 'h_store')}</h2>
    <p class="tab-lead">
      {isEn
        ? `Rust-indexed sharded catalog with ETag/304 cache (${meta.catalogTotal || items.length} indexed · ready for 10,000+ plugins)`
        : `Rust 分片索引 + ETag 磁盘缓存（已索引 ${meta.catalogTotal || items.length} 款 · 支持万级扩展按页加载）`}
    </p>
  </div>
  <Button variant="secondary" disabled={loading} onclick={() => emitQuery(meta.page, true)}>
    <RefreshCw size={13} />
    {tr(lang, 'btn_refresh_store')}
  </Button>
</div>

<StarterPacksCard {lang} {catalogById} {installedIds} />

<div class="t-card store-toolbar">
  <div class="toolbar-row">
    <div style="flex:1;min-width:190px">
      <Input bind:value={query} size="sm" type="search" placeholder={isEn ? 'Search 10k+ plugins by name, keyword, author…' : '检索市场插件（名称 / 关键词 / 作者 / ID）…'} oninput={onSearchInput} />
    </div>
    <Select
      size="sm"
      minWidth="126px"
      value={runtime}
      options={[
        { value: 'all', label: isEn ? 'All Runtimes' : '全部运行时' },
        { value: 'stdio_jsonrpc', label: 'STDIO 原生' },
        { value: 'wasm', label: 'WASM 沙箱' },
        { value: 'webview', label: 'WebView2' },
      ]}
      onchange={(v) => { runtime = v; emitQuery(1); }}
    />
    <Select
      size="sm"
      minWidth="112px"
      value={sort}
      options={[
        { value: 'default', label: isEn ? 'Featured' : '默认排序' },
        { value: 'name', label: isEn ? 'Name A-Z' : '名称排序' },
        { value: 'downloads', label: isEn ? 'Popular' : '热度下载' },
      ]}
      onchange={(v) => { sort = v; emitQuery(1); }}
    />
  </div>
  <div class="cat-pills">
    {#each CATS as c}
      {@const cnt = c.id === 'all' ? meta.catalogTotal : (meta.categoryCounts[c.id] || 0)}
      {#if c.id === 'all' || cnt > 0}
        <button type="button" class="cat-pill {category === c.id ? 'is-active' : ''}" onclick={() => { category = c.id; emitQuery(1); }}>
          {isEn ? c.en : c.zh}{#if cnt > 0} <span class="cat-n">{cnt}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
</div>

{#if detailLoading || detail}
  <div class="t-card detail-drawer">
    {#if detailLoading && !detail}
      <div class="t-skeleton" style="height:82px"></div>
    {:else if detail}
      <div class="detail-head">
        <div class="store-logo" style="background:{detail.icon_url ? 'transparent' : `hsl(${seedHue(detail.id)}, 62%, 48%)`}">
          {#if detail.icon_url}<img src={detail.icon_url} alt={detail.id} width="36" height="36" />{:else}<span>{detail.name.slice(0, 1).toUpperCase()}</span>{/if}
        </div>
        <div style="flex:1;min-width:0">
          <div class="store-title">
            <span class="t-label">{detail.name}</span>
            <span class="t-pill">v{detail.version}</span>
            <span class="t-pill">{localizeRuntime(detail.runtime_kind, isEn)}</span>
            {#if modeText(detail.display_mode)}<span class="t-pill">{modeText(detail.display_mode)}</span>{/if}
          </div>
          <div class="t-desc">{detail.author || 'Community'} · {detail.license || 'MIT'} · <span class="mono">{detail.id}</span></div>
        </div>
        <button type="button" class="icon-x" aria-label="Close" onclick={() => onCloseDetail?.()}><X size={14} /></button>
      </div>
      <p class="detail-desc">{detail.description}</p>
      <div class="detail-grid">
        <div><span class="d-k">{isEn ? 'Triggers' : '触发词'}</span> <span class="mono">{(detail.keywords || []).join(', ') || '-'}</span></div>
        <div><span class="d-k">SHA-256</span> <span class="mono">{detail.sha256}</span></div>
      </div>
      <div class="detail-perms">
        <span class="d-k">{isEn ? 'Permissions' : '声明权限'}</span>
        {#if (detail.permissions || []).length === 0}
          <span class="t-pill">{tr(lang, 'perm_none')}</span>
        {:else}
          {#each detail.permissions || [] as perm}
            <span class="t-pill {isHighRisk(perm) ? 'perm-risk' : ''}">
              {#if isHighRisk(perm)}<ShieldAlert size={11} />{/if}{localizePermission(perm, isEn)}
            </span>
          {/each}
        {/if}
      </div>
      <div class="detail-foot">
        {#if detail.homepage}
          <Button variant="secondary" size="sm" onclick={() => detail?.homepage && postHost({ cmd: 'open_external', url: detail.homepage })}>
            <ExternalLink size={12} />{isEn ? 'Homepage' : '项目主页'}
          </Button>
        {/if}
        <div style="flex:1"></div>
        {#if detail.update_available}
          <Button variant="primary" size="sm" onclick={() => detail && installOrUpgrade(detail)}>
            <Download size={13} />{isEn ? `Upgrade to v${detail.version}` : `升级至 v${detail.version}`}
          </Button>
        {:else if !detail.installed && detail.platform_compatible === false}
          <span class="t-pill">{isEn ? 'Unsupported OS' : '暂不支持当前系统'}</span>
        {:else if !detail.installed}
          <Button variant="primary" size="sm" onclick={() => detail && installOrUpgrade(detail)}>
            <Download size={13} />{isEn ? 'Install' : '安装'}
          </Button>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<div class="t-card store-wrap">
  {#if statusText}
    <div class="t-desc" style="margin-bottom:10px">{statusText} · {isEn ? `${meta.total} matched` : `命中 ${meta.total} 项`}</div>
  {/if}

  {#if loading}
    <div class="skel-list">
      <div class="t-skeleton" style="height:58px"></div>
      <div class="t-skeleton" style="height:58px"></div>
    </div>
  {:else if items.length === 0}
    <div class="t-desc">{isEn ? 'No plugins match your filters.' : '当前检索条件暂无匹配插件条目'}</div>
  {:else}
    {#each items as p}
      {@const mLabel = modeText(p.display_mode)}
      <div class="store-row">
        <div class="store-logo" style="background:{p.icon_url ? 'transparent' : `hsl(${seedHue(p.id)}, 62%, 48%)`}">
          {#if p.icon_url}
            <img src={p.icon_url} alt={p.id} width="36" height="36" />
          {:else}
            <span>{p.name.slice(0, 1).toUpperCase()}</span>
          {/if}
        </div>
        <div class="store-info">
          <div class="store-title">
            <span class="t-label">{p.name}</span>
            <span class="t-pill">{localizeRuntime(p.runtime_kind, isEn)}</span>
            {#if mLabel}<span class="t-pill">{mLabel}</span>{/if}
            <span class="store-ver">v{p.version}</span>
            {#if p.update_available}
              <span class="t-pill upd-pill">↑ {p.installed_version} → {p.version}</span>
            {/if}
          </div>
          <div class="t-desc">{p.description}</div>
          <div class="store-meta">
            <span>{p.id}</span>
            {#if p.keywords?.length}<span>· {(p.keywords || []).join(', ')}</span>{/if}
            {#if p.sha256}<span>· SHA-256 {(p.sha256 || '').slice(0, 12)}…</span>{/if}
          </div>
        </div>
        <div class="store-act">
          <Button variant="secondary" size="sm" onclick={() => onInspectDetail?.(p.id)}>
            <Info size={13} />{isEn ? 'Details' : '详情'}
          </Button>
          {#if p.update_available}
            <Button variant="primary" size="sm" onclick={() => installOrUpgrade(p)}>
              <Download size={13} />{isEn ? 'Upgrade' : '升级'}
            </Button>
          {:else if p.removable}
            <Button variant="danger" size="sm" onclick={() => postHost({ cmd: 'uninstall_plugin', plugin_id: p.id })}>
              <Trash2 size={13} />{isEn ? 'Uninstall' : '卸载'}
            </Button>
          {:else if p.installed}
            <span class="t-pill">{isEn ? 'Installed' : '已安装'}</span>
          {:else if p.platform_compatible === false}
            <span class="t-pill">{isEn ? 'Unsupported OS' : '暂不支持当前系统'}</span>
          {:else}
            <Button variant="primary" size="sm" onclick={() => installOrUpgrade(p)}>
              <Download size={13} />{isEn ? 'Install' : '安装'}
            </Button>
          {/if}
        </div>
      </div>
    {/each}
  {/if}

  {#if meta.totalPages > 1}
    <div class="pager">
      <Button variant="secondary" size="sm" disabled={loading || meta.page <= 1} onclick={() => emitQuery(meta.page - 1)}>
        <ChevronLeft size={13} />{isEn ? 'Prev' : '上一页'}
      </Button>
      <span class="t-desc">{isEn ? `Page ${meta.page} / ${meta.totalPages}` : `第 ${meta.page} / ${meta.totalPages} 页 · 每页 24 项`}</span>
      <Button variant="secondary" size="sm" disabled={loading || meta.page >= meta.totalPages} onclick={() => emitQuery(meta.page + 1)}>
        {isEn ? 'Next' : '下一页'}<ChevronRight size={13} />
      </Button>
    </div>
  {/if}
</div>

<style>
  .store-toolbar { padding: 12px 14px; margin-bottom: 12px; display: flex; flex-direction: column; gap: 10px; }
  .toolbar-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .cat-pills { display: flex; gap: 6px; flex-wrap: wrap; }
  .cat-pill { display: inline-flex; align-items: center; gap: 4px; padding: 4px 10px; border-radius: 999px; border: 1px solid var(--border); background: var(--canvas); color: var(--text-secondary); font-family: inherit; font-size: 12px; cursor: pointer; }
  .cat-pill.is-active { border-color: var(--accent); background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .cat-n { opacity: 0.72; font-size: 11px; font-variant-numeric: tabular-nums; }
  .detail-drawer { padding: 14px 16px; margin-bottom: 12px; display: flex; flex-direction: column; gap: 10px; border-color: var(--accent); }
  .detail-head { display: flex; align-items: center; gap: 10px; }
  .detail-desc { font-size: 12.5px; color: var(--text-secondary); }
  .detail-grid { display: flex; flex-direction: column; gap: 4px; font-size: 11.5px; color: var(--muted); word-break: break-all; }
  .detail-perms { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .detail-foot { display: flex; align-items: center; gap: 8px; padding-top: 6px; border-top: 1px solid var(--border); }
  .d-k { font-size: 11.5px; font-weight: 600; color: var(--text-secondary); margin-right: 4px; }
  .perm-risk { background: var(--warn-soft); color: var(--warn); }
  .upd-pill { background: var(--ok-soft); color: var(--ok); }
  .icon-x { width: 26px; height: 26px; border-radius: 7px; display: inline-flex; align-items: center; justify-content: center; color: var(--muted); cursor: pointer; }
  .icon-x:hover { background: var(--hover); color: var(--text); }
  .store-wrap { padding: 14px 16px; }
  .skel-list { display: flex; flex-direction: column; gap: 10px; }
  .store-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--border); }
  .store-row:last-child { border-bottom: 0; padding-bottom: 0; }
  .store-row:first-child { padding-top: 0; }
  .store-logo { width: 36px; height: 36px; border-radius: 9px; flex-shrink: 0; overflow: hidden; display: inline-flex; align-items: center; justify-content: center; color: #fff; font-weight: 700; font-size: 14px; }
  .store-logo img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .store-info { flex: 1; min-width: 0; }
  .store-title { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .store-ver { font-size: 11.5px; color: var(--muted); }
  .store-meta { margin-top: 4px; font-size: 11px; color: var(--muted); font-family: "Cascadia Mono", Consolas, monospace; display: flex; gap: 6px; flex-wrap: wrap; }
  .store-act { flex-shrink: 0; display: flex; align-items: center; gap: 6px; }
  .pager { display: flex; align-items: center; justify-content: space-between; padding-top: 12px; margin-top: 8px; border-top: 1px solid var(--border); }
</style>
