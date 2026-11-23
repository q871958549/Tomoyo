<script lang="ts">
  import { Keyboard, Package, RefreshCw, Search, SlidersHorizontal, Workflow } from '@lucide/svelte';
  import tomoyoLogo from '../../assets/tomoyo-logo.png';
  import { searchSettingsEntries, type SettingSearchEntry } from '../../lib/settingsSearch';
  import { tr, type Lang } from '../../lib/i18n';
  import Input from '../../lib/ui/Input.svelte';

  export type SettingsTabId = 'core' | 'search' | 'hotkeys' | 'flows' | 'plugins' | 'system';

  interface Props {
    lang: Lang;
    activeTab: SettingsTabId;
    conflictCount: number;
    onSelectTab: (tab: SettingsTabId) => void;
    onApplyHit: (hit: SettingSearchEntry) => void;
  }

  let { lang, activeTab, conflictCount, onSelectTab, onApplyHit }: Props = $props();
  let quickQuery = $state('');
  let searchHits = $derived(searchSettingsEntries(quickQuery));

  const navItems = [
    { id: 'core' as const, key: 'nav_core', icon: SlidersHorizontal },
    { id: 'search' as const, key: 'nav_search', icon: Search },
    { id: 'hotkeys' as const, key: 'nav_hotkeys', icon: Keyboard },
    { id: 'flows' as const, key: 'nav_flows', icon: Workflow },
    { id: 'plugins' as const, key: 'nav_plugins', icon: Package },
    { id: 'system' as const, key: 'nav_system', icon: RefreshCw },
  ];

  function pickHit(hit: SettingSearchEntry) {
    quickQuery = '';
    onApplyHit(hit);
  }
</script>

<aside class="sidebar">
  <div class="brand">
    <img class="brand-logo" src={tomoyoLogo} alt="Tomoyo" width="32" height="32" draggable="false" />
    <div>
      <div class="brand-name">Tomoyo</div>
      <div class="brand-sub">{tr(lang, 'brand_sub')}</div>
    </div>
  </div>

  <div class="side-search">
    <Input bind:value={quickQuery} type="search" size="sm" placeholder={tr(lang, 'search_settings_ph')} />
    {#if searchHits.length > 0}
      <div class="hit-pop">
        {#each searchHits as hit}
          <button type="button" class="hit-item" onclick={() => pickHit(hit)}>
            {lang === 'en' ? hit.en : hit.zh}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <nav class="nav">
    {#each navItems as item}
      {@const Icon = item.icon}
      <button type="button" data-tab={item.id} class="nav-item {activeTab === item.id ? 'is-active' : ''}" onclick={() => onSelectTab(item.id)}>
        <span class="nav-ico"><Icon size={15} /></span>
        <span>{tr(lang, item.key)}</span>
        {#if item.id === 'hotkeys' && conflictCount > 0}
          <span class="nav-badge">{conflictCount}</span>
        {/if}
      </button>
    {/each}
  </nav>

  <div class="side-foot">{tr(lang, 'sidebar_foot')}</div>
</aside>

<style>
  .sidebar { width: 218px; flex-shrink: 0; background: var(--sidebar); border-right: 1px solid var(--border); display: flex; flex-direction: column; padding: 14px 10px; }
  .brand { display: flex; align-items: center; gap: 10px; padding: 2px 8px 12px; border-bottom: 1px solid var(--border); margin-bottom: 10px; }
  .brand-logo { width: 32px; height: 32px; border-radius: 8px; display: block; flex-shrink: 0; filter: drop-shadow(0 1px 2px rgba(15, 23, 42, 0.14)); }
  .brand-name { font-size: 14.5px; font-weight: 600; }
  .brand-sub { font-size: 11px; color: var(--muted); }
  .side-search { position: relative; margin-bottom: 10px; }
  .hit-pop { position: absolute; left: 0; right: 0; top: 38px; z-index: 20; background: var(--surface); border: 1px solid var(--border-strong); border-radius: 8px; box-shadow: 0 8px 20px rgba(0,0,0,0.16); padding: 4px; display: flex; flex-direction: column; gap: 2px; }
  .hit-item { border: 0; background: transparent; text-align: left; padding: 6px 8px; border-radius: 6px; font-family: inherit; font-size: 12px; color: var(--text); cursor: pointer; }
  .hit-item:hover { background: var(--accent-soft); color: var(--accent); }
  .nav { display: flex; flex-direction: column; gap: 3px; overflow-y: auto; }
  .nav-item { position: relative; display: flex; align-items: center; gap: 9px; width: 100%; height: 35px; padding: 0 10px; border: 0; border-radius: 7px; background: transparent; color: var(--text-secondary); font-family: inherit; font-size: 13px; font-weight: 500; text-align: left; white-space: nowrap; cursor: pointer; transition: background-color 0.12s ease, color 0.12s ease; }
  .nav-item:hover:not(.is-active) { background: var(--hover); color: var(--text); }
  .nav-item.is-active { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .nav-item.is-active::before { content: ""; position: absolute; left: 0; top: 7px; bottom: 7px; width: 3px; border-radius: 999px; background: var(--accent); }
  .nav-ico { display: inline-flex; opacity: 0.85; flex-shrink: 0; }
  .nav-badge { margin-left: auto; min-width: 17px; height: 17px; padding: 0 5px; border-radius: 999px; background: var(--danger); color: #fff; font-size: 10.5px; font-weight: 600; display: inline-flex; align-items: center; justify-content: center; }
  .side-foot { margin-top: auto; padding: 10px 8px 0; border-top: 1px solid var(--border); font-size: 11px; color: var(--muted); line-height: 1.5; }
</style>
