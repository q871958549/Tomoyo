<script lang="ts">
  import { AlertTriangle, CheckCircle2, Crosshair, FolderOpen, RefreshCw, ShieldAlert } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type {
    HotkeyAuditReport,
    HotkeyHealthReport,
    LiveProbeResult,
    TomoyoConfig,
  } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    health: HotkeyHealthReport;
    audit: HotkeyAuditReport | null;
    auditing: boolean;
    probingSpec: string;
    probeResult: LiveProbeResult | null;
    onRunAudit: () => void;
    onProbe: (spec: string) => void;
    onApplyFreeKey: (slot: 'main' | 'clipboard' | 'translate' | 'capture', key: string) => void;
  }

  let {
    cfg,
    lang,
    health,
    audit,
    auditing,
    probingSpec,
    probeResult,
    onRunAudit,
    onProbe,
    onApplyFreeKey,
  }: Props = $props();

  let filter = $state('all');
  let isEn = $derived(lang === 'en');

  let slots = $derived([
    { id: 'main' as const, label: isEn ? 'Main Launcher' : '主启动器唤起', spec: cfg.hotkey, st: health.main },
    { id: 'clipboard' as const, label: isEn ? 'Clipboard Center' : '剪贴板管理中心', spec: cfg.clipboard.hotkey, st: health.clipboard },
    { id: 'translate' as const, label: isEn ? 'Selection Translate' : '全局划词翻译', spec: cfg.translate.hotkey, st: health.translate },
    { id: 'capture' as const, label: isEn ? 'Screenshot & Pin' : '截图与桌面贴图', spec: cfg.capture.hotkey, st: health.capture },
  ]);

  let filteredMatrix = $derived.by(() => {
    const list = audit?.items || [];
    if (filter === 'busy') return list.filter((i) => i.status === 'conflict' || i.status === 'occupied_external');
    if (filter === 'free') return list.filter((i) => i.status === 'free');
    return list;
  });

  function statusLabel(st: string): string {
    if (isEn) {
      if (st === 'conflict') return 'Conflict';
      if (st === 'occupied_external') return 'Occupied';
      if (st === 'tomoyo') return 'Tomoyo';
      return 'Free';
    }
    if (st === 'conflict') return '冲突被占';
    if (st === 'occupied_external') return '外部占用';
    if (st === 'tomoyo') return 'Tomoyo 已占';
    return '当前空闲';
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

<div class="t-card sec-card">
  <div class="sec-title">
    <ShieldAlert size={14} />
    <span>{isEn ? 'Tomoyo Core Hotkeys & One-Click Free Replacements' : 'Tomoyo 核心热键状态与一键换空闲键'}</span>
  </div>

  {#each slots as s}
    {@const isBad = !!s.spec && !!s.st && !s.st.disabled && !s.st.ok}
    {@const freeList = audit?.recommended_free?.[s.id] || []}
    <div class="slot-row">
      <div class="slot-main">
        <div class="slot-top">
          <span class="t-label">{s.label}</span>
          {#if s.spec}
            <kbd class="t-kbd">{s.spec}</kbd>
          {:else}
            <span class="t-desc">{tr(lang, 'hk_off')}</span>
          {/if}
          {#if isBad}
            <span class="pill is-bad"><AlertTriangle size={11} /> {s.st?.reason || tr(lang, 'hk_conflict')}</span>
          {:else if s.spec && s.st?.ok}
            <span class="pill is-ok"><CheckCircle2 size={11} /> {tr(lang, 'hk_ok')}</span>
          {/if}
        </div>
        {#if isBad && freeList.length > 0}
          <div class="rec-line">
            <span class="rec-hint">{isEn ? 'Verified free:' : '实测空闲可一键切换：'}</span>
            {#each freeList as fk}
              <button type="button" class="free-chip" onclick={() => onApplyFreeKey(s.id, fk)}>{fk}</button>
            {/each}
          </div>
        {/if}
      </div>
      {#if isBad && s.spec}
        <Button
          variant="secondary"
          size="sm"
          disabled={!!probingSpec}
          onclick={() => onProbe(s.spec)}
        >
          <Crosshair size={13} />
          {probingSpec === s.spec ? (isEn ? 'Probing…' : '探测中…') : (isEn ? 'Catch Owner' : '抓现行定位')}
        </Button>
      {/if}
    </div>
  {/each}
</div>

{#if probeResult}
  <div class="t-card probe-card {probeResult.found ? 'is-hit' : ''}">
    <div class="probe-head">
      <strong>
        {isEn ? 'Live Culprit Probe:' : '实测「抓现行」结果：'}
        <kbd class="t-kbd">{probeResult.spec}</kbd>
      </strong>
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
        {#if audit}
          {isEn
            ? `Probed ${audit.total_probed} shortcuts · ${audit.external_occupied} occupied externally · ${audit.conflicts} Tomoyo conflicts`
            : `已探测 ${audit.total_probed} 组热键 · 外部占用 ${audit.external_occupied} 组 · Tomoyo 冲突 ${audit.conflicts} 组（点击「探测」可模拟触发抓真凶）`}
        {:else}
          {isEn ? 'Running side-effect-free Win32 probe…' : '正在静默探测系统热键状态…'}
        {/if}
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

  {#if auditing && !audit}
    <div class="t-skeleton" style="height:120px"></div>
  {:else}
    <div class="mat-list">
      {#each filteredMatrix as item}
        <div class="mat-row">
          <div class="mat-key"><kbd class="t-kbd">{item.spec}</kbd></div>
          <div>
            <span class="pill is-{item.status}">{statusLabel(item.status)}</span>
          </div>
          <div class="mat-sus">
            {#if item.suspects.length > 0}
              {#each item.suspects as sus}
                <button
                  type="button"
                  class="sus-tag"
                  title={sus.exe_path || sus.reason}
                  onclick={() => sus.exe_path && postHost({ cmd: 'reveal_path', path: sus.exe_path })}
                >
                  {sus.process_name} · {sus.reason}
                </button>
              {/each}
            {:else if item.status === 'conflict' || item.status === 'occupied_external'}
              <span class="t-desc">{isEn ? 'Unknown owner — use Catch Probe' : '未知程序占用 · 可点右侧「探测」抓现行'}</span>
            {:else if item.tomoyo_slot}
              <span class="t-desc">Tomoyo ({item.tomoyo_slot})</span>
            {:else}
              <span class="t-desc">{isEn ? 'Available for binding' : '空闲，可直接绑定'}</span>
            {/if}
          </div>
          <div class="mat-act">
            {#if item.status === 'conflict' || item.status === 'occupied_external'}
              <Button
                variant="ghost"
                size="sm"
                disabled={!!probingSpec}
                onclick={() => onProbe(item.spec)}
              >
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
  .tab-head-bar { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; margin-bottom: 16px; }
  .tab-head-bar h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
  .tab-lead { margin: 0; font-size: 12.5px; color: var(--muted); }
  .sec-card { padding: 14px 16px; }
  .sec-title { display: flex; align-items: center; gap: 7px; font-size: 13px; font-weight: 600; margin-bottom: 10px; }
  .slot-row {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 10px 0; border-top: 1px solid var(--border);
  }
  .slot-main { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .slot-top { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .rec-line { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .rec-hint { font-size: 11.5px; color: var(--muted); }
  .free-chip {
    padding: 2px 8px; border-radius: 6px; border: 1px solid var(--border);
    background: var(--canvas); color: var(--accent); font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 11.5px; font-weight: 500; cursor: pointer;
  }
  .free-chip:hover { border-color: var(--accent); background: var(--accent-soft); }

  .probe-card { margin-top: 14px; padding: 14px 16px; border-color: var(--border-strong); }
  .probe-card.is-hit { border-color: var(--accent); background: var(--accent-soft); }
  .probe-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 6px; }
  .probe-body { font-size: 12.5px; display: flex; flex-direction: column; gap: 3px; }
  .probe-meta { font-size: 11.5px; color: var(--text-secondary); word-break: break-all; }
  .mono { font-family: "Cascadia Mono", Consolas, monospace; }

  .mat-bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 12px; flex-wrap: wrap; }
  .mat-list { display: flex; flex-direction: column; }
  .mat-row {
    display: grid; grid-template-columns: 128px 88px 1fr 78px; gap: 10px; align-items: center;
    padding: 8px 0; border-top: 1px solid var(--border); font-size: 12px;
  }
  .mat-sus { display: flex; flex-wrap: wrap; gap: 5px; min-width: 0; }
  .mat-act { display: flex; justify-content: flex-end; }
  .sus-tag {
    border: 1px solid var(--border); background: var(--canvas); color: var(--text-secondary);
    border-radius: 6px; padding: 2px 7px; font-family: inherit; font-size: 11.5px; cursor: pointer;
  }
  .sus-tag:hover { border-color: var(--accent); color: var(--accent); }
  .pill {
    display: inline-flex; align-items: center; gap: 4px; padding: 2px 8px;
    border-radius: 999px; font-size: 11px; font-weight: 500; white-space: nowrap;
  }
  .pill.is-ok, .pill.is-free { background: var(--ok-soft); color: var(--ok); }
  .pill.is-bad, .pill.is-conflict { background: var(--danger-soft); color: var(--danger); }
  .pill.is-occupied_external { background: var(--warn-soft); color: var(--warn); }
  .pill.is-tomoyo { background: var(--accent-soft); color: var(--accent); }
</style>
