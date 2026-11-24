<script lang="ts">
  import { ChevronDown, ChevronUp, FileCode, FolderOpen, Layers, ShieldCheck, SlidersHorizontal } from '@lucide/svelte';
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
  import Input from '../../lib/ui/Input.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import CaptureTab from './CaptureTab.svelte';
  import ClipboardPluginCard from './ClipboardPluginCard.svelte';
  import PluginConfigDrawer from './PluginConfigDrawer.svelte';
  import TranslateTab from './TranslateTab.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    plugins: PluginEntry[];
    officialIcons?: Record<string, string>;
    configPath: string;
    pluginsDir: string;
    runningApps: string[];
    secretsStatus: { deepl: boolean; openai: boolean; plugins?: Record<string, boolean> };
    hotkeyHealth: HotkeyHealthReport;
    showRecord: boolean;
    expandedId: string | null;
    healthReport?: { summary?: { total: number; healthy: number; warnings: number; updates: number; mirrors_ok: number; mirrors_total: number }; mirrors?: Array<{ source: string; ok: boolean; latency_ms: number; detail: string }> } | null;
    checkingHealth?: boolean;
    onCheckHealth?: () => void;
    onToggleExpand: (id: string | null) => void;
    onTogglePlugin: (id: string, enabled: boolean) => void;
    onUpdatePluginConfig: (
      hotkeys: Record<string, string>,
      settings: Record<string, Record<string, string | number | boolean>>,
      displayModes?: Record<string, string>
    ) => void;
    onJumpDoctor?: () => void;
    onToast: (msg: string, warn?: boolean) => void;
    onSave: (delayMs?: number) => void;
  }

  let {
    cfg, lang, plugins, officialIcons = {},
    configPath, pluginsDir, runningApps, secretsStatus, hotkeyHealth, showRecord, expandedId,
    healthReport = null, checkingHealth = false, onCheckHealth,
    onToggleExpand, onTogglePlugin, onUpdatePluginConfig, onJumpDoctor, onToast, onSave,
  }: Props = $props();

  let filterText = $state('');
  let filterCat = $state('all');
  let isEn = $derived(lang === 'en');
  let officialIdSet = new Set(OFFICIAL_PLUGINS.map((m) => m.id.toLowerCase()));

  let filteredOfficials = $derived.by(() => {
    const q = filterText.trim().toLowerCase();
    return OFFICIAL_PLUGINS.filter((m) => {
      if (filterCat !== 'all' && m.category !== filterCat) return false;
      if (!q) return true;
      return `${m.id} ${m.zhName} ${m.enName} ${m.triggers} ${m.zhDesc}`.toLowerCase().includes(q);
    });
  });

  let filteredExternals = $derived.by(() => {
    const q = filterText.trim().toLowerCase();
    return plugins.filter((p) => {
      if (officialIdSet.has(p.id.toLowerCase())) return false;
      if (filterCat !== 'all' && (p.category || 'dev') !== filterCat) return false;
      if (!q) return true;
      return `${p.id} ${p.name} ${(p.keywords || []).join(' ')} ${p.description || ''}`.toLowerCase().includes(q);
    });
  });

  function hasV2Config(p: PluginEntry): boolean {
    return (p.declared_hotkeys?.length || 0) > 0 || (p.settings_schema?.length || 0) > 0 || !!p.display_mode;
  }

  function hasOfficialDrawer(id: string, entry?: PluginEntry): boolean {
    return id === PLUGIN_CLIPBOARD || id === PLUGIN_TRANSLATE || id === PLUGIN_CAPTURE || !!entry;
  }

  function resolveIcon(id: string, entry?: PluginEntry): string {
    return entry?.icon_url || officialIcons[id.toLowerCase()] || officialIcons[id] || '';
  }

  function modeBadge(id: string, entry?: PluginEntry): string {
    const ov = (cfg.plugin_display_modes || {})[id.toLowerCase()];
    const eff = ov || entry?.display_mode || '';
    if (eff === 'inline') return isEn ? 'Inline' : '内嵌';
    if (eff === 'window') return isEn ? 'Window' : '独立窗';
    return '';
  }

  function onChangeDisplayMode(pluginId: string, mode: string) {
    const nextModes = { ...(cfg.plugin_display_modes || {}) };
    if (mode === 'inline' || mode === 'window') nextModes[pluginId] = mode;
    else delete nextModes[pluginId];
    onUpdatePluginConfig({ ...(cfg.plugin_hotkeys || {}) }, { ...(cfg.plugin_settings || {}) }, nextModes);
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

<div class="t-card mod-card" style="margin-bottom:12px">
  <div class="filter-bar">
    <div style="flex:1;min-width:180px">
      <Input bind:value={filterText} size="sm" type="search" placeholder={isEn ? 'Filter installed plugins by name, id, trigger…' : '检索已安装插件（名称 / ID / 触发词）…'} />
    </div>
    <Select
      size="sm"
      minWidth="136px"
      value={filterCat}
      options={[
        { value: 'all', label: isEn ? 'All Categories' : '全部分类' },
        { value: 'productivity', label: isEn ? 'Productivity' : '效率工具' },
        { value: 'docs', label: isEn ? 'Docs & Notes' : '文档与笔记' },
        { value: 'ai', label: isEn ? 'AI & OCR' : 'AI 与识字' },
        { value: 'media', label: isEn ? 'Capture & Media' : '截图与图像' },
        { value: 'dev', label: isEn ? 'Developer' : '开发调试' },
      ]}
      onchange={(v) => (filterCat = v)}
    />
    <Button variant="secondary" size="sm" disabled={checkingHealth} onclick={() => onCheckHealth?.()}>
      <ShieldCheck size={13} />{checkingHealth ? (isEn ? 'Checking…' : '体检中…') : (isEn ? 'Health Check' : '一键体检')}
    </Button>
  </div>
  {#if healthReport?.summary}
    {@const s = healthReport.summary}
    <div class="health-bar">
      <span class="t-pill {s.warnings === 0 ? 'upd-btn' : ''}">{isEn ? `${s.healthy}/${s.total} healthy` : `插件健康 ${s.healthy}/${s.total}`}</span>
      {#if s.updates > 0}<span class="t-pill upd-btn">{isEn ? `${s.updates} update(s)` : `${s.updates} 项可升级`}</span>{/if}
      {#each healthReport.mirrors || [] as m}
        <span class="t-pill">{m.ok ? '✓' : '✗'} {m.detail} ({m.latency_ms}ms)</span>
      {/each}
    </div>
  {/if}
</div>

<div class="t-card mod-card">
  <div class="sec-head">
    <Layers size={14} class="sec-ico" />
    <span>{isEn ? 'Built-In Official Feature Plugins (Opt-Out Ready · Phase 3 Externalization)' : '官方功能插件（内置可开关 · 其余 OCR / 备忘录 / 编辑器均按需安装）'}</span>
  </div>

  {#if filteredOfficials.length === 0}
    <div class="empty-plug">{isEn ? 'No matching official plugins.' : '当前筛选条件下无匹配官方插件。'}</div>
  {/if}
  {#each filteredOfficials as item}
    {@const enabled = isPluginEnabled(cfg, item.id)}
    {@const entry = plugins.find((p) => p.id.toLowerCase() === item.id.toLowerCase())}
    {@const iconSrc = resolveIcon(item.id, entry)}
    {@const canConfig = hasOfficialDrawer(item.id, entry)}
    {@const isOpen = expandedId === item.id}
    {@const mTag = modeBadge(item.id, entry)}
    <div class="mod-item">
      <div class="mod-row {enabled ? '' : 'is-off'}">
        <div class="plug-avatar">
          {#if iconSrc}
            <img src={iconSrc} alt={item.id} width="32" height="32" />
          {:else}
            <span>{item.zhName.slice(0, 1)}</span>
          {/if}
        </div>
        <div class="mod-info" style="flex:1">
          <div class="mod-top">
            <span class="t-label">{isEn ? item.enName : item.zhName}</span>
            <span class="t-pill">{item.triggers}</span>
            {#if mTag}<span class="t-pill">{mTag}</span>{/if}
            {#if entry}
              <span class="t-pill">{localizeRuntime(entry.runtime, isEn)} · {entry.max_memory_mb} MB</span>
            {/if}
            <span class="plug-id">{item.id}</span>
          </div>
          <div class="t-desc">{isEn ? item.enDesc : item.zhDesc}</div>
        </div>
        <div class="mod-acts">
          {#if entry?.update_available && entry.update_source && entry.update_sha256}
            <button type="button" class="cfg-pill upd-btn" onclick={() => postHost({ cmd: 'install_plugin', plugin_id: entry.id, source: entry.update_source!, sha256: entry.update_sha256! })}>
              ↑ v{entry.latest_version}
            </button>
          {/if}
          {#if canConfig}
            <button type="button" class="cfg-pill {isOpen ? 'is-open' : ''}" data-config-plugin={item.id} onclick={() => onToggleExpand(isOpen ? null : item.id)}>
              <SlidersHorizontal size={11} />
              <span>{isEn ? 'Settings' : '配置'}</span>
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
            <CaptureTab embedded {cfg} {lang} capHotkeyStatus={hotkeyHealth.capture} pinHotkeyStatus={hotkeyHealth.pin} ocrHotkeyStatus={hotkeyHealth.ocr} recordHotkeyStatus={hotkeyHealth.record} {showRecord} {onJumpDoctor} {onSave} />
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
            <PluginConfigDrawer plugin={entry} {cfg} {lang} pluginSecrets={secretsStatus.plugins || {}} {onChangeDisplayMode} {onChangeHotkey} {onChangeSetting} />
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
  {#if filteredExternals.length === 0}
    <div class="empty-plug">{isEn ? 'No matching third-party plugins (browse Plugin Catalog).' : '暂无匹配的扩展插件（可在上方「插件市场」按分类搜索安装）。'}</div>
  {:else}
    {#each filteredExternals as p}
      {@const enabled = isPluginEnabled(cfg, p.id)}
      {@const configurable = hasV2Config(p)}
      {@const isOpen = expandedId === p.id}
      {@const extIcon = resolveIcon(p.id, p)}
      {@const mTag = modeBadge(p.id, p)}
      <div class="plug-item">
        <div class="plug-grid plug-row {enabled ? '' : 'is-off'}">
          <div class="plug-name" style="display:flex;align-items:center;gap:9px">
            <div class="plug-avatar sm">
              {#if extIcon}
                <img src={extIcon} alt={p.id} width="26" height="26" />
              {:else}
                <span>{p.name.slice(0, 1)}</span>
              {/if}
            </div>
            <div style="min-width:0;flex:1">
              <div class="plug-title-row">
                <span class="t-label">{p.name}</span>
                {#if mTag}<span class="t-pill">{mTag}</span>{/if}
                {#if p.update_available && p.update_source && p.update_sha256}
                  <button type="button" class="cfg-pill upd-btn" onclick={() => postHost({ cmd: 'install_plugin', plugin_id: p.id, source: p.update_source!, sha256: p.update_sha256! })}>
                    ↑ v{p.latest_version}
                  </button>
                {/if}
                {#if configurable}
                  <button type="button" class="cfg-pill {isOpen ? 'is-open' : ''}" data-config-plugin={p.id} onclick={() => onToggleExpand(isOpen ? null : p.id)}>
                    <SlidersHorizontal size={11} />
                    <span>{isEn ? 'Config' : '配置'}</span>
                    {#if isOpen}<ChevronUp size={11} />{:else}<ChevronDown size={11} />{/if}
                  </button>
                {/if}
              </div>
              <div class="plug-id">{p.id}</div>
            </div>
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
          <PluginConfigDrawer plugin={p} {cfg} {lang} pluginSecrets={secretsStatus.plugins || {}} {onChangeDisplayMode} {onChangeHotkey} {onChangeSetting} />
        {/if}
      </div>
    {/each}
  {/if}
</div>

<div class="t-card" style="margin-top:14px">
  <div class="t-row">
    <div style="min-width:0"><div class="t-label">{tr(lang, 'lbl_cfg_path')}</div><div class="path-sub">{configPath}</div></div>
    <Button variant="secondary" onclick={() => configPath && postHost({ cmd: 'open_path', path: configPath })}><FileCode size={14} />{tr(lang, 'btn_open_cfg')}</Button>
  </div>
  <div class="t-row">
    <div style="min-width:0"><div class="t-label">{tr(lang, 'lbl_plug_dir')}</div><div class="path-sub">{pluginsDir}</div></div>
    <Button variant="secondary" onclick={() => pluginsDir && postHost({ cmd: 'open_path', path: pluginsDir })}><FolderOpen size={14} />{tr(lang, 'btn_open_plug')}</Button>
  </div>
</div>

<style>
  .mod-card { padding: 12px 16px; }
  .filter-bar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .sec-head { display: flex; align-items: center; gap: 7px; font-size: 13px; font-weight: 600; margin-bottom: 6px; }
  :global(.sec-ico) { color: var(--accent); flex-shrink: 0; }
  .mod-item { border-top: 1px solid var(--border); }
  .mod-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 10px 0; }
  .mod-row.is-off, .plug-row.is-off { opacity: 0.56; }
  .plug-avatar {
    width: 32px; height: 32px; border-radius: 8px; flex-shrink: 0; overflow: hidden;
    display: inline-flex; align-items: center; justify-content: center;
    background: var(--accent-soft); color: var(--accent); font-weight: 700; font-size: 13px;
  }
  .plug-avatar.sm { width: 26px; height: 26px; border-radius: 7px; font-size: 11px; }
  .plug-avatar img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .mod-info { min-width: 0; }
  .mod-top { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .mod-acts { display: flex; align-items: center; gap: 10px; flex-shrink: 0; }
  .mod-drawer { margin: 2px 0 12px; padding: 12px 14px; border-radius: 10px; background: var(--canvas); border: 1px solid var(--border); }
  .perm-strip { display: flex; flex-wrap: wrap; gap: 12px; padding-bottom: 8px; margin-bottom: 8px; border-bottom: 1px solid var(--border); }
  .plug-wrap { padding: 12px 16px; }
  .plug-grid { display: grid; grid-template-columns: minmax(150px, 1.45fr) 70px minmax(64px, 0.75fr) minmax(105px, 1fr) 56px 46px; gap: 10px; align-items: center; }
  .plug-head { font-size: 11.5px; font-weight: 600; color: var(--muted); padding-bottom: 8px; border-bottom: 1px solid var(--border); }
  .plug-item { border-bottom: 1px solid var(--border); }
  .plug-item:last-child { border-bottom: 0; }
  .plug-row { padding: 10px 0; font-size: 12.5px; }
  .plug-title-row { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .cfg-pill {
    display: inline-flex; align-items: center; gap: 4px; padding: 3px 9px; border-radius: 999px;
    border: 1px solid var(--border-strong); background: var(--surface); color: var(--text-secondary);
    font-family: inherit; font-size: 11.5px; font-weight: 500; cursor: pointer; white-space: nowrap;
  }
  .cfg-pill:hover, .cfg-pill.is-open { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .upd-btn { border-color: var(--ok); color: var(--ok); background: var(--ok-soft); }
  .health-bar { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 10px; padding-top: 10px; border-top: 1px solid var(--border); }
  .empty-plug { padding: 12px 0 4px; font-size: 12px; color: var(--muted); }
  .plug-id { font-size: 11px; color: var(--muted); font-family: "Cascadia Mono", Consolas, monospace; }
  .plug-kw { font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; color: var(--text-secondary); }
  .plug-perms { display: flex; flex-direction: column; gap: 5px; }
  .perm-item { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--text-secondary); }
  .plug-mem { text-align: right; font-variant-numeric: tabular-nums; color: var(--text-secondary); white-space: nowrap; }
  .plug-sw { display: flex; justify-content: flex-end; }
  .path-sub { margin-top: 2px; font-size: 12px; color: var(--muted); font-family: "Cascadia Mono", Consolas, monospace; word-break: break-all; }
</style>
