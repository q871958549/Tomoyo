<script lang="ts">
  import { ChevronDown, ChevronUp, FileCode, FolderOpen, Layers, SlidersHorizontal } from '@lucide/svelte';
  import {
    isPluginEnabled,
    localizePermission,
    localizeRuntime,
    OFFICIAL_PLUGINS,
    PLUGIN_CAPTURE,
    PLUGIN_CLIPBOARD,
    PLUGIN_TRANSLATE,
  } from '../../lib/defaults';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { HotkeyHealthReport, PluginEntry, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import CaptureTab from './CaptureTab.svelte';
  import ClipboardPluginCard from './ClipboardPluginCard.svelte';
  import PluginConfigDrawer from './PluginConfigDrawer.svelte';
  import TranslateTab from './TranslateTab.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    plugins: PluginEntry[];
    configPath: string;
    pluginsDir: string;
    runningApps: string[];
    secretsStatus: { deepl: boolean; openai: boolean };
    hotkeyHealth: HotkeyHealthReport;
    showRecord: boolean;
    expandedId: string | null;
    onToggleExpand: (id: string | null) => void;
    onTogglePlugin: (id: string, enabled: boolean) => void;
    onUpdatePluginConfig: (
      hotkeys: Record<string, string>,
      settings: Record<string, Record<string, string | number | boolean>>
    ) => void;
    onJumpDoctor?: () => void;
    onToast: (msg: string, warn?: boolean) => void;
    onSave: (delayMs?: number) => void;
  }

  let {
    cfg, lang, plugins, configPath, pluginsDir, runningApps, secretsStatus,
    hotkeyHealth, showRecord, expandedId, onToggleExpand, onTogglePlugin,
    onUpdatePluginConfig, onJumpDoctor, onToast, onSave,
  }: Props = $props();

  let isEn = $derived(lang === 'en');
  let officialIdSet = new Set(OFFICIAL_PLUGINS.map((m) => m.id.toLowerCase()));
  let externalPlugins = $derived(plugins.filter((p) => !officialIdSet.has(p.id.toLowerCase())));

  function hasV2Config(p: PluginEntry): boolean {
    return (p.declared_hotkeys?.length || 0) > 0 || (p.settings_schema?.length || 0) > 0;
  }

  function hasOfficialDrawer(id: string, entry?: PluginEntry): boolean {
    return id === PLUGIN_CLIPBOARD || id === PLUGIN_TRANSLATE || id === PLUGIN_CAPTURE || !!entry;
  }

  function onChangeHotkey(slotKey: string, spec: string) {
    const next = { ...(cfg.plugin_hotkeys || {}) };
    if (spec.trim()) next[slotKey] = spec.trim();
    else delete next[slotKey];
    onUpdatePluginConfig(next, { ...(cfg.plugin_settings || {}) });
  }

  function onChangeSetting(pluginId: string, key: string, value: string | number | boolean) {
    const nextSettings = { ...(cfg.plugin_settings || {}) };
    nextSettings[pluginId] = { ...(nextSettings[pluginId] || {}), [key]: value };
    onUpdatePluginConfig({ ...(cfg.plugin_hotkeys || {}) }, nextSettings);
  }

  function togglePerm(p: PluginEntry, perm: string, allowed: boolean) {
    const next = new Set(p.granted_permissions || []);
    if (allowed) next.add(perm);
    else next.delete(perm);
    p.granted_permissions = [...next];
    postHost({ cmd: 'set_plugin_permission', plugin_id: p.id, permission: perm, allowed });
  }
</script>

<div class="t-card mod-card">
  <div class="sec-head">
    <Layers size={14} class="sec-ico" />
    <span>{isEn ? 'Official Feature Modules (Click Config Inside Each Plugin)' : '官方功能插件（按需启停 · 参数收口于插件内配置）'}</span>
  </div>

  {#each OFFICIAL_PLUGINS as item}
    {@const enabled = isPluginEnabled(cfg, item.id)}
    {@const entry = plugins.find((p) => p.id.toLowerCase() === item.id.toLowerCase())}
    {@const canConfig = hasOfficialDrawer(item.id, entry)}
    {@const isOpen = expandedId === item.id}
    <div class="mod-item">
      <div class="mod-row {enabled ? '' : 'is-off'}">
        <div class="mod-info">
          <div class="mod-top">
            <span class="t-label">{isEn ? item.enName : item.zhName}</span>
            <span class="t-pill">{item.triggers}</span>
            {#if entry}
              <span class="t-pill">{localizeRuntime(entry.runtime, isEn)} · {entry.max_memory_mb} MB</span>
            {/if}
            <span class="plug-id">{item.id}</span>
          </div>
          <div class="t-desc">{isEn ? item.enDesc : item.zhDesc}</div>
        </div>
        <div class="mod-acts">
          {#if canConfig}
            <button
              type="button"
              class="cfg-pill {isOpen ? 'is-open' : ''}"
              data-config-plugin={item.id}
              onclick={() => onToggleExpand(isOpen ? null : item.id)}
            >
              <SlidersHorizontal size={11} />
              <span>{isEn ? 'Settings' : '参数与设置'}</span>
              {#if isOpen}<ChevronUp size={11} />{:else}<ChevronDown size={11} />{/if}
            </button>
          {/if}
          <Switch checked={enabled} ariaLabel={isEn ? item.enName : item.zhName} onchange={(v) => onTogglePlugin(item.id, v)} />
        </div>
      </div>

      {#if canConfig && isOpen}
        <div class="mod-drawer" data-plugin-drawer={item.id}>
          {#if item.id === PLUGIN_CLIPBOARD}
            <ClipboardPluginCard {cfg} {lang} {runningApps} clipHotkeyStatus={hotkeyHealth.clipboard} {onJumpDoctor} {onSave} />
          {:else if item.id === PLUGIN_TRANSLATE}
            <TranslateTab embedded {cfg} {lang} {secretsStatus} trHotkeyStatus={hotkeyHealth.translate} {onJumpDoctor} {onToast} {onSave} />
          {:else if item.id === PLUGIN_CAPTURE}
            <CaptureTab embedded {cfg} {lang} capHotkeyStatus={hotkeyHealth.capture} pinHotkeyStatus={hotkeyHealth.pin} ocrHotkeyStatus={hotkeyHealth.ocr} {showRecord} {onJumpDoctor} {onSave} />
          {:else if entry}
            {#if (entry.declared_permissions || []).length > 0}
              <div class="perm-strip">
                {#each entry.declared_permissions as perm}
                  {@const granted = (entry.granted_permissions || []).includes(perm)}
                  <div class="perm-item">
                    <Switch checked={granted} onchange={(v) => togglePerm(entry, perm, v)} />
                    <span>{localizePermission(perm, isEn)}</span>
                  </div>
                {/each}
              </div>
            {/if}
            <PluginConfigDrawer plugin={entry} {cfg} {lang} {onChangeHotkey} {onChangeSetting} />
          {/if}
        </div>
      {/if}
    </div>
  {/each}
</div>

<div class="t-card plug-wrap" style="margin-top:14px">
  <div class="plug-grid plug-head">
    <span>{tr(lang, 'th_pname')}</span>
    <span>{tr(lang, 'th_rt')}</span>
    <span>{tr(lang, 'th_trig')}</span>
    <span>{tr(lang, 'th_perms')}</span>
    <span style="text-align:right">{tr(lang, 'th_cap')}</span>
    <span style="text-align:right">{isEn ? 'State' : '启停'}</span>
  </div>
  {#if externalPlugins.length === 0}
    <div class="empty-plug">{isEn ? 'No third-party plugins installed yet (browse Plugin Catalog above).' : '暂无第三方扩展插件（可切换上方「插件市场」浏览安装）。'}</div>
  {:else}
    {#each externalPlugins as p}
      {@const enabled = isPluginEnabled(cfg, p.id)}
      {@const configurable = hasV2Config(p)}
      {@const isOpen = expandedId === p.id}
      <div class="plug-item">
        <div class="plug-grid plug-row {enabled ? '' : 'is-off'}">
          <div class="plug-name">
            <div class="plug-title-row">
              <span class="t-label">{p.name}</span>
              {#if configurable}
                <button
                  type="button"
                  class="cfg-pill {isOpen ? 'is-open' : ''}"
                  data-config-plugin={p.id}
                  onclick={() => onToggleExpand(isOpen ? null : p.id)}
                >
                  <SlidersHorizontal size={11} />
                  <span>{isEn ? 'Config' : '参数与热键'}</span>
                  {#if isOpen}<ChevronUp size={11} />{:else}<ChevronDown size={11} />{/if}
                </button>
              {/if}
            </div>
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
        {#if configurable && isOpen}
          <PluginConfigDrawer plugin={p} {cfg} {lang} {onChangeHotkey} {onChangeSetting} />
        {/if}
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
      <FileCode size={14} />{tr(lang, 'btn_open_cfg')}
    </Button>
  </div>
  <div class="t-row">
    <div style="min-width:0">
      <div class="t-label">{tr(lang, 'lbl_plug_dir')}</div>
      <div class="path-sub">{pluginsDir}</div>
    </div>
    <Button variant="secondary" onclick={() => pluginsDir && postHost({ cmd: 'open_path', path: pluginsDir })}>
      <FolderOpen size={14} />{tr(lang, 'btn_open_plug')}
    </Button>
  </div>
</div>

<style>
  .mod-card { padding: 14px 16px; }
  .sec-head { display: flex; align-items: center; gap: 7px; font-size: 13px; font-weight: 600; margin-bottom: 6px; }
  :global(.sec-ico) { color: var(--accent); flex-shrink: 0; }
  .mod-item { border-top: 1px solid var(--border); }
  .mod-row { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 11px 0; }
  .mod-row.is-off, .plug-row.is-off { opacity: 0.56; }
  .mod-info { min-width: 0; }
  .mod-top { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .mod-acts { display: flex; align-items: center; gap: 10px; flex-shrink: 0; }
  .mod-drawer {
    margin: 2px 0 12px; padding: 12px 14px; border-radius: 10px;
    background: var(--canvas); border: 1px solid var(--border);
  }
  .perm-strip { display: flex; flex-wrap: wrap; gap: 12px; padding-bottom: 8px; margin-bottom: 8px; border-bottom: 1px solid var(--border); }
  .plug-wrap { padding: 12px 16px; }
  .plug-grid {
    display: grid;
    grid-template-columns: minmax(140px, 1.4fr) 74px minmax(64px, 0.8fr) minmax(105px, 1fr) 58px 46px;
    gap: 10px; align-items: center;
  }
  .plug-head { font-size: 11.5px; font-weight: 600; color: var(--muted); padding-bottom: 8px; border-bottom: 1px solid var(--border); }
  .plug-item { border-bottom: 1px solid var(--border); }
  .plug-item:last-child { border-bottom: 0; }
  .plug-row { padding: 10px 0; font-size: 12.5px; }
  .plug-title-row { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .cfg-pill {
    display: inline-flex; align-items: center; gap: 4px; padding: 3px 9px; border-radius: 999px;
    border: 1px solid var(--border-strong); background: var(--surface); color: var(--text-secondary);
    font-family: inherit; font-size: 11.5px; font-weight: 500; cursor: pointer; white-space: nowrap;
    transition: border-color 0.12s ease, color 0.12s ease, background-color 0.12s ease;
  }
  .cfg-pill:hover, .cfg-pill.is-open { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .empty-plug { padding: 12px 0 4px; font-size: 12px; color: var(--muted); }
  .plug-id { font-size: 11px; color: var(--muted); font-family: "Cascadia Mono", Consolas, monospace; }
  .plug-kw { font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; color: var(--text-secondary); }
  .plug-perms { display: flex; flex-direction: column; gap: 5px; }
  .perm-item { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--text-secondary); }
  .plug-mem { text-align: right; font-variant-numeric: tabular-nums; color: var(--text-secondary); white-space: nowrap; }
  .plug-sw { display: flex; justify-content: flex-end; }
  .path-sub { margin-top: 2px; font-size: 12px; color: var(--muted); font-family: "Cascadia Mono", Consolas, monospace; word-break: break-all; }
</style>
