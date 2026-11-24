<script lang="ts">
  import { Crosshair, FolderOpen, RefreshCw } from '@lucide/svelte';
  import { isPluginEnabled, PLUGIN_CAPTURE, PLUGIN_CLIPBOARD, PLUGIN_TRANSLATE } from '../../lib/defaults';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type {
    HotkeyAuditReport,
    HotkeyHealthReport,
    LiveProbeResult,
    SuspectProcess,
    TomoyoConfig,
  } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Combobox from '../../lib/ui/Combobox.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import HotkeySlotsCard from './HotkeySlotsCard.svelte';
  import SuspectDetailCard from './SuspectDetailCard.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    runningApps: string[];
    health: HotkeyHealthReport;
    audit: HotkeyAuditReport | null;
    auditing: boolean;
    probingSpec: string;
    probeResult: LiveProbeResult | null;
    onRunAudit: () => void;
    onProbe: (spec: string) => void;
    onApplyFreeKey: (slot: 'main' | 'clipboard' | 'translate' | 'capture' | 'pin' | 'ocr' | 'record', key: string) => void;
    onToggleOverride: (v: boolean) => void;
    onUpdateAvoidance: (fullscreen: boolean, excluded: string[]) => void;
  }

  let {
    cfg, lang, runningApps, health, audit, auditing, probingSpec, probeResult,
    onRunAudit, onProbe, onApplyFreeKey, onToggleOverride, onUpdateAvoidance,
  }: Props = $props();

  function normalizeExe(raw: string): string | null {
    let s = raw.trim().toLowerCase();
    if (!s || /[\\/]|\.\./.test(s) || s.length > 64) return null;
    if (!s.endsWith('.exe')) s += '.exe';
    const stem = s.slice(0, -4);
    return stem && /^[a-z0-9._ -]+$/.test(stem) ? s : null;
  }

  let filter = $state('all');
  let customSpec = $state('');
  let selectedSuspect = $state<{ spec: string; suspect: SuspectProcess } | null>(null);
  let isEn = $derived(lang === 'en');

  const slotPresets = {
    main: [{ value: 'Alt+Space', label: 'Alt+Space' }, { value: 'Ctrl+Space', label: 'Ctrl+Space' }, { value: 'Ctrl+Shift+Space', label: 'Ctrl+Shift+Space' }, { value: 'Alt+Q', label: 'Alt+Q' }],
    clipboard: [{ value: 'Alt+V', label: 'Alt+V' }, { value: 'Ctrl+Shift+V', label: 'Ctrl+Shift+V' }, { value: 'Alt+`', label: 'Alt+`' }, { value: 'Ctrl+Alt+V', label: 'Ctrl+Alt+V' }],
    translate: [{ value: 'Alt+T', label: 'Alt+T' }, { value: 'Ctrl+Shift+T', label: 'Ctrl+Shift+T' }, { value: 'Alt+Q', label: 'Alt+Q' }, { value: 'F4', label: 'F4' }],
    capture: [{ value: 'F1', label: 'F1' }, { value: 'Alt+S', label: 'Alt+S' }, { value: 'Alt+A', label: 'Alt+A' }, { value: 'Ctrl+Shift+A', label: 'Ctrl+Shift+A' }, { value: 'PrintScreen', label: 'PrintScreen' }],
    pin: [{ value: 'F3', label: 'F3' }, { value: 'Alt+3', label: 'Alt+3' }, { value: 'Alt+P', label: 'Alt+P' }, { value: 'Ctrl+Shift+P', label: 'Ctrl+Shift+P' }],
    ocr: [{ value: 'Alt+Shift+C', label: 'Alt+Shift+C' }, { value: 'Ctrl+Shift+O', label: 'Ctrl+Shift+O' }, { value: 'F4', label: 'F4' }, { value: 'Alt+O', label: 'Alt+O' }],
    record: [{ value: 'Alt+Shift+R', label: 'Alt+Shift+R' }, { value: 'Ctrl+Shift+R', label: 'Ctrl+Shift+R' }, { value: 'Alt+R', label: 'Alt+R' }, { value: 'F9', label: 'F9' }],
  };

  let slots = $derived([
    { id: 'main' as const, label: isEn ? 'Main Launcher' : '主启动器唤起', spec: cfg.hotkey, st: health.main, on: true, presets: slotPresets.main },
    { id: 'clipboard' as const, label: isEn ? 'Clipboard Center' : '剪贴板管理中心', spec: cfg.clipboard.hotkey, st: health.clipboard, on: isPluginEnabled(cfg, PLUGIN_CLIPBOARD), presets: slotPresets.clipboard },
    { id: 'translate' as const, label: isEn ? 'Selection Translate' : '全局划词翻译', spec: cfg.translate.hotkey, st: health.translate, on: isPluginEnabled(cfg, PLUGIN_TRANSLATE), presets: slotPresets.translate },
    { id: 'capture' as const, label: isEn ? 'Region Screenshot' : '全局区域截图', spec: cfg.capture.hotkey, st: health.capture, on: isPluginEnabled(cfg, PLUGIN_CAPTURE), presets: slotPresets.capture },
    { id: 'pin' as const, label: isEn ? 'Pin Latest Clipboard' : '全局一键贴图', spec: cfg.capture.pin_hotkey, st: health.pin, on: isPluginEnabled(cfg, PLUGIN_CAPTURE), presets: slotPresets.pin },
    { id: 'ocr' as const, label: isEn ? 'Screenshot OCR' : '全局截图识字', spec: cfg.capture.ocr_hotkey, st: health.ocr, on: isPluginEnabled(cfg, PLUGIN_CAPTURE), presets: slotPresets.ocr },
    { id: 'record' as const, label: isEn ? 'Region Screen Record' : '全局区域录屏', spec: cfg.capture.record_hotkey || '', st: health.record, on: isPluginEnabled(cfg, PLUGIN_CAPTURE), presets: slotPresets.record },
  ].filter((s) => s.on));

  let filteredMatrix = $derived.by(() => {
    const list = audit?.items || [];
    if (filter === 'busy') return list.filter((i) => i.status === 'conflict' || i.status === 'occupied_external' || i.status === 'overridden');
    if (filter === 'free') return list.filter((i) => i.status === 'free');
    return list;
  });

  function statusLabel(st: string): string {
    if (isEn) {
      if (st === 'conflict') return 'Conflict';
      if (st === 'overridden') return 'Rescued';
      if (st === 'occupied_external') return 'Occupied';
      if (st === 'tomoyo') return 'Tomoyo';
      return 'Free';
    }
    if (st === 'conflict') return '冲突被占';
    if (st === 'overridden') return '已强行接管';
    if (st === 'occupied_external') return '外部占用';
    if (st === 'tomoyo') return 'Tomoyo 已占';
    return '当前空闲';
  }

  function runCustomProbe() {
    const clean = customSpec.trim();
    if (!clean || probingSpec) return;
    onProbe(clean);
  }
</script>

<div class="tab-head-bar">
  <div>
    <h2>{tr(lang, 'h_hotkeys')}</h2>
    <p class="tab-lead">{tr(lang, 'lead_hotkeys')}</p>
  </div>
  <Button variant="primary" disabled={auditing} onclick={onRunAudit}>
    <RefreshCw size={13} />
    {auditing ? (isEn ? 'Scanning…' : '正在探测…') : (isEn ? 'Scan Now' : '重新扫描热键')}
  </Button>
</div>

{#if audit}
  <div class="kpi-strip">
    <div class="kpi-pill"><span>{isEn ? 'Probed' : '已扫描'}</span><strong>{audit.total_probed}</strong></div>
    <div class="kpi-pill"><span>{isEn ? 'External' : '外部占用'}</span><strong>{audit.external_occupied}</strong></div>
    <div class="kpi-pill is-ov"><span>{isEn ? 'Rescued' : '高优接管'}</span><strong>{audit.overridden || 0}</strong></div>
    <div class="kpi-pill {audit.conflicts > 0 ? 'is-bad' : 'is-ok'}">
      <span>{isEn ? 'Conflicts' : '未解决冲突'}</span><strong>{audit.conflicts}</strong>
    </div>
  </div>
{/if}

<HotkeySlotsCard
  {cfg}
  {lang}
  {slots}
  {audit}
  {probingSpec}
  {onApplyFreeKey}
  {onProbe}
  {onToggleOverride}
  onSelectSuspect={(spec, suspect) => (selectedSuspect = { spec, suspect })}
/>

<!-- Fullscreen & Remote-Desktop Zero-Polling Hotkey Avoidance -->
<div class="t-card sec-card" style="margin-top:14px">
  <div class="avoid-row">
    <div>
      <div class="t-label">{isEn ? 'Auto-Yield Hotkeys in Fullscreen Games / Videos' : '全屏游戏与全屏视频时自动避让热键'}</div>
      <div class="t-desc">
        {isEn
          ? 'Zero-polling check only on keypress; passes keys straight through during D3D or borderless fullscreen.'
          : '零后台轮询，仅在按键瞬间放行全屏游戏/播放器，杜绝误呼出打断。'}
      </div>
    </div>
    <Switch
      checked={cfg.suspend_hotkeys_in_fullscreen !== false}
      onchange={(v) => onUpdateAvoidance(v, cfg.hotkey_excluded_apps || [])}
    />
  </div>
  <div class="avoid-col">
    <div>
      <div class="t-label">{isEn ? 'Remote Desktop & App Hotkey Yield List' : '远程桌面与指定前台应用避让名单'}</div>
      <div class="t-desc">
        {isEn
          ? 'When any process below is active, Tomoyo yields all shortcuts & override hooks directly to it.'
          : '当前台窗口命中以下进程（如 RayLink / 远程桌面）时，热键与接管钩子全部直接透传给对方。'}
      </div>
    </div>
    <Combobox
      selected={cfg.hotkey_excluded_apps || []}
      suggestions={runningApps}
      placeholder={isEn ? 'Search running apps or type .exe (e.g. raylink.exe)…' : '搜索运行中程序或输入进程名（如 raylink.exe）…'}
      emptyText={isEn ? 'No apps excluded' : '尚未配置热键避让应用'}
      normalize={normalizeExe}
      invalidMessage={isEn ? 'Enter a valid .exe filename' : '请输入合法的 .exe 进程名'}
      onchange={(next) => onUpdateAvoidance(cfg.suspend_hotkeys_in_fullscreen !== false, next)}
    />
  </div>
</div>

{#if probeResult}
  <div class="t-card probe-card {probeResult.is_free ? 'is-free-hit' : probeResult.found ? 'is-hit' : ''}">
    <div class="probe-head">
      <div class="probe-title">
        <strong>{isEn ? 'Differential Probe:' : '差分「抓现行」结果：'}</strong>
        <kbd class="t-kbd">{probeResult.spec}</kbd>
        {#if probeResult.is_free}
          <span class="pill is-ok">{isEn ? 'Verified Free' : '实测空闲'}</span>
        {:else if probeResult.confidence === 'confirmed'}
          <span class="pill is-tomoyo">{isEn ? 'Confirmed Owner' : '实锤窗口捕获'}</span>
        {:else if probeResult.confidence === 'likely'}
          <span class="pill is-occupied_external">{isEn ? 'CPU Spike Suspect' : '基线差分唤醒嫌疑'}</span>
        {:else}
          <span class="pill is-override">{isEn ? 'Silent / Driver Hold' : '静默底层占位'}</span>
        {/if}
      </div>
      {#if probeResult.found && probeResult.exe_path}
        <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'reveal_path', path: probeResult.exe_path })}>
          <FolderOpen size={13} />
          {isEn ? 'Locate Exe' : '定位程序目录'}
        </Button>
      {/if}
    </div>
    <div class="probe-body">
      <div>{probeResult.detail}</div>
      {#if probeResult.window_title}
        <div class="probe-meta">{isEn ? 'Window:' : '响应窗口：'} {probeResult.window_title}</div>
      {/if}
      {#if probeResult.exe_path}
        <div class="probe-meta mono">{probeResult.exe_path}</div>
      {/if}
    </div>
  </div>
{/if}

<div class="t-card sec-card" style="margin-top:14px">
  <div class="mat-bar">
    <div>
      <div class="t-label">{isEn ? 'System Shortcut Occupancy Matrix' : '系统高频热键全景占用矩阵'}</div>
      <div class="t-desc">
        {isEn
          ? 'Side-effect-free Win32 probe + 3-stage baseline-calibrated culprit finder (filters out live wallpapers & OS daemons).'
          : '零副作用扫描 + 三阶段基线校准抓现行（自动过滤动态壁纸与系统服务噪音）。'}
      </div>
    </div>
    <Segmented
      value={filter}
      options={[
        { value: 'all', label: isEn ? 'All' : '全部' },
        { value: 'busy', label: isEn ? 'Occupied' : '已占用' },
        { value: 'free', label: isEn ? 'Free' : '空闲' },
      ]}
      onchange={(v) => (filter = v)}
    />
  </div>

  <div class="custom-probe">
    <input
      class="t-input probe-inp"
      placeholder={isEn ? 'Probe any shortcut (e.g. Alt+C, Ctrl+Alt+V, F1)…' : '输入任意组合键实测是否空闲或抓占用真凶（如 Alt+C、Ctrl+Alt+V）…'}
      bind:value={customSpec}
      onkeydown={(e) => e.key === 'Enter' && runCustomProbe()}
    />
    <Button variant="secondary" size="sm" disabled={!customSpec.trim() || !!probingSpec} onclick={runCustomProbe}>
      <Crosshair size={13} />
      {probingSpec && probingSpec === customSpec.trim() ? (isEn ? 'Probing…' : '探测中…') : (isEn ? 'Probe Key' : '探测此键')}
    </Button>
  </div>

  {#if selectedSuspect}
    <SuspectDetailCard
      spec={selectedSuspect.spec}
      suspect={selectedSuspect.suspect}
      {isEn}
      onClose={() => (selectedSuspect = null)}
    />
  {/if}

  {#if auditing && !audit}
    <div class="t-skeleton" style="height:120px"></div>
  {:else}
    <div class="mat-list">
      {#each filteredMatrix as item}
        <div class="mat-row">
          <div class="mat-key"><kbd class="t-kbd">{item.spec}</kbd></div>
          <div><span class="pill is-{item.status}">{statusLabel(item.status)}</span></div>
          <div class="mat-sus">
            {#if item.suspects.length > 0}
              {#each item.suspects as sus}
                <button
                  type="button"
                  class="sus-tag"
                  title={isEn ? 'Click to view remediation & process actions' : '点击查看解除冲突指引、定位程序或结束进程'}
                  onclick={() => (selectedSuspect = { spec: item.spec, suspect: sus })}
                >
                  {sus.process_name} · {sus.reason}
                </button>
              {/each}
            {:else if item.status === 'overridden'}
              <span class="t-desc">{isEn ? `Tomoyo (${item.tomoyo_slot}) rescued over silent external hold` : `Tomoyo (${item.tomoyo_slot}) 已自动高优接管 · 可点右侧「探测」追踪外部占位`}</span>
            {:else if item.status === 'conflict' || item.status === 'occupied_external'}
              <span class="t-desc">{isEn ? 'Silent external hold — click Probe' : '外部程序占用 · 可点右侧「探测」抓现行'}</span>
            {:else if item.tomoyo_slot}
              <span class="t-desc">Tomoyo ({item.tomoyo_slot})</span>
            {:else}
              <span class="t-desc">{isEn ? 'Available for binding' : '空闲，可直接绑定'}</span>
            {/if}
          </div>
          <div class="mat-act">
            {#if item.status === 'conflict' || item.status === 'occupied_external' || item.status === 'overridden'}
              <Button variant="ghost" size="sm" disabled={!!probingSpec} onclick={() => onProbe(item.spec)}>
                <Crosshair size={12} />
                {probingSpec === item.spec ? '…' : (isEn ? 'Probe' : '探测')}
              </Button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .kpi-strip { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; margin-bottom: 14px; }
  .kpi-pill {
    display: flex; align-items: center; justify-content: space-between; padding: 9px 12px;
    border-radius: 9px; background: var(--surface); border: 1px solid var(--border); font-size: 12px; color: var(--muted);
  }
  .kpi-pill strong { font-size: 15px; color: var(--text); font-variant-numeric: tabular-nums; }
  .kpi-pill.is-ov strong { color: #7c3aed; }
  .kpi-pill.is-bad strong { color: var(--danger); }
  .kpi-pill.is-ok strong { color: var(--ok); }
  .sec-card { padding: 14px 16px; }
  .probe-card { margin-top: 14px; padding: 14px 16px; border-color: var(--border-strong); }
  .probe-card.is-hit { border-color: var(--accent); background: var(--accent-soft); }
  .probe-card.is-free-hit { border-color: var(--ok); background: var(--ok-soft); }
  .probe-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 6px; flex-wrap: wrap; }
  .probe-title { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .probe-body { font-size: 12.5px; display: flex; flex-direction: column; gap: 3px; }
  .probe-meta { font-size: 11.5px; color: var(--text-secondary); word-break: break-all; }
  .mat-bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; flex-wrap: wrap; }
  .custom-probe { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; }
  .probe-inp { flex: 1; font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; }
  .mat-list { display: flex; flex-direction: column; }
  .mat-row { display: grid; grid-template-columns: 128px 94px 1fr 74px; gap: 10px; align-items: center; padding: 8px 0; border-top: 1px solid var(--border); font-size: 12px; }
  .mat-sus { display: flex; flex-wrap: wrap; gap: 5px; min-width: 0; }
  .mat-act { display: flex; justify-content: flex-end; }
  .sus-tag { border: 1px solid var(--border); background: var(--canvas); color: var(--text-secondary); border-radius: 6px; padding: 2px 7px; font-family: inherit; font-size: 11.5px; cursor: pointer; }
  .sus-tag:hover { border-color: var(--accent); color: var(--accent); }
  .pill { display: inline-flex; align-items: center; gap: 4px; padding: 2px 8px; border-radius: 999px; font-size: 11px; font-weight: 500; white-space: nowrap; }
  .pill.is-ok, .pill.is-free { background: var(--ok-soft); color: var(--ok); }
  .pill.is-override, .pill.is-overridden { background: rgba(124, 58, 237, 0.12); color: #7c3aed; }
  .pill.is-bad, .pill.is-conflict { background: var(--danger-soft); color: var(--danger); }
  .pill.is-occupied_external { background: var(--warn-soft); color: var(--warn); }
  .pill.is-tomoyo { background: var(--accent-soft); color: var(--accent); }
  .avoid-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding-bottom: 10px; }
  .avoid-col { display: flex; flex-direction: column; gap: 8px; padding-top: 10px; border-top: 1px solid var(--border); }
</style>
