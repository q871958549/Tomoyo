<script lang="ts">
  import { onDestroy } from 'svelte';
  import { AlertTriangle, CheckCircle2, Keyboard, X, Zap } from '@lucide/svelte';
  import { buildComboFromEvent } from '../hotkeys';
  import { postHost, subscribeHost } from '../ipc';
  import type { HotkeySlotStatus, SelectOption } from '../types';
  import Select from './Select.svelte';

  interface Props {
    value: string;
    presets: SelectOption[];
    allowDisable?: boolean;
    status?: HotkeySlotStatus;
    presetPlaceholder?: string;
    disabledLabel?: string;
    okLabel?: string;
    conflictLabel?: string;
    recordPrompt?: string;
    needModPrompt?: string;
    onchange: (next: string) => void;
    onJumpDoctor?: () => void;
  }

  let {
    value,
    presets,
    allowDisable = false,
    status = undefined,
    presetPlaceholder = '预设',
    disabledLabel = '已停用',
    okLabel = '已生效',
    conflictLabel = '已被其他软件占用',
    recordPrompt = '独占捕获中，请按键…',
    needModPrompt = '普通键需加修饰键（F1–F12 / PrintScreen 可单键）',
    onchange,
    onJumpDoctor = undefined,
  }: Props = $props();

  let recording = $state(false);
  let promptMsg = $state('');
  let recEl: HTMLDivElement | undefined = $state();
  let unsubHost: (() => void) | null = null;

  let parts = $derived(value ? value.split('+').map((p) => p.trim()).filter(Boolean) : []);
  let hasClear = $derived(allowDisable && !!value && !recording);

  let presetOptions = $derived.by(() => {
    const base = [...presets];
    if (allowDisable) base.push({ value: '__off__', label: disabledLabel });
    return base;
  });

  let showConflict = $derived(
    !!value && !!status && !status.disabled && !status.ok && status.spec.toLowerCase() === value.toLowerCase()
  );
  let showOk = $derived(
    !!value && !!status && !status.disabled && status.ok && status.spec.toLowerCase() === value.toLowerCase()
  );
  let showOverridden = $derived(showOk && !!status?.overridden);

  function startRecording() {
    if (recording) return;
    recording = true;
    promptMsg = recordPrompt;
    recEl?.focus();
    unsubHost?.();
    unsubHost = subscribeHost((data) => {
      if (!recording || data.event !== 'hotkey_captured') return;
      if (data.cancelled) {
        stopRecording();
      } else if (data.need_mod) {
        promptMsg = needModPrompt;
      } else if (typeof data.combo === 'string' && data.combo) {
        const picked = data.combo;
        stopRecording();
        onchange(picked);
      }
    });
    postHost({ cmd: 'start_hotkey_capture' });
  }

  function stopRecording() {
    if (!recording) return;
    recording = false;
    promptMsg = '';
    unsubHost?.();
    unsubHost = null;
    postHost({ cmd: 'stop_hotkey_capture' });
  }

  onDestroy(() => {
    if (recording) postHost({ cmd: 'stop_hotkey_capture' });
    unsubHost?.();
  });

  function onKeyDown(e: KeyboardEvent) {
    if (!recording) {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        startRecording();
      }
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      stopRecording();
      return;
    }
    if (allowDisable && (e.key === 'Backspace' || e.key === 'Delete') && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
      stopRecording();
      onchange('');
      return;
    }
    const res = buildComboFromEvent(e);
    if (res.needMod) {
      promptMsg = needModPrompt;
      return;
    }
    if (res.combo) {
      const picked = res.combo;
      stopRecording();
      onchange(picked);
    }
  }

  function pickPreset(v: string) {
    stopRecording();
    onchange(v === '__off__' ? '' : v);
  }
</script>

<div class="t-hk-col">
  <div class="t-hk-row">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      bind:this={recEl}
      role="button"
      tabindex="0"
      title={recording ? '按 Esc 取消录入' : '点击后直接按下快捷键录入'}
      class="t-hk-box {recording ? 'is-rec' : ''} {showConflict ? 'is-conflict' : ''} {hasClear ? 'has-clear' : ''}"
      onclick={startRecording}
      onblur={stopRecording}
      onkeydown={onKeyDown}
    >
      {#if recording}
        <span class="t-hk-rec-wrap">
          <span class="t-hk-dot" aria-hidden="true"></span>
          <span class="t-hk-prompt">{promptMsg}</span>
        </span>
      {:else}
        <span class="t-hk-ico" aria-hidden="true"><Keyboard size={13} /></span>
        {#if parts.length > 0}
          <span class="t-hk-keys">
            {#each parts as p, idx}
              {#if idx > 0}<span class="t-hk-plus">+</span>{/if}
              <kbd class="t-kbd t-hk-cap">{p}</kbd>
            {/each}
          </span>
        {:else}
          <span class="t-hk-off">{disabledLabel}</span>
        {/if}
      {/if}

      {#if hasClear}
        <button
          type="button"
          class="t-hk-clr"
          title={disabledLabel}
          onclick={(e) => { e.stopPropagation(); onchange(''); }}
        >
          <X size={12} />
        </button>
      {/if}
    </div>

    <Select
      value=""
      options={presetOptions}
      placeholder={presetPlaceholder}
      align="right"
      minWidth="106px"
      onchange={pickPreset}
    />
  </div>

  {#if showConflict}
    <div class="t-hk-status-line">
      <div class="t-hk-badge is-conflict" title={status?.reason || conflictLabel}>
        <AlertTriangle size={12} />
        <span>{status?.reason || conflictLabel}</span>
      </div>
      {#if onJumpDoctor}
        <button type="button" class="t-hk-jump" onclick={onJumpDoctor}>定位真凶 →</button>
      {/if}
    </div>
  {:else if showOverridden}
    <div class="t-hk-badge is-override" title={status?.reason || ''}>
      <Zap size={12} />
      <span>高优先级接管</span>
    </div>
  {:else if showOk}
    <div class="t-hk-badge is-ok">
      <CheckCircle2 size={12} />
      <span>{okLabel}</span>
    </div>
  {/if}
</div>

<style>
  .t-hk-col { display: flex; flex-direction: column; align-items: flex-end; gap: 5px; flex-shrink: 0; }
  .t-hk-row { display: flex; align-items: center; gap: 8px; }
  .t-hk-box {
    position: relative; min-width: 186px; height: 34px; padding: 0 30px;
    border-radius: 7px; border: 1px solid var(--border); background: var(--canvas);
    display: inline-flex; align-items: center; justify-content: center;
    cursor: pointer; user-select: none; outline: none;
    transition: border-color 0.14s ease, box-shadow 0.14s ease, background-color 0.14s ease;
  }
  .t-hk-box:hover {
    border-color: var(--border-strong); background: var(--surface);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
  }
  .t-hk-box:focus-visible { border-color: var(--accent); box-shadow: 0 0 0 3px var(--ring); }
  .t-hk-box.is-rec {
    border-color: var(--accent); box-shadow: 0 0 0 3px var(--ring);
    background: var(--accent-soft); padding: 0 12px;
  }
  .t-hk-box.is-conflict { border-color: var(--danger); background: var(--danger-soft); }
  .t-hk-ico {
    position: absolute; left: 10px; display: inline-flex; align-items: center;
    color: var(--muted); opacity: 0.65; pointer-events: none; transition: color 0.14s ease, opacity 0.14s ease;
  }
  .t-hk-box:hover .t-hk-ico { color: var(--accent); opacity: 0.95; }
  .t-hk-rec-wrap { display: inline-flex; align-items: center; gap: 7px; }
  .t-hk-dot {
    width: 7px; height: 7px; border-radius: 999px; background: var(--accent); flex-shrink: 0;
    animation: hk-pulse 1.1s ease-in-out infinite;
  }
  @keyframes hk-pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.4; transform: scale(0.8); }
  }
  .t-hk-keys { display: inline-flex; align-items: center; gap: 4px; }
  .t-hk-cap { min-width: 22px; text-align: center; box-shadow: 0 1px 0 var(--border-strong); }
  .t-hk-plus { font-size: 11px; color: var(--muted); font-weight: 500; }
  .t-hk-prompt { font-size: 11.5px; color: var(--accent); font-weight: 500; white-space: nowrap; }
  .t-hk-off { font-size: 12.5px; color: var(--muted); }
  .t-hk-clr {
    position: absolute; right: 6px; display: inline-flex; align-items: center; justify-content: center;
    width: 20px; height: 20px; border: 0; border-radius: 5px; background: transparent;
    color: var(--muted); cursor: pointer; transition: background-color 0.12s ease, color 0.12s ease;
  }
  .t-hk-clr:hover { background: var(--hover); color: var(--text); }
  .t-hk-status-line { display: inline-flex; align-items: center; gap: 6px; flex-wrap: wrap; justify-content: flex-end; }
  .t-hk-badge {
    display: inline-flex; align-items: center; gap: 4px; font-size: 11.5px; font-weight: 500;
    padding: 2px 8px; border-radius: 999px; white-space: nowrap;
  }
  .t-hk-badge.is-ok { background: var(--ok-soft); color: var(--ok); }
  .t-hk-badge.is-override { background: rgba(124, 58, 237, 0.12); color: #7c3aed; }
  .t-hk-badge.is-conflict { background: var(--danger-soft); color: var(--danger); }
  .t-hk-jump {
    border: 0; background: transparent; color: var(--accent); font-family: inherit;
    font-size: 11.5px; font-weight: 500; cursor: pointer; padding: 2px 4px; white-space: nowrap;
  }
  .t-hk-jump:hover { text-decoration: underline; }
</style>
