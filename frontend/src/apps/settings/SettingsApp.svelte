<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Camera, FileText, Globe, Keyboard, Languages, Package,
    Palette, RefreshCw, Search, SlidersHorizontal, Store,
  } from '@lucide/svelte';
  import {
    hydrateSettingsInit, isPluginEnabled, PLUGIN_CAPTURE, PLUGIN_CLIPBOARD, PLUGIN_TRANSLATE,
  } from '../../lib/defaults';
  import { applyThemeClass, installErrorTelemetry, postHost, subscribeHost } from '../../lib/ipc';
  import { OFFICIAL_REPO, tr, type Lang } from '../../lib/i18n';
  import type {
    CustomFilterTab, HotkeyAuditReport, HotkeyHealthReport, LiveProbeResult,
    ReleaseInfo, TomoyoConfig, UpdateStatus,
  } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import AppearanceTab from './AppearanceTab.svelte';
  import CaptureTab from './CaptureTab.svelte';
  import GeneralTab from './GeneralTab.svelte';
  import HotkeysTab from './HotkeysTab.svelte';
  import LogsTab from './LogsTab.svelte';
  import PluginsTab from './PluginsTab.svelte';
  import SearchClipTab from './SearchClipTab.svelte';
  import StoreTab, { type StoreItem } from './StoreTab.svelte';
  import TranslateTab from './TranslateTab.svelte';
  import UpdatesTab from './UpdatesTab.svelte';
  import WebSearchTab from './WebSearchTab.svelte';

  const init = hydrateSettingsInit();

  let cfg = $state<TomoyoConfig>(init.config);
  let autostart = $state(init.autostart);
  let plugins = $state(init.plugins);
  let secretsStatus = $state(init.secrets_status);
  let hotkeyHealth = $state<HotkeyHealthReport>(init.hotkey_health || {});
  let activeTab = $state('general');
  let toastMsg = $state('');
  let toastWarn = $state(false);
  let folderErr = $state('');

  // Hotkey Doctor state
  let hotkeyAudit = $state<HotkeyAuditReport | null>(null);
  let auditingHotkeys = $state(false);
  let probingSpec = $state('');
  let probeResult = $state<LiveProbeResult | null>(null);

  // Store / Logs / Updates state
  let storeLoaded = $state(false);
  let storeLoading = $state(false);
  let storeItems = $state<StoreItem[]>([]);
  let storeStatus = $state('');
  let logSource = $state('tomoyo.log');
  let logErrorsOnly = $state(false);
  let logContent = $state('');
  let logMeta = $state('');
  let updateStatus = $state<UpdateStatus | null>(init.cached_update ?? null);
  let checkingUpdates = $state(false);
  let updatesCheckedOnce = $state(false);
  let installProgress = $state<{ percent: number; message: string; done?: boolean; ok?: boolean } | null>(null);

  let lang = $derived<Lang>(cfg.language === 'en' ? 'en' : 'zh');
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  let conflictSlots = $derived.by(() => {
    const entries = [
      { label: lang === 'en' ? 'Launcher' : '主启动器', st: hotkeyHealth.main, on: true },
      { label: lang === 'en' ? 'Clipboard' : '剪贴板', st: hotkeyHealth.clipboard, on: isPluginEnabled(cfg, PLUGIN_CLIPBOARD) },
      { label: lang === 'en' ? 'Translate' : '划词翻译', st: hotkeyHealth.translate, on: isPluginEnabled(cfg, PLUGIN_TRANSLATE) },
      { label: lang === 'en' ? 'Capture' : '截图', st: hotkeyHealth.capture, on: isPluginEnabled(cfg, PLUGIN_CAPTURE) },
    ];
    return entries.filter((e) => e.on && e.st && !e.st.disabled && !e.st.ok && e.st.spec);
  });

  function jumpToHotkeyConflict() { activeTab = 'hotkeys'; runHotkeyAudit(); }
  function onToggleOverride(v: boolean) { cfg.override_conflict_hotkeys = v; scheduleSave(0); setTimeout(runHotkeyAudit, 260); }
  function onUpdateFilters(hidden: string[], custom: CustomFilterTab[]) {
    cfg.hidden_builtin_filters = hidden;
    cfg.custom_filters = custom;
    scheduleSave(0);
  }
  function onTogglePlugin(id: string, enabled: boolean) {
    const low = id.toLowerCase();
    const next = (cfg.disabled_plugins || []).filter((x) => x.toLowerCase() !== low);
    cfg.disabled_plugins = enabled ? next : [...next, low];
    if (!enabled && ((low === PLUGIN_TRANSLATE && activeTab === 'translate') || (low === PLUGIN_CAPTURE && activeTab === 'capture'))) {
      activeTab = 'plugins';
    }
    scheduleSave(0);
    setTimeout(() => postHost({ cmd: 'check_hotkey_health' }), 260);
  }

  let navItems = $derived([
    { id: 'general', key: 'nav_general', icon: SlidersHorizontal, on: true },
    { id: 'hotkeys', key: 'nav_hotkeys', icon: Keyboard, on: true },
    { id: 'appearance', key: 'nav_appearance', icon: Palette, on: true },
    { id: 'search', key: 'nav_search', icon: Search, on: true },
    { id: 'translate', key: 'nav_translate', icon: Languages, on: isPluginEnabled(cfg, PLUGIN_TRANSLATE) },
    { id: 'capture', key: 'nav_capture', icon: Camera, on: isPluginEnabled(cfg, PLUGIN_CAPTURE) },
    { id: 'web', key: 'nav_web', icon: Globe, on: true },
    { id: 'store', key: 'nav_store', icon: Store, on: true },
    { id: 'plugins', key: 'nav_plugins', icon: Package, on: true },
    { id: 'logs', key: 'nav_logs', icon: FileText, on: true },
    { id: 'updates', key: 'nav_updates', icon: RefreshCw, on: true },
  ].filter((n) => n.on));

  function setToast(msg: string, warn = false) {
    toastMsg = msg;
    toastWarn = warn;
  }

  function saveNow() {
    postHost({ cmd: 'save_settings', config: $state.snapshot(cfg), autostart });
    setToast(`✓ ${tr(lang, 'toast_saved')} · ${new Date().toLocaleTimeString()}`);
  }

  function scheduleSave(delayMs = 0) {
    if (saveTimer) clearTimeout(saveTimer);
    if (delayMs <= 0) saveNow();
    else saveTimer = setTimeout(saveNow, delayMs);
  }

  function runHotkeyAudit() {
    auditingHotkeys = true;
    postHost({ cmd: 'audit_hotkeys' });
  }

  function probeHotkeyOwner(spec: string) {
    probingSpec = spec;
    probeResult = null;
    postHost({ cmd: 'probe_hotkey_owner', spec });
  }

  function applyFreeKey(slot: 'main' | 'clipboard' | 'translate' | 'capture', key: string) {
    if (slot === 'main') cfg.hotkey = key;
    else if (slot === 'clipboard') cfg.clipboard.hotkey = key;
    else if (slot === 'translate') cfg.translate.hotkey = key;
    else cfg.capture.hotkey = key;
    scheduleSave(0);
    setTimeout(runHotkeyAudit, 260);
  }

  function switchTab(id: string) {
    activeTab = id;
    if (id === 'hotkeys' && !hotkeyAudit && !auditingHotkeys) {
      runHotkeyAudit();
    } else if (id === 'store' && !storeLoaded) {
      storeLoaded = true;
      fetchStore();
    } else if (id === 'logs') {
      refreshLogs();
    } else if (id === 'updates' && !updatesCheckedOnce) {
      updatesCheckedOnce = true;
      const nowSec = Math.floor(Date.now() / 1000);
      const last = updateStatus?.checked_at_unix || 0;
      if (!updateStatus || nowSec - last > 300) checkUpdatesNow();
    }
  }

  function fetchStore() {
    storeLoading = true;
    const sources = cfg.plugin_registries?.length ? cfg.plugin_registries : ['registry/index.json'];
    postHost({ cmd: 'fetch_store', sources });
  }

  function refreshLogs() {
    postHost({ cmd: 'read_logs', source: logSource, errors_only: logErrorsOnly });
  }

  function checkUpdatesNow() {
    checkingUpdates = true;
    postHost({ cmd: 'check_updates', repo: cfg.update.repo || OFFICIAL_REPO });
  }

  function installRelease(rel: ReleaseInfo) {
    installProgress = { percent: 4, message: lang === 'en' ? 'Downloading & verifying SHA-256…' : '正在下载安装包并校验 SHA-256…' };
    postHost({ cmd: 'install_update', release: rel });
  }

  function onThemePick(name: string) {
    cfg.theme = name;
    applyThemeClass(name);
    postHost({ cmd: 'sync_theme', theme: name });
    scheduleSave(0);
  }

  function onLanguageChange(code: Lang) {
    cfg.language = code;
    document.documentElement.lang = code === 'en' ? 'en' : 'zh-CN';
    scheduleSave(0);
  }

  function onAutostartChange(v: boolean) {
    autostart = v;
    scheduleSave(0);
  }

  function checkHotkeyConflictsToast(h: HotkeyHealthReport) {
    const bad = [h.main, h.clipboard, h.translate, h.capture].filter((s) => s && !s.disabled && !s.ok && s.spec);
    if (bad.length === 0 && toastWarn) {
      setToast(lang === 'en' ? '✓ All global hotkeys active' : '✓ 所有快捷键已实时生效', false);
    }
  }

  onMount(() => {
    installErrorTelemetry('settings_ui');
    applyThemeClass(cfg.theme);
    document.documentElement.lang = lang === 'en' ? 'en' : 'zh-CN';
    setToast(tr(lang, 'toast_ready'));
    postHost({ cmd: 'check_hotkey_health' });

    const unsub = subscribeHost((data) => {
      const ev = String(data.event || '');
      if (ev === 'settings_saved') {
        if (data.config) cfg = data.config as TomoyoConfig;
        const notes = Array.isArray(data.notes) ? (data.notes as string[]) : [];
        if (notes.length > 0) setToast(`⚠ ${notes[0]}`, true);
      } else if (ev === 'hotkey_health' && data.health) {
        hotkeyHealth = data.health as HotkeyHealthReport;
        checkHotkeyConflictsToast(hotkeyHealth);
      } else if (ev === 'hotkey_audit' && data.report) {
        auditingHotkeys = false;
        hotkeyAudit = data.report as HotkeyAuditReport;
      } else if (ev === 'hotkey_probe_result' && data.result) {
        probingSpec = '';
        probeResult = data.result as LiveProbeResult;
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
        if (!data.cancelled && data.ok && typeof data.path === 'string') {
          cfg.capture.save_dir = data.path;
          scheduleSave(0);
        } else if (data.error) setToast(`✗ ${data.error}`, true);
      } else if (ev === 'secrets_status') {
        if (data.status) secretsStatus = { ...secretsStatus, ...(data.status as { deepl: boolean; openai: boolean }) };
        if (data.message) setToast(`${data.ok ? '✓ ' : '✗ '}${data.message}`, !data.ok);
      } else if (ev === 'store_loaded') {
        storeLoading = false;
        storeItems = Array.isArray(data.items) ? (data.items as StoreItem[]) : [];
        const srcs = Array.isArray(data.sources) ? (data.sources as string[]).join(' · ') : 'Registry';
        const errs = Array.isArray(data.errors) && data.errors.length ? ` (${(data.errors as string[]).join('; ')})` : '';
        storeStatus = srcs + errs;
      } else if (ev === 'store_action_result' || ev === 'perm_action_result') {
        setToast(`${data.ok ? '✓ ' : '✗ '}${data.message || ''}`, !data.ok);
        if (ev === 'store_action_result' && data.ok) fetchStore();
      } else if (ev === 'logs_loaded') {
        const iss = (data.issues || {}) as { warnings?: number; errors?: number; has_crash_report?: boolean };
        const crash = iss.has_crash_report ? ' · 存在崩溃快照' : '';
        logMeta = `${data.source || logSource} · ERROR ${iss.errors || 0} / WARN ${iss.warnings || 0}${crash} · ${data.logs_dir || init.logs_dir}`;
        logContent = String(data.content || '');
      } else if (ev === 'diag_bundle_copied') {
        setToast(lang === 'en' ? '✓ Diagnostic bundle copied' : '✓ 已复制脱敏诊断报告到剪贴板');
      } else if (ev === 'config_backup_exported' && typeof data.json === 'string') {
        const blob = new Blob([data.json], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `tomoyo-backup-${new Date().toISOString().slice(0, 10)}.json`;
        a.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
        setToast(lang === 'en' ? '✓ Desensitized backup exported' : '✓ 已导出脱敏配置备份（不含 DPAPI 密钥）');
      } else if (ev === 'config_backup_imported') {
        if (data.ok && data.config) {
          cfg = data.config as TomoyoConfig;
          applyThemeClass(cfg.theme);
          document.documentElement.lang = cfg.language === 'en' ? 'en' : 'zh-CN';
        }
        setToast(`${data.ok ? '✓ ' : '✗ '}${data.message || ''}`, !data.ok);
      } else if (ev === 'update_status') {
        checkingUpdates = false;
        updateStatus = (data.status as UpdateStatus) || null;
      } else if (ev === 'update_install_progress' && data.progress) {
        const p = data.progress as { percent?: number; message?: string };
        installProgress = { percent: Number(p.percent || 5), message: String(p.message || '') };
      } else if (ev === 'update_install_result') {
        installProgress = {
          percent: data.ok ? 100 : installProgress?.percent || 0,
          message: String(data.message || ''),
          done: true,
          ok: !!data.ok,
        };
        setToast(`${data.ok ? '✓ ' : '✗ '}${data.message || ''}`, !data.ok);
        if (data.ok && data.launched_installer) setTimeout(() => postHost({ cmd: 'close' }), 450);
      }
    });

    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.defaultPrevented) postHost({ cmd: 'close' });
    };
    window.addEventListener('keydown', onKey);
    return () => {
      unsub();
      window.removeEventListener('keydown', onKey);
    };
  });
</script>

<div class="shell">
  <aside class="sidebar">
    <div class="brand">
      <div class="brand-tile">T</div>
      <div>
        <div class="brand-name">Tomoyo</div>
        <div class="brand-sub">{tr(lang, 'brand_sub')}</div>
      </div>
    </div>

    <nav class="nav">
      {#each navItems as item}
        {@const Icon = item.icon}
        <button
          type="button"
          data-tab={item.id}
          class="nav-item {activeTab === item.id ? 'is-active' : ''}"
          onclick={() => switchTab(item.id)}
        >
          <span class="nav-ico"><Icon size={15} /></span>
          <span>{tr(lang, item.key)}</span>
          {#if item.id === 'hotkeys' && conflictSlots.length > 0}
            <span class="nav-badge">{conflictSlots.length}</span>
          {/if}
        </button>
      {/each}
    </nav>

    <div class="side-foot">{tr(lang, 'sidebar_foot')}</div>
  </aside>

  <main class="main">
    <div class="scroll">
      <div class="container">
        {#if activeTab === 'general'}
          <GeneralTab
            {cfg} {lang} {autostart} hotkeyStatus={hotkeyHealth.main}
            {onLanguageChange} {onAutostartChange}
            onJumpDoctor={() => switchTab('hotkeys')} onSave={scheduleSave}
          />
        {:else if activeTab === 'hotkeys'}
          <HotkeysTab
            {cfg} {lang} health={hotkeyHealth} audit={hotkeyAudit} auditing={auditingHotkeys}
            {probingSpec} {probeResult}
            onRunAudit={runHotkeyAudit} onProbe={probeHotkeyOwner} onApplyFreeKey={applyFreeKey}
            {onToggleOverride}
          />
        {:else if activeTab === 'appearance'}
          <AppearanceTab {cfg} {lang} {onThemePick} onSave={scheduleSave} />
        {:else if activeTab === 'search'}
          <SearchClipTab
            {cfg} {lang} runningApps={init.running_apps} {folderErr}
            clipHotkeyStatus={hotkeyHealth.clipboard}
            onJumpDoctor={() => switchTab('hotkeys')} onOpenPlugins={() => switchTab('plugins')} {onUpdateFilters} onSave={scheduleSave}
          />
        {:else if activeTab === 'translate'}
          <TranslateTab
            {cfg} {lang} {secretsStatus} trHotkeyStatus={hotkeyHealth.translate}
            onJumpDoctor={() => switchTab('hotkeys')} onToast={setToast} onSave={scheduleSave}
          />
        {:else if activeTab === 'capture'}
          <CaptureTab
            {cfg} {lang} capHotkeyStatus={hotkeyHealth.capture}
            onJumpDoctor={() => switchTab('hotkeys')} onSave={scheduleSave}
          />
        {:else if activeTab === 'web'}
          <WebSearchTab {cfg} {lang} onSave={scheduleSave} />
        {:else if activeTab === 'store'}
          <StoreTab {lang} items={storeItems} statusText={storeStatus} loading={storeLoading} onRefresh={fetchStore} />
        {:else if activeTab === 'plugins'}
          <PluginsTab {cfg} {lang} {plugins} configPath={init.config_path} pluginsDir={init.plugins_dir} {onTogglePlugin} />
        {:else if activeTab === 'logs'}
          <LogsTab
            {lang} source={logSource} errorsOnly={logErrorsOnly} content={logContent} metaLine={logMeta}
            onSourceChange={(s) => { logSource = s; refreshLogs(); }}
            onErrorsOnlyChange={(v) => { logErrorsOnly = v; refreshLogs(); }}
            onRefresh={refreshLogs}
          />
        {:else if activeTab === 'updates'}
          <UpdatesTab
            {cfg} {lang} version={init.version} {updateStatus} checking={checkingUpdates}
            {installProgress} onCheckNow={checkUpdatesNow} onInstall={installRelease} onSave={scheduleSave}
          />
        {/if}
      </div>
    </div>

    <footer class="bottom-bar">
      {#if conflictSlots.length > 0}
        {@const c0 = conflictSlots[0]!}
        <button id="toast" type="button" class="conflict-bar-btn" onclick={jumpToHotkeyConflict}>
          ⚠ {conflictSlots.length === 1
            ? (lang === 'en'
              ? `${c0.label} hotkey ${c0.st?.spec} occupied · Click to locate & fix →`
              : `${c0.label}快捷键 ${c0.st?.spec} 被占用 · 点击定位与强行接管 →`)
            : (lang === 'en'
              ? `${conflictSlots.length} hotkeys conflicted (${c0.label} ${c0.st?.spec}) · Click to fix →`
              : `${conflictSlots.length} 项快捷键被占用（${c0.label} ${c0.st?.spec} 等）· 点击定位与修复 →`)}
        </button>
      {:else}
        <span id="toast" class="toast {toastWarn ? 'is-warn' : ''}">{toastMsg}</span>
      {/if}
      <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'close' })}>
        {tr(lang, 'btn_close')}
      </Button>
    </footer>
  </main>
</div>

<style>
  .shell { display: flex; height: 100%; width: 100%; overflow: hidden; }
  .sidebar { width: 216px; flex-shrink: 0; background: var(--sidebar); border-right: 1px solid var(--border); display: flex; flex-direction: column; padding: 16px 10px 14px; }
  .brand { display: flex; align-items: center; gap: 10px; padding: 2px 8px 14px; border-bottom: 1px solid var(--border); margin-bottom: 10px; }
  .brand-tile { width: 30px; height: 30px; border-radius: 8px; background: linear-gradient(155deg, #ffffff 0%, #eaeef6 100%); border: 1px solid #d5dce9; color: #111827; font-family: Georgia, serif; font-weight: 700; font-size: 17px; display: grid; place-items: center; box-shadow: 0 1px 3px rgba(15, 23, 42, 0.1); }
  .brand-name { font-size: 14.5px; font-weight: 600; letter-spacing: 0.2px; }
  .brand-sub { font-size: 11px; color: var(--muted); }
  .nav { display: flex; flex-direction: column; gap: 2px; overflow-y: auto; }
  .nav-item { position: relative; display: flex; align-items: center; gap: 9px; width: 100%; height: 34px; padding: 0 10px; border: 0; border-radius: 7px; background: transparent; color: var(--text-secondary); font-family: inherit; font-size: 13px; font-weight: 500; text-align: left; white-space: nowrap; cursor: pointer; transition: background-color 0.12s ease, color 0.12s ease; }
  .nav-item:hover:not(.is-active) { background: var(--hover); color: var(--text); }
  .nav-item.is-active { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .nav-item.is-active::before { content: ""; position: absolute; left: 0; top: 7px; bottom: 7px; width: 3px; border-radius: 999px; background: var(--accent); }
  .nav-ico { display: inline-flex; opacity: 0.85; flex-shrink: 0; }
  .nav-badge { margin-left: auto; min-width: 17px; height: 17px; padding: 0 5px; border-radius: 999px; background: var(--danger); color: #fff; font-size: 10.5px; font-weight: 600; display: inline-flex; align-items: center; justify-content: center; }
  .side-foot { margin-top: auto; padding: 10px 8px 0; border-top: 1px solid var(--border); font-size: 11px; color: var(--muted); line-height: 1.55; }
  .main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .scroll { flex: 1; overflow-y: auto; padding: 24px 28px; }
  .container { max-width: 740px; }
  .bottom-bar { height: 46px; padding: 0 24px; border-top: 1px solid var(--border); background: var(--sidebar); display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-shrink: 0; }
  .toast { font-size: 12.5px; color: var(--ok); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .toast.is-warn { color: var(--warn); }
  .conflict-bar-btn { border: 1px solid rgba(220, 38, 38, 0.28); background: var(--danger-soft); color: var(--danger); border-radius: 999px; padding: 4px 12px; font-family: inherit; font-size: 12px; font-weight: 500; cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; transition: background-color 0.12s ease, border-color 0.12s ease; }
  .conflict-bar-btn:hover { border-color: var(--danger); }
</style>
