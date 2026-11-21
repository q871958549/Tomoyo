<script lang="ts">
  import { Download, RefreshCw, Trash2 } from '@lucide/svelte';
  import { localizeRuntime } from '../../lib/defaults';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import Button from '../../lib/ui/Button.svelte';

  export interface StoreItem {
    id: string;
    name: string;
    version: string;
    description: string;
    runtime_kind: string;
    keywords?: string[];
    source: string;
    sha256: string;
    installed?: boolean;
    removable?: boolean;
  }

  interface Props {
    lang: Lang;
    items: StoreItem[];
    statusText: string;
    loading: boolean;
    onRefresh: () => void;
  }

  let { lang, items, statusText, loading, onRefresh }: Props = $props();
</script>

<div class="tab-head-bar">
  <div>
    <h2>{tr(lang, 'h_store')}</h2>
    <p class="tab-lead">{tr(lang, 'lead_store')}</p>
  </div>
  <Button variant="secondary" disabled={loading} onclick={onRefresh}>
    <RefreshCw size={13} />
    {tr(lang, 'btn_refresh_store')}
  </Button>
</div>

<div class="t-card store-wrap">
  {#if statusText}
    <div class="t-desc" style="margin-bottom:10px">{statusText}</div>
  {/if}

  {#if loading}
    <div class="skel-list">
      <div class="t-skeleton" style="height:56px"></div>
      <div class="t-skeleton" style="height:56px"></div>
    </div>
  {:else if items.length === 0}
    <div class="t-desc">{lang === 'en' ? 'No plugins found in registry.' : '暂无可用插件条目'}</div>
  {:else}
    {#each items as p}
      <div class="store-row">
        <div class="store-info">
          <div class="store-title">
            <span class="t-label">{p.name}</span>
            <span class="t-pill">{localizeRuntime(p.runtime_kind, lang === 'en')}</span>
            <span class="store-ver">v{p.version}</span>
          </div>
          <div class="t-desc">{p.description}</div>
          <div class="store-meta">
            <span>{p.id}</span>
            {#if p.keywords?.length}<span>· {(p.keywords || []).join(', ')}</span>{/if}
            <span>· SHA-256 {(p.sha256 || '').slice(0, 12)}…</span>
          </div>
        </div>
        <div class="store-act">
          {#if p.removable}
            <Button
              variant="danger"
              size="sm"
              onclick={() => postHost({ cmd: 'uninstall_plugin', plugin_id: p.id })}
            >
              <Trash2 size={13} />
              {lang === 'en' ? 'Uninstall' : '卸载'}
            </Button>
          {:else if p.installed}
            <span class="t-pill">{lang === 'en' ? 'Bundled' : '随包内置'}</span>
          {:else}
            <Button
              variant="primary"
              size="sm"
              onclick={() => postHost({ cmd: 'install_plugin', plugin_id: p.id, source: p.source, sha256: p.sha256 })}
            >
              <Download size={13} />
              {lang === 'en' ? 'Install' : '安装'}
            </Button>
          {/if}
        </div>
      </div>
    {/each}
  {/if}
</div>

<style>
  .tab-head-bar { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; margin-bottom: 18px; }
  .tab-head-bar h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
  .tab-lead { margin: 0; font-size: 12.5px; color: var(--muted); }
  .store-wrap { padding: 14px 16px; }
  .skel-list { display: flex; flex-direction: column; gap: 10px; }
  .store-row {
    display: flex; align-items: center; justify-content: space-between; gap: 14px;
    padding: 12px 0; border-bottom: 1px solid var(--border);
  }
  .store-row:last-child { border-bottom: 0; padding-bottom: 0; }
  .store-row:first-child { padding-top: 0; }
  .store-info { flex: 1; min-width: 0; }
  .store-title { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .store-ver { font-size: 11.5px; color: var(--muted); }
  .store-meta {
    margin-top: 4px; font-size: 11px; color: var(--muted);
    font-family: "Cascadia Mono", Consolas, monospace; display: flex; gap: 6px; flex-wrap: wrap;
  }
  .store-act { flex-shrink: 0; }
</style>
