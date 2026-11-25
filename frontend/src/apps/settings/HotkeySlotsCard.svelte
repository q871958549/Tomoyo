<script lang="ts">
  import { AlertTriangle, CheckCircle2, Crosshair, ShieldAlert, Zap } from '@lucide/svelte';
  import { tr, type Lang } from '../../lib/i18n';
  import type {
    HotkeyAuditReport,
    HotkeySlotStatus,
    SelectOption,
    SuspectProcess,
    TomoyoConfig,
  } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  type SlotId = 'main' | 'clipboard' | 'translate' | 'capture' | 'pin' | 'ocr' | 'record';
  interface SlotEntry {
    id: SlotId;
    label: string;
    spec: string;
    st?: HotkeySlotStatus;
    presets: SelectOption[];
  }

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    slots: SlotEntry[];
    audit: HotkeyAuditReport | null;
    probingSpec: string;
    onApplyFreeKey: (slot: SlotId, key: string) => void;
    onProbe: (spec: string) => void;
    onToggleOverride: (v: boolean) => void;
    onSelectSuspect: (spec: string, suspect: SuspectProcess) => void;
  }

  let {
    cfg, lang, slots, audit, probingSpec,
    onApplyFreeKey, onProbe, onToggleOverride, onSelectSuspect,
  }: Props = $props();

  let isEn = $derived(lang === 'en');

  function suspectsForSpec(spec: string): SuspectProcess[] {
    if (!spec || !audit) return [];
    return audit.items.find((it) => it.spec.toLowerCase() === spec.toLowerCase())?.suspects || [];
  }
</script>

<div class="t-card sec-card">
  <div class="sec-title">
    <ShieldAlert size={14} />
    <span>{isEn ? 'Tomoyo Core Hotkeys · Direct Edit, Auto-Rescue & Free Replacements' : 'Tomoyo 核心热键管理 · 点击直接修改、自动接管与一键换空闲键'}</span>
  </div>

  {#each slots as s}
    {@const isBad = !!s.spec && !!s.st && !s.st.disabled && !s.st.ok}
    {@const isOv = !!s.spec && !!s.st?.ok && !!s.st?.overridden}
    {@const freeList = audit?.recommended_free?.[s.id] || []}
    {@const suspects = (isBad || isOv) ? suspectsForSpec(s.spec) : []}
    <div class="slot-row">
      <div class="slot-head">
        <div class="slot-info">
          <div class="slot-top">
            <span class="t-label">{s.label}</span>
            {#if isBad}
              <span class="pill is-bad"><AlertTriangle size={11} /> {s.st?.reason || tr(lang, 'hk_conflict')}</span>
            {:else if isOv}
              <span class="pill is-override"><Zap size={11} /> {isEn ? 'Auto-Rescued' : '已高优接管'}</span>
            {:else if s.spec && s.st?.ok}
              <span class="pill is-ok"><CheckCircle2 size={11} /> {tr(lang, 'hk_ok')}</span>
            {/if}
          </div>
          <div class="t-desc">
            {isEn
              ? 'Click the key box to press any shortcut directly, or choose from presets.'
              : '点击右侧热键框直接按键录入，或从快捷预设切换常用组合键。'}
          </div>
        </div>

        <div class="slot-ctrl">
          <HotkeyRecorder
            value={s.spec}
            presets={s.presets}
            allowDisable={s.id !== 'main'}
            presetPlaceholder={tr(lang, 'preset_opt')}
            disabledLabel={tr(lang, 'hk_off')}
            okLabel={tr(lang, 'hk_ok')}
            conflictLabel={tr(lang, 'hk_conflict')}
            recordPrompt={tr(lang, 'rec_prompt')}
            needModPrompt={tr(lang, 'rec_need_mod')}
            onchange={(next) => onApplyFreeKey(s.id, next)}
          />
        </div>
      </div>

      {#if isBad || isOv}
        <div class="slot-remedy">
          <div class="remedy-meta">
            {#if suspects.length > 0}
              <div class="rec-line">
                <span class="rec-hint">{isEn ? 'Suspected holder:' : '疑似外部占用：'}</span>
                {#each suspects as sus}
                  <button type="button" class="sus-tag" onclick={() => onSelectSuspect(s.spec, sus)}>
                    {sus.process_name} · {isEn ? 'How to fix' : '查看解除方案'}
                  </button>
                {/each}
              </div>
            {/if}
            {#if freeList.length > 0}
              <div class="rec-line">
                <span class="rec-hint">{isEn ? 'Zero-conflict free keys:' : '实测空闲可一键换无冲突键：'}</span>
                {#each freeList as fk}
                  <button type="button" class="free-chip" onclick={() => onApplyFreeKey(s.id, fk)}>{fk}</button>
                {/each}
              </div>
            {/if}
          </div>
          {#if s.spec}
            <div class="slot-acts">
              {#if isBad}
                <Button variant="primary" size="sm" class="btn-rescue-{s.id}" onclick={() => onToggleOverride(true)}>
                  <Zap size={12} />
                  {isEn ? 'Rescue Now' : '一键接管'}
                </Button>
              {/if}
              <Button variant="secondary" size="sm" disabled={!!probingSpec} onclick={() => onProbe(s.spec)}>
                <Crosshair size={13} />
                {probingSpec === s.spec ? (isEn ? 'Probing…' : '基线探测中…') : (isEn ? 'Catch Owner' : '抓现行')}
              </Button>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/each}

  <div class="slot-row override-row">
    <div class="slot-info">
      <div class="slot-top">
        <Zap size={14} class="ov-ico" />
        <span class="t-label">{isEn ? 'Auto High-Priority Conflict Rescue' : '高优先级强行接管冲突热键'}</span>
      </div>
      <div class="t-desc">
        {isEn
          ? 'Claims conflicted global shortcuts cleanly (or mounts a rescue hook) when enabled; falls back without overriding when off.'
          : '开启时优先尝试从冲突软件（如 KRunner / 系统快捷键）接管绑定；关闭时保留外部占用并自动回退或仅告警。'}
      </div>
    </div>
    <Switch checked={!!cfg.override_conflict_hotkeys} onchange={onToggleOverride} />
  </div>
</div>

<style>
  .sec-card { padding: 14px 16px; }
  .sec-title { display: flex; align-items: center; gap: 7px; font-size: 13px; font-weight: 600; margin-bottom: 10px; }
  .slot-row { display: flex; flex-direction: column; gap: 10px; padding: 12px 0; border-top: 1px solid var(--border); }
  .slot-head { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
  .slot-info { display: flex; flex-direction: column; gap: 3px; min-width: 180px; flex: 1; }
  .slot-top { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .slot-ctrl { display: flex; align-items: center; flex-shrink: 0; }
  .slot-remedy {
    display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap;
    padding: 9px 11px; border-radius: 8px; background: var(--canvas); border: 1px solid var(--border);
  }
  .remedy-meta { display: flex; flex-direction: column; gap: 6px; min-width: 0; flex: 1; }
  .slot-acts { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .rec-line { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .rec-hint { font-size: 11.5px; color: var(--muted); }
  .free-chip {
    padding: 2px 8px; border-radius: 6px; border: 1px solid var(--border);
    background: var(--surface); color: var(--accent); font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 11.5px; font-weight: 500; cursor: pointer;
  }
  .free-chip:hover { border-color: var(--accent); background: var(--accent-soft); }
  .sus-tag { border: 1px solid var(--border); background: var(--surface); color: var(--text-secondary); border-radius: 6px; padding: 2px 7px; font-family: inherit; font-size: 11.5px; cursor: pointer; }
  .sus-tag:hover { border-color: var(--accent); color: var(--accent); }
  .pill { display: inline-flex; align-items: center; gap: 4px; padding: 2px 8px; border-radius: 999px; font-size: 11px; font-weight: 500; white-space: nowrap; }
  .pill.is-ok { background: var(--ok-soft); color: var(--ok); }
  .pill.is-override { background: rgba(124, 58, 237, 0.12); color: #7c3aed; }
  .pill.is-bad { background: var(--danger-soft); color: var(--danger); }
  .override-row {
    flex-direction: row; align-items: center; justify-content: space-between; gap: 14px;
    background: var(--canvas); margin: 4px -16px -14px; padding: 12px 16px; border-radius: 0 0 10px 10px;
  }
  :global(.ov-ico) { color: #7c3aed; flex-shrink: 0; }
</style>
