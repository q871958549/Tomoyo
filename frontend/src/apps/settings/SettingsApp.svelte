<script lang="ts">
  import { onMount } from 'svelte';
  import { hydrateSettingsInit, isPluginEnabled, PLUGIN_CAPTURE, PLUGIN_CLIPBOARD, PLUGIN_TRANSLATE } from '../../lib/defaults';
  import { assignCoreHotkeyWithSwap, type CoreHotkeySlot } from '../../lib/hotkeys';
  import { applyThemeClass, installErrorTelemetry, postHost, subscribeHost } from '../../lib/ipc';
  import { OFFICIAL_REPO, tr, type Lang } from '../../lib/i18n';
  import type { SettingSearchEntry } from '../../lib/settingsSearch';
  import type {
    CustomFilterTab, HotkeyAuditReport, HotkeyHealthReport, LiveProbeResult,
    ReleaseInfo, TomoyoConfig, UpdateStatus, WorkflowDef,
  } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import AppearanceTab from './AppearanceTab.svelte';
  import FlowsTab from './FlowsTab.svelte';
  import GeneralTab from './GeneralTab.svelte';
  import HotkeysTab from './HotkeysTab.svelte';
  import LogsTab from './LogsTab.svelte';
  import PluginsTab from './PluginsTab.svelte';
  import SearchClipTab from './SearchClipTab.svelte';
  import SettingsSidebar, { type SettingsTabId } from './SettingsSidebar.svelte';
  import StoreTab, { type StoreItem, type StorePageMeta, type StoreQueryParams } from './StoreTab.svelte';
  import UpdatesTab from './UpdatesTab.svelte';
  import WebSearchTab from './WebSearchTab.svelte';

  const init = hydrateSettingsInit();

  let cfg = $state<TomoyoConfig>(init.config);
  let autostart = $state(init.autostart);
  let syncDir = $state(init.sync_dir || '');
  let configPath = $state(init.config_path || '');
  let plugins = $state(init.plugins);
  let flows = $state<WorkflowDef[]>(init.flows || []);
  let secretsStatus = $state(init.secrets_status);
  let hotkeyHealth = $state<HotkeyHealthReport>(init.hotkey_health || {});
  let activeTab = $state<SettingsTabId>('core');
  let subCore = $state('general');
  let subSearch = $state('indexing');
  let subPlugins = $state('installed');
  let subSystem = $state('updates');
  let expandedPlugin = $state<string | null>(null);
  let toastMsg = $state('');
  let toastWarn = $state(false);
  let folderErr = $state('');

  let hotkeyAudit = $state<HotkeyAuditReport | null>(null);
  let auditingHotkeys = $state(false);
  let probingSpec = $state('');
  let probeResult = $state<LiveProbeResult | null>(null);

  let storeLoaded = $state(false);
  let storeLoading = $state(false);
  let storeItems = $state<StoreItem[]>([]);
  let storeMeta = $state<StorePageMeta>({ total: 0, catalogTotal: 0, page: 1, totalPages: 1, categoryCounts: {} });
  let storeStatus = $state('');
  let logSource = $state('tomoyo.log');
  let logErrorsOnly = $state(false);
  let logContent = $state('');
  let logMeta = $state('');
  let updateStatus = $state<UpdateStatus | null>(init.cached_update ?? null);
  let checkingUpdates = $state(false);
  let updatePulse = $state(false);
  let updatesCheckedOnce = $state(false);
  let checkStartedAt = 0;
  let manualUpdateCheck = false;
  let checkTimer: ReturnType<typeof setTimeout> | null = null;
  let installProgress = $state<{ percent: number; message: string; done?: boolean; ok?: boolean } | null>(null);

  let lang = $derived<Lang>(cfg.language === 'en' ? 'en' : 'zh');
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  let conflictSlots = $derived.by(() => {
    const capOn = isPluginEnabled(cfg, PLUGIN_CAPTURE);
    const entries = [
      { label: lang === 'en' ? 'Launcher' : '主启动器', st: hotkeyHealth.main, on: true },
      { label: lang === 'en' ? 'Clipboard' : '剪贴板', st: hotkeyHealth.clipboard, on: isPluginEnabled(cfg, PLUGIN_CLIPBOARD) },
      { label: lang === 'en' ? 'Translate' : '划词翻译', st: hotkeyHealth.translate, on: isPluginEnabled(cfg, PLUGIN_TRANSLATE) },
      { label: lang === 'en' ? 'Capture' : '截图', st: hotkeyHealth.capture, on: capOn },
      { label: lang === 'en' ? 'Pin' : '全局贴图', st: hotkeyHealth.pin, on: capOn },
      { label: lang === 'en' ? 'Screenshot OCR' : '截图识字', st: hotkeyHealth.ocr, on: capOn },
    ];
    return entries.filter((e) => e.on && e.st && !e.st.disabled && !e.st.ok && e.st.spec);
  });

  function jumpToHotkeyConflict() { switchTab('hotkeys'); }
  function onToggleOverride(v: boolean) { cfg.override_conflict_hotkeys = v; scheduleSave(0); setTimeout(runHotkeyAudit, 260); }
  function onUpdateFilters(hidden: string[], custom: CustomFilterTab[], layout?: string) {
    cfg.hidden_builtin_filters = hidden; cfg.custom_filters = custom;
    if (layout) cfg.filter_bar_layout = layout;
    scheduleSave(0);
  }
  function onUpdateAvoidance(fullscreen: boolean, excluded: string[]) {
    cfg.suspend_hotkeys_in_fullscreen = fullscreen; cfg.hotkey_excluded_apps = excluded; scheduleSave(0);
  }
  function onUpdatePluginConfig(
    hotkeys: Record<string, string>,
    settings: Record<string, Record<string, string | number | boolean>>,
    displayModes?: Record<string, string>
  ) {
    cfg.plugin_hotkeys = hotkeys; cfg.plugin_settings = settings;
    if (displayModes !== undefined) { cfg.plugin_display_modes = displayModes; scheduleSave(0); }
    else scheduleSave(120);
  }
  function onSaveFlows(next: WorkflowDef[]) {
    flows = next;
    postHost({ cmd: 'save_flows', flows: $state.snapshot(next) });
  }
  function onTogglePlugin(id: string, enabled: boolean) {
    const low = id.toLowerCase();
    const next = (cfg.disabled_plugins || []).filter((x) => x.toLowerCase() !== low);
    cfg.disabled_plugins = enabled ? next : [...next, low];
    scheduleSave(0);
    setTimeout(() => postHost({ cmd: 'check_hotkey_health' }), 260);
  }

  function setToast(msg: string, warn = false) { toastMsg = msg; toastWarn = warn; }
  function saveNow() { postHost({ cmd: 'save_settings', config: $state.snapshot(cfg), autostart }); setToast(`✓ ${tr(lang, 'toast_saved')} · ${new Date().toLocaleTimeString()}`); }
  function scheduleSave(delayMs = 0) { if (saveTimer) clearTimeout(saveTimer); if (delayMs <= 0) saveNow(); else saveTimer = setTimeout(saveNow, delayMs); }
  function runHotkeyAudit() { auditingHotkeys = true; postHost({ cmd: 'audit_hotkeys' }); }
  function probeHotkeyOwner(spec: string) { probingSpec = spec; probeResult = null; postHost({ cmd: 'probe_hotkey_owner', spec }); }
  function applyFreeKey(slot: CoreHotkeySlot, key: string) {
    const capOn = isPluginEnabled(cfg, PLUGIN_CAPTURE);
    const swapped = assignCoreHotkeyWithSwap(cfg, slot, key, {
      main: true, clipboard: isPluginEnabled(cfg, PLUGIN_CLIPBOARD),
      translate: isPluginEnabled(cfg, PLUGIN_TRANSLATE), capture: capOn, pin: capOn, ocr: capOn,
    });
    scheduleSave(0);
    if (swapped) setToast(lang === 'en' ? '✓ Updated & swapped duplicate key cleanly' : '✓ 已生效（检测到重复绑定，已自动与原槽位互换）');
    setTimeout(runHotkeyAudit, 260);
  }

  function switchTab(id: typeof activeTab) {
    activeTab = id;
    if (id === 'hotkeys' && !hotkeyAudit && !auditingHotkeys) runHotkeyAudit();
    else if (id === 'plugins' && subPlugins === 'store' && !storeLoaded) { storeLoaded = true; fetchStore(); }
    else if (id === 'system' && subSystem === 'logs') refreshLogs();
    else if (id === 'system' && subSystem === 'updates' && !updatesCheckedOnce) {
      updatesCheckedOnce = true;
      if (!updateStatus || Math.floor(Date.now() / 1000) - (updateStatus.checked_at_unix || 0) > 300) checkUpdatesNow(false);
    }
  }

  function applySearchHit(hit: SettingSearchEntry) {
    if (hit.sub) {
      if (hit.tab === 'core') subCore = hit.sub;
      else if (hit.tab === 'search') subSearch = hit.sub;
      else if (hit.tab === 'plugins') subPlugins = hit.sub;
      else if (hit.tab === 'system') subSystem = hit.sub;
    }
    if (hit.pluginId) expandedPlugin = hit.pluginId;
    switchTab(hit.tab);
  }

  function applyInitialRoute(route: string) {
    const [tab, target] = route.trim().split(':', 2);
    if (tab === 'plugins') {
      subPlugins = target === 'store' ? 'store' : 'installed';
      if (target && target !== 'store') expandedPlugin = target;
      switchTab('plugins');
    } else if (tab === 'system') {
      subSystem = target === 'logs' ? 'logs' : 'updates';
      switchTab('system');
    } else if (tab === 'hotkeys' || tab === 'flows' || tab === 'search' || tab === 'core') {
      switchTab(tab);
    }
  }

  function fetchStore(p?: StoreQueryParams) {
    storeLoading = true;
    postHost({
      cmd: 'fetch_store',
      sources: cfg.plugin_registries?.length ? cfg.plugin_registries : ['registry/index.json'],
      query: p?.query || '', category: p?.category || 'all', runtime: p?.runtime || 'all',
      sort: p?.sort || 'default', page: p?.page || 1, page_size: 24, force_refresh: !!p?.force_refresh,
    });
  }
  function refreshLogs() { postHost({ cmd: 'read_logs', source: logSource, errors_only: logErrorsOnly }); }
  function checkUpdatesNow(manual = true) {
    if (checkTimer) { clearTimeout(checkTimer); checkTimer = null; }
    checkingUpdates = true;
    checkStartedAt = Date.now();
    manualUpdateCheck = manual;
    postHost({ cmd: 'check_updates', repo: cfg.update.repo || OFFICIAL_REPO });
  }
  function installRelease(rel: ReleaseInfo) {
    installProgress = { percent: 4, message: lang === 'en' ? 'Downloading & verifying SHA-256…' : '正在下载安装包并校验 SHA-256…' };
    postHost({ cmd: 'install_update', release: rel });
  }
  function onThemePick(name: string) { cfg.theme = name; applyThemeClass(name); postHost({ cmd: 'sync_theme', theme: name }); scheduleSave(0); }
  function onLanguageChange(code: Lang) { cfg.language = code; document.documentElement.lang = code === 'en' ? 'en' : 'zh-CN'; scheduleSave(0); }
  function onAutostartChange(v: boolean) { autostart = v; scheduleSave(0); }
  function checkHotkeyConflictsToast(h: HotkeyHealthReport) {
    const bad = [h.main, h.clipboard, h.translate, h.capture, h.pin, h.ocr].filter((s) => s && !s.disabled && !s.ok && s.spec);
    if (bad.length === 0 && toastWarn) setToast(lang === 'en' ? '✓ All global hotkeys active' : '✓ 所有快捷键已实时生效', false);
  }

  onMount(() => {
    installErrorTelemetry('settings_ui');
    applyThemeClass(cfg.theme);
    document.documentElement.lang = lang === 'en' ? 'en' : 'zh-CN';
    setToast(tr(lang, 'toast_ready'));
    postHost({ cmd: 'check_hotkey_health' });
    if (init.initial_route) applyInitialRoute(init.initial_route);
    requestAnimationFrame(() => postHost({ cmd: 'window_ready' }));

    const unsub = subscribeHost((data) => {
      const ev = String(data.event || '');
      if (ev === 'settings_saved') {
        if (data.config) cfg = data.config as TomoyoConfig;
        const notes = Array.isArray(data.notes) ? (data.notes as string[]) : [];
        if (notes.length > 0) setToast(`⚠ ${notes[0]}`, true);
      } else if (ev === 'hotkey_health' && data.health) {
        hotkeyHealth = data.health as HotkeyHealthReport; checkHotkeyConflictsToast(hotkeyHealth);
      } else if (ev === 'hotkey_audit' && data.report) {
        auditingHotkeys = false; hotkeyAudit = data.report as HotkeyAuditReport;
      } else if (ev === 'hotkey_probe_result' && data.result) {
        probingSpec = ''; probeResult = data.result as LiveProbeResult;
      } else if (ev === 'conflict_process_terminated') {
        setToast(`${data.ok ? '✓ ' : '✗ '}${data.message || ''}`, !data.ok);
        if (data.ok) auditingHotkeys = true;
      } else if (ev === 'folder_picked') {
        if (data.cancelled) return;
        if (data.ok && typeof data.path === 'string') {
          folderErr = '';
          if (!cfg.apps.extra_dirs.some((d) => d.toLowerCase() === String(data.path).toLowerCase())) {
            cfg.apps.extra_dirs = [...cfg.apps.extra_dirs, data.path];
            scheduleSave(0);
          }
        } else if (data.error) folderErr = String(data.error);
      } else if (ev === 'capture_dir_picked') {
        if (!data.cancelled && data.ok && typeof data.path === 'string') { cfg.capture.save_dir = data.path; scheduleSave(0); }
        else if (data.error) setToast(`✗ ${data.error}`, true);
      } else if (ev === 'secrets_status') {
        if (data.status) secretsStatus = { ...secretsStatus, ...(data.status as { deepl: boolean; openai: boolean }) };
        if (data.message) setToast(`${data.ok ? '✓ ' : '✗ '}${data.message}`, !data.ok);
      } else if (ev === 'store_loaded') {
        storeLoading = false;
        storeItems = Array.isArray(data.items) ? (data.items as StoreItem[]) : [];
        storeMeta = {
          total: Number(data.total ?? storeItems.length),
          catalogTotal: Number(data.catalog_total ?? storeItems.length),
          page: Number(data.page ?? 1),
          totalPages: Number(data.total_pages ?? 1),
          categoryCounts: (data.category_counts as Record<string, number>) || {},
        };
        const srcs = Array.isArray(data.sources) ? (data.sources as string[]).join(' · ') : 'Registry';
        const errs = Array.isArray(data.errors) && data.errors.length ? ` (${(data.errors as string[]).join('; ')})` : '';
        storeStatus = srcs + errs;
      } else if (ev === 'store_action_result' || ev === 'perm_action_result') {
        setToast(`${data.ok ? '✓ ' : '✗ '}${data.message || ''}`, !data.ok);
        if (ev === 'store_action_result' && data.ok) fetchStore();
      } else if (ev === 'logs_loaded') {
        const iss = (data.issues || {}) as { warnings?: number; errors?: number; has_crash_report?: boolean };
        logMeta = `${data.source || logSource} · ERROR ${iss.errors || 0} / WARN ${iss.warnings || 0}${iss.has_crash_report ? ' · 存在崩溃快照' : ''} · ${data.logs_dir || init.logs_dir}`;
        logContent = String(data.content || '');
        if (data.cleared) setToast(lang === 'en' ? '✓ Archived logs & crash snapshot' : '✓ 已归档旧日志与崩溃快照');
      } else if (ev === 'diag_bundle_copied') {
        setToast(lang === 'en' ? '✓ Diagnostic bundle copied' : '✓ 已复制脱敏诊断报告到剪贴板');
      } else if (ev === 'config_backup_exported' && typeof data.json === 'string') {
        const blob = new Blob([data.json], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a'); a.href = url; a.download = `tomoyo-backup-${new Date().toISOString().slice(0, 10)}.json`; a.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
        setToast(lang === 'en' ? '✓ Desensitized backup exported' : '✓ 已导出脱敏配置备份（不含 DPAPI 密钥）');
      } else if (ev === 'config_backup_imported') {
        if (data.ok && data.config) { cfg = data.config as TomoyoConfig; applyThemeClass(cfg.theme); document.documentElement.lang = cfg.language === 'en' ? 'en' : 'zh-CN'; }
        setToast(`${data.ok ? '✓ ' : '✗ '}${data.message || ''}`, !data.ok);
      } else if (ev === 'flows_saved') {
        if (data.ok && Array.isArray(data.flows)) flows = data.flows as WorkflowDef[];
        setToast(`${data.ok ? '✓ ' : '✗ '}${data.message || ''}`, !data.ok);
      } else if (ev === 'sync_dir_updated') {
        if (data.cancelled) return;
        if (data.ok) {
          syncDir = String(data.sync_dir || '');
          if (typeof data.config_path === 'string') configPath = data.config_path;
          if (data.config) { cfg = data.config as TomoyoConfig; applyThemeClass(cfg.theme); document.documentElement.lang = cfg.language === 'en' ? 'en' : 'zh-CN'; }
        }
        setToast(`${data.ok ? '✓ ' : '✗ '}${data.message || ''}`, !data.ok);
      } else if (ev === 'update_status') {
        const nextSt = (data.status as UpdateStatus) || null;
        const finish = () => {
          checkingUpdates = false;
          updateStatus = nextSt;
          updatePulse = true;
          setTimeout(() => { updatePulse = false; }, 750);
          if (manualUpdateCheck && nextSt) {
            manualUpdateCheck = false;
            if (nextSt.state === 'up_to_date') setToast(lang === 'en' ? `✓ Up to date (v${nextSt.current || init.version})` : `✓ 已核验：当前已是最新版本 v${nextSt.current || init.version}`);
            else if (nextSt.state === 'available' && nextSt.release) setToast(lang === 'en' ? `↑ New release v${nextSt.release.version} available` : `↑ 发现新版本 v${nextSt.release.version}`);
            else if (nextSt.state === 'error') setToast(`✗ ${nextSt.message || '检查更新失败'}`, true);
          }
        };
        const remain = (manualUpdateCheck ? 480 : 0) - (Date.now() - checkStartedAt);
        if (remain > 0) checkTimer = setTimeout(finish, remain);
        else finish();
      } else if (ev === 'update_install_progress' && data.progress) {
        const p = data.progress as { percent?: number; message?: string };
        installProgress = { percent: Number(p.percent || 5), message: String(p.message || '') };
      } else if (ev === 'update_install_result') {
        installProgress = { percent: data.ok ? 100 : installProgress?.percent || 0, message: String(data.message || ''), done: true, ok: !!data.ok };
        setToast(`${data.ok ? '✓ ' : '✗ '}${data.message || ''}`, !data.ok);
        if (data.ok && data.launched_installer) setTimeout(() => postHost({ cmd: 'close' }), 450);
      }
    });

    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape' && !e.defaultPrevented) postHost({ cmd: 'close' }); };
    window.addEventListener('keydown', onKey);
    return () => { unsub(); if (checkTimer) clearTimeout(checkTimer); window.removeEventListener('keydown', onKey); };
  });
</script>

<div class="shell">
  <SettingsSidebar {lang} {activeTab} conflictCount={conflictSlots.length} onSelectTab={switchTab} onApplyHit={applySearchHit} />

  <main class="main">
    <div class="scroll">
      <div class="container">
        {#if activeTab === 'core'}
          <div class="subbar">
            <Segmented value={subCore} options={[{ value: 'general', label: tr(lang, 'nav_general') }, { value: 'appearance', label: tr(lang, 'nav_appearance') }]} onchange={(v) => (subCore = v)} />
          </div>
          {#if subCore === 'general'}
            <GeneralTab {cfg} {lang} {autostart} {syncDir} hotkeyStatus={hotkeyHealth.main} {onLanguageChange} {onAutostartChange} onJumpDoctor={() => switchTab('hotkeys')} onSave={scheduleSave} />
          {:else}
            <AppearanceTab bind:cfg {lang} systemFonts={init.system_fonts || []} {onThemePick} onSave={scheduleSave} />
          {/if}
        {:else if activeTab === 'search'}
          <div class="subbar">
            <Segmented value={subSearch} options={[{ value: 'indexing', label: lang === 'en' ? 'Apps & Filters' : '应用与标签' }, { value: 'web', label: tr(lang, 'nav_web') }]} onchange={(v) => (subSearch = v)} />
          </div>
          {#if subSearch === 'indexing'}
            <SearchClipTab {cfg} {lang} {folderErr} {onUpdateFilters} onSave={scheduleSave} />
          {:else}
            <WebSearchTab {cfg} {lang} onSave={scheduleSave} />
          {/if}
        {:else if activeTab === 'hotkeys'}
          <HotkeysTab {cfg} {lang} runningApps={init.running_apps} health={hotkeyHealth} audit={hotkeyAudit} auditing={auditingHotkeys} {probingSpec} {probeResult} onRunAudit={runHotkeyAudit} onProbe={probeHotkeyOwner} onApplyFreeKey={applyFreeKey} {onToggleOverride} {onUpdateAvoidance} />
        {:else if activeTab === 'flows'}
          <FlowsTab {lang} {flows} flowsPath={init.flows_path || ''} {onSaveFlows} />
        {:else if activeTab === 'plugins'}
          <div class="subbar">
            <Segmented value={subPlugins} options={[{ value: 'installed', label: lang === 'en' ? 'Modules & Plugins' : '已安装与插件设置' }, { value: 'store', label: tr(lang, 'nav_store') }]} onchange={(v) => { subPlugins = v; if (v === 'store' && !storeLoaded) { storeLoaded = true; fetchStore(); } }} />
          </div>
          {#if subPlugins === 'installed'}
            <PluginsTab {cfg} {lang} {plugins} officialIcons={init.official_icons || {}} {configPath} pluginsDir={init.plugins_dir} runningApps={init.running_apps} {secretsStatus} {hotkeyHealth} showRecord={true} expandedId={expandedPlugin} onToggleExpand={(id) => (expandedPlugin = id)} {onTogglePlugin} {onUpdatePluginConfig} onJumpDoctor={() => switchTab('hotkeys')} onToast={setToast} onSave={scheduleSave} />
          {:else}
            <StoreTab {lang} items={storeItems} meta={storeMeta} statusText={storeStatus} loading={storeLoading} onQuery={fetchStore} />
          {/if}
        {:else if activeTab === 'system'}
          <div class="subbar">
            <Segmented value={subSystem} options={[{ value: 'updates', label: tr(lang, 'nav_updates') }, { value: 'logs', label: tr(lang, 'nav_logs') }]} onchange={(v) => { subSystem = v; if (v === 'logs') refreshLogs(); }} />
          </div>
          {#if subSystem === 'updates'}
            <UpdatesTab {cfg} {lang} version={init.version} {updateStatus} checking={checkingUpdates} pulse={updatePulse} {installProgress} onCheckNow={() => checkUpdatesNow(true)} onInstall={installRelease} onSave={scheduleSave} />
          {:else}
            <LogsTab {lang} source={logSource} errorsOnly={logErrorsOnly} content={logContent} metaLine={logMeta} onSourceChange={(s) => { logSource = s; refreshLogs(); }} onErrorsOnlyChange={(v) => { logErrorsOnly = v; refreshLogs(); }} onRefresh={refreshLogs} />
          {/if}
        {/if}
      </div>
    </div>

    <footer class="bottom-bar">
      {#if conflictSlots.length > 0}
        {@const c0 = conflictSlots[0]!}
        <button id="toast" type="button" class="conflict-bar-btn" onclick={jumpToHotkeyConflict}>
          ⚠ {conflictSlots.length === 1
            ? (lang === 'en' ? `${c0.label} ${c0.st?.spec} conflict · Click to fix →` : `${c0.label}快捷键 ${c0.st?.spec} 冲突 · 点击定位 →`)
            : (lang === 'en' ? `${conflictSlots.length} hotkey conflicts (${c0.label} ${c0.st?.spec}) · Click to fix →` : `${conflictSlots.length} 项快捷键冲突（${c0.label} ${c0.st?.spec} 等）· 点击修复 →`)}
        </button>
      {:else}
        <span id="toast" class="toast {toastWarn ? 'is-warn' : ''}">{toastMsg}</span>
      {/if}
      <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'close' })}>{tr(lang, 'btn_close')}</Button>
    </footer>
  </main>
</div>

<style>
  .shell { display: flex; height: 100%; width: 100%; overflow: hidden; }
  .main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .scroll { flex: 1; overflow-y: auto; padding: 20px 28px; }
  .container { max-width: 740px; }
  .subbar { margin-bottom: 14px; }
  .bottom-bar { height: 46px; padding: 0 24px; border-top: 1px solid var(--border); background: var(--sidebar); display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-shrink: 0; }
  .toast { font-size: 12.5px; color: var(--ok); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .toast.is-warn { color: var(--warn); }
  .conflict-bar-btn { border: 1px solid rgba(220, 38, 38, 0.28); background: var(--danger-soft); color: var(--danger); border-radius: 999px; padding: 4px 12px; font-family: inherit; font-size: 12px; font-weight: 500; cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
