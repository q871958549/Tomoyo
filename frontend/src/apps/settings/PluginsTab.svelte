<script lang="ts">
  import { FileCode, FolderOpen } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { PluginEntry } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    lang: Lang;
    plugins: PluginEntry[];
    configPath: string;
    pluginsDir: string;
  }

  let { lang, plugins, configPath, pluginsDir }: Props = $props();

  function togglePerm(p: PluginEntry, perm: string, allowed: boolean) {
    const next = new Set(p.granted_permissions || []);
    if (allowed) next.add(perm);
    else next.delete(perm);
    p.granted_permissions = [...next];
    postHost({ cmd: 'set_plugin_permission', plugin_id: p.id, permission: perm, allowed });
  }
</script>

<div class="tab-head">
  <h2>{tr(lang, 'h_plugins')}</h2>
  <p class="tab-lead">{tr(lang, 'lead_plugins')}</p>
</div>

<div class="t-card plug-wrap">
  <div class="plug-grid plug-head">
    <span>{tr(lang, 'th_pname')}</span>
    <span>{tr(lang, 'th_rt')}</span>
    <span>{tr(lang, 'th_trig')}</span>
    <span>{tr(lang, 'th_perms')}</span>
    <span style="text-align:right">{tr(lang, 'th_cap')}</span>
  </div>
  {#each plugins as p}
    <div class="plug-grid plug-row">
      <div class="plug-name">
        <div class="t-label">{p.name}</div>
        <div class="plug-id">{p.id}</div>
      </div>
      <div><span class="t-pill">{p.runtime}</span></div>
      <div class="plug-kw">{(p.keywords || []).join(', ')}</div>
      <div class="plug-perms">
        {#if (p.declared_permissions || []).length === 0}
          <span class="t-desc">{tr(lang, 'perm_none')}</span>
        {:else}
          {#each p.declared_permissions as perm}
            {@const granted = (p.granted_permissions || []).includes(perm)}
            <div class="perm-item">
              <Switch checked={granted} onchange={(v) => togglePerm(p, perm, v)} />
              <span>{perm}</span>
            </div>
          {/each}
        {/if}
      </div>
      <div class="plug-mem">{p.max_memory_mb} MB</div>
    </div>
  {/each}
</div>

<div class="t-card" style="margin-top:14px">
  <div class="t-row">
    <div style="min-width:0">
      <div class="t-label">{tr(lang, 'lbl_cfg_path')}</div>
      <div class="path-sub">{configPath}</div>
    </div>
    <Button variant="secondary" onclick={() => configPath && postHost({ cmd: 'open_path', path: configPath })}>
      <FileCode size={14} />
      {tr(lang, 'btn_open_cfg')}
    </Button>
  </div>
  <div class="t-row">
    <div style="min-width:0">
      <div class="t-label">{tr(lang, 'lbl_plug_dir')}</div>
      <div class="path-sub">{pluginsDir}</div>
    </div>
    <Button variant="secondary" onclick={() => pluginsDir && postHost({ cmd: 'open_path', path: pluginsDir })}>
      <FolderOpen size={14} />
      {tr(lang, 'btn_open_plug')}
    </Button>
  </div>
</div>

<style>
  .tab-head { margin-bottom: 18px; }
  .tab-head h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
  .tab-lead { margin: 0; font-size: 12.5px; color: var(--muted); }
  .plug-wrap { padding: 12px 16px; }
  .plug-grid {
    display: grid;
    grid-template-columns: minmax(150px, 1.5fr) 78px minmax(80px, 0.9fr) minmax(110px, 1fr) 68px;
    gap: 10px;
    align-items: center;
  }
  .plug-head {
    font-size: 11.5px; font-weight: 600; color: var(--muted);
    padding-bottom: 8px; border-bottom: 1px solid var(--border);
  }
  .plug-row { padding: 10px 0; border-bottom: 1px solid var(--border); font-size: 12.5px; }
  .plug-row:last-child { border-bottom: 0; padding-bottom: 2px; }
  .plug-id { font-size: 11px; color: var(--muted); font-family: "Cascadia Mono", Consolas, monospace; }
  .plug-kw { font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; color: var(--text-secondary); }
  .plug-perms { display: flex; flex-direction: column; gap: 5px; }
  .perm-item { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--text-secondary); }
  .plug-mem { text-align: right; font-variant-numeric: tabular-nums; color: var(--text-secondary); white-space: nowrap; }
  .path-sub {
    margin-top: 2px; font-size: 12px; color: var(--muted);
    font-family: "Cascadia Mono", Consolas, monospace; word-break: break-all;
  }
</style>
