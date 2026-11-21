<script lang="ts">
  import { FileCode, FolderOpen, Layers } from '@lucide/svelte';
  import {
    isPluginEnabled,
    localizePermission,
    localizeRuntime,
    OFFICIAL_PLUGINS,
  } from '../../lib/defaults';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { PluginEntry, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    plugins: PluginEntry[];
    configPath: string;
    pluginsDir: string;
    onTogglePlugin: (id: string, enabled: boolean) => void;
  }

  let { cfg, lang, plugins, configPath, pluginsDir, onTogglePlugin }: Props = $props();
  let isEn = $derived(lang === 'en');

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

<!-- Official Modular Plugins -->
<div class="t-card mod-card">
  <div class="sec-head">
    <Layers size={14} class="sec-ico" />
    <span>{isEn ? 'Official Feature Modules (Optional · Microkernel Design)' : '官方功能插件（按需启停 · 坚守微内核初衷）'}</span>
  </div>
  {#each OFFICIAL_PLUGINS as item}
    {@const enabled = isPluginEnabled(cfg, item.id)}
    <div class="mod-row">
      <div class="mod-info">
        <div class="mod-top">
          <span class="t-label">{isEn ? item.enName : item.zhName}</span>
          <span class="t-pill">{item.triggers}</span>
          <span class="plug-id">{item.id}</span>
        </div>
        <div class="t-desc">{isEn ? item.enDesc : item.zhDesc}</div>
      </div>
      <Switch
        checked={enabled}
        ariaLabel={isEn ? item.enName : item.zhName}
        onchange={(v) => onTogglePlugin(item.id, v)}
      />
    </div>
  {/each}
  <div class="mod-note">
    {isEn
      ? 'Pure launcher core: future Notes (dev.tomoyo.notes) will also ship exclusively as an optional installable plugin — never forced into tomoyo.exe.'
      : '坚守初衷：规划中的「本地备忘录 (dev.tomoyo.notes)」亦将作为独立可选插件按需安装，永不强制膨胀 Tomoyo 主进程。'}
  </div>
</div>

<!-- Installed External / Sandbox Plugins -->
<div class="t-card plug-wrap" style="margin-top:14px">
  <div class="plug-grid plug-head">
    <span>{tr(lang, 'th_pname')}</span>
    <span>{tr(lang, 'th_rt')}</span>
    <span>{tr(lang, 'th_trig')}</span>
    <span>{tr(lang, 'th_perms')}</span>
    <span style="text-align:right">{tr(lang, 'th_cap')}</span>
    <span style="text-align:right">{isEn ? 'State' : '启停'}</span>
  </div>
  {#if plugins.length === 0}
    <div class="empty-plug">{isEn ? 'No external plugins installed yet.' : '当前尚未安装额外外部插件（可在「插件清单」按需安装）。'}</div>
  {:else}
    {#each plugins as p}
      {@const enabled = isPluginEnabled(cfg, p.id)}
      <div class="plug-grid plug-row {enabled ? '' : 'is-off'}">
        <div class="plug-name">
          <div class="t-label">{p.name}</div>
          <div class="plug-id">{p.id}</div>
        </div>
        <div><span class="t-pill">{localizeRuntime(p.runtime, isEn)}</span></div>
        <div class="plug-kw">{(p.keywords || []).join(', ')}</div>
        <div class="plug-perms">
          {#if (p.declared_permissions || []).length === 0}
            <span class="t-desc">{tr(lang, 'perm_none')}</span>
          {:else}
            {#each p.declared_permissions as perm}
              {@const granted = (p.granted_permissions || []).includes(perm)}
              <div class="perm-item">
                <Switch checked={granted} onchange={(v) => togglePerm(p, perm, v)} />
                <span>{localizePermission(perm, isEn)}</span>
              </div>
            {/each}
          {/if}
        </div>
        <div class="plug-mem">{p.max_memory_mb} MB</div>
        <div class="plug-sw">
          <Switch checked={enabled} ariaLabel={p.name} onchange={(v) => onTogglePlugin(p.id, v)} />
        </div>
      </div>
    {/each}
  {/if}
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
  .mod-card { padding: 14px 16px; }
  .sec-head { display: flex; align-items: center; gap: 7px; font-size: 13px; font-weight: 600; margin-bottom: 8px; }
  :global(.sec-ico) { color: var(--accent); flex-shrink: 0; }
  .mod-row {
    display: flex; align-items: center; justify-content: space-between; gap: 14px;
    padding: 10px 0; border-top: 1px solid var(--border);
  }
  .mod-info { min-width: 0; }
  .mod-top { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .mod-note {
    margin-top: 8px; padding: 8px 10px; border-radius: 7px; background: var(--canvas);
    border: 1px dashed var(--border-strong); font-size: 11.5px; color: var(--muted);
  }
  .plug-wrap { padding: 12px 16px; }
  .plug-grid {
    display: grid;
    grid-template-columns: minmax(140px, 1.4fr) 74px minmax(64px, 0.8fr) minmax(105px, 1fr) 58px 46px;
    gap: 10px; align-items: center;
  }
  .plug-head {
    font-size: 11.5px; font-weight: 600; color: var(--muted);
    padding-bottom: 8px; border-bottom: 1px solid var(--border);
  }
  .plug-row { padding: 10px 0; border-bottom: 1px solid var(--border); font-size: 12.5px; }
  .plug-row.is-off { opacity: 0.56; }
  .plug-row:last-child { border-bottom: 0; padding-bottom: 2px; }
  .empty-plug { padding: 12px 0 4px; font-size: 12px; color: var(--muted); }
  .plug-id { font-size: 11px; color: var(--muted); font-family: "Cascadia Mono", Consolas, monospace; }
  .plug-kw { font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; color: var(--text-secondary); }
  .plug-perms { display: flex; flex-direction: column; gap: 5px; }
  .perm-item { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--text-secondary); }
  .plug-mem { text-align: right; font-variant-numeric: tabular-nums; color: var(--text-secondary); white-space: nowrap; }
  .plug-sw { display: flex; justify-content: flex-end; }
  .path-sub {
    margin-top: 2px; font-size: 12px; color: var(--muted);
    font-family: "Cascadia Mono", Consolas, monospace; word-break: break-all;
  }
</style>
