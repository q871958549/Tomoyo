<script lang="ts">
  import { onDestroy } from 'svelte';
  import { Crosshair, Keyboard, Pencil, X } from '@lucide/svelte';
  import { buildComboFromEvent } from '../../lib/hotkeys';
  import type { Lang } from '../../lib/i18n';
  import { postHost, subscribeHost } from '../../lib/ipc';
  import Button from '../../lib/ui/Button.svelte';

  interface Props {
    lang: Lang;
    probingSpec: string;
    onProbe: (spec: string) => void;
  }

  let { lang, probingSpec, onProbe }: Props = $props();

  let isEn = $derived(lang === 'en');
  let mode = $state<'capture' | 'text'>('capture');
  let spec = $state('');
  let recording = $state(false);
  let hint = $state('');
  let boxEl: HTMLDivElement | undefined = $state();
  let inputEl: HTMLInputElement | undefined = $state();
  let unsubHost: (() => void) | null = null;

  let parts = $derived(spec.trim() ? spec.trim().split('+').map((p) => p.trim()).filter(Boolean) : []);
  let canProbe = $derived(!!spec.trim() && !probingSpec);
  let isProbingThis = $derived(!!probingSpec && probingSpec.toLowerCase() === spec.trim().toLowerCase());

  function stopCapture() {
    if (!recording) return;
    recording = false;
    hint = '';
    unsubHost?.();
    unsubHost = null;
    postHost({ cmd: 'stop_hotkey_capture' });
  }

  function startCapture() {
    if (mode !== 'capture' || recording) return;
    recording = true;
    hint = isEn
      ? 'Listening… press any shortcut (Esc to cancel)'
      : '独占监听中，请直接按下任意组合键（Esc 取消）…';
    boxEl?.focus();
    unsubHost?.();
    unsubHost = subscribeHost((data) => {
      if (!recording || data.event !== 'hotkey_captured') return;
      if (data.cancelled) {
        stopCapture();
      } else if (data.need_mod) {
        hint = isEn
          ? 'Add Ctrl / Alt / Shift / Win (F1–F12 / PrintScreen can be standalone)'
          : '普通键需加修饰键（F1–F12 / PrintScreen 可单键）';
      } else if (typeof data.combo === 'string' && data.combo) {
        spec = data.combo;
        stopCapture();
      }
    });
    postHost({ cmd: 'start_hotkey_capture' });
  }

  function toggleMode(e: MouseEvent) {
    e.stopPropagation();
    stopCapture();
    mode = mode === 'capture' ? 'text' : 'capture';
    if (mode === 'text') {
      queueMicrotask(() => inputEl?.focus());
    }
  }

  function clearSpec(e: MouseEvent) {
    e.stopPropagation();
    stopCapture();
    spec = '';
  }

  function runProbe() {
    stopCapture();
    const clean = spec.trim();
    if (!clean || probingSpec) return;
    onProbe(clean);
  }

  function onCaptureKeyDown(e: KeyboardEvent) {
    if (!recording) {
      if (e.key === 'Enter' && spec.trim()) {
        e.preventDefault();
        runProbe();
      } else if (e.key === ' ') {
        e.preventDefault();
        startCapture();
      } else if ((e.key === 'Backspace' || e.key === 'Delete') && spec) {
        e.preventDefault();
        spec = '';
      }
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      stopCapture();
      return;
    }
    const res = buildComboFromEvent(e);
    if (res.needMod) {
      hint = isEn
        ? 'Add Ctrl / Alt / Shift / Win (F1–F12 / PrintScreen can be standalone)'
        : '普通键需加修饰键（F1–F12 / PrintScreen 可单键）';
      return;
    }
    if (res.combo) {
      spec = res.combo;
      stopCapture();
    }
  }

  onDestroy(() => {
    if (recording) postHost({ cmd: 'stop_hotkey_capture' });
    unsubHost?.();
  });
</script>

<div class="probe-bar">
  {#if mode === 'capture'}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      bind:this={boxEl}
      role="button"
      tabindex="0"
      class="probe-box is-capture {recording ? 'is-rec' : ''}"
      title={isEn ? 'Click and press any shortcut to probe' : '点击后直接按下任意快捷键实测'}
      onclick={startCapture}
      onblur={stopCapture}
      onkeydown={onCaptureKeyDown}
    >
      {#if recording}
        <span class="probe-dot" aria-hidden="true"></span>
        <span class="probe-rec-txt">{hint}</span>
      {:else}
        <span class="probe-ico" aria-hidden="true"><Keyboard size={14} /></span>
        {#if parts.length > 0}
          <span class="probe-keys">
            {#each parts as p, idx}
              {#if idx > 0}<span class="probe-plus">+</span>{/if}
              <kbd class="t-kbd probe-cap">{p}</kbd>
            {/each}
          </span>
          <span class="probe-ready">{isEn ? 'Press Enter or click Probe' : '按 Enter 或点右侧探测'}</span>
        {:else}
          <span class="probe-ph">
            {isEn
              ? 'Click and press any shortcut to test (supports F1–F12, PrintScreen, occupied keys)…'
              : '点击此处直接按下任意组合键实测是否空闲或抓占用真凶（支持已被占用键）…'}
          </span>
        {/if}
      {/if}

      <div class="probe-tools">
        {#if spec && !recording}
          <button type="button" class="probe-clr" title={isEn ? 'Clear' : '清空'} onclick={clearSpec}>
            <X size={13} />
          </button>
        {/if}
        <button
          type="button"
          class="mode-btn"
          title={isEn ? 'Switch to manual text input' : '切换为手动输入热键文本'}
          onclick={toggleMode}
        >
          <Pencil size={11} />
          <span>{isEn ? 'Type' : '手输'}</span>
        </button>
      </div>
    </div>
  {:else}
    <div class="probe-box is-text">
      <span class="probe-ico" aria-hidden="true"><Pencil size={13} /></span>
      <input
        bind:this={inputEl}
        type="text"
        class="probe-inp"
        autocomplete="off"
        spellcheck="false"
        placeholder={isEn ? 'Type shortcut (e.g. Alt+C, Ctrl+Alt+V, F1)…' : '手动输入组合键（如 Alt+C、Ctrl+Alt+V、F1）…'}
        bind:value={spec}
        onkeydown={(e) => e.key === 'Enter' && runProbe()}
      />
      <div class="probe-tools">
        {#if spec}
          <button type="button" class="probe-clr" title={isEn ? 'Clear' : '清空'} onclick={clearSpec}>
            <X size={13} />
          </button>
        {/if}
        <button
          type="button"
          class="mode-btn"
          title={isEn ? 'Switch to press-to-capture' : '切换为直接按键捕获'}
          onclick={toggleMode}
        >
          <Keyboard size={11} />
          <span>{isEn ? 'Press' : '按键'}</span>
        </button>
      </div>
    </div>
  {/if}

  <Button variant={canProbe ? 'primary' : 'secondary'} size="md" disabled={!canProbe} onclick={runProbe}>
    <Crosshair size={13} />
    {isProbingThis ? (isEn ? 'Probing…' : '探测中…') : (isEn ? 'Probe Key' : '探测此键')}
  </Button>
</div>

<style>
  .probe-bar { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; }
  .probe-box {
    flex: 1; min-width: 0; height: 34px; padding: 0 6px 0 10px;
    border-radius: 7px; border: 1px solid var(--border); background: var(--canvas);
    display: flex; align-items: center; gap: 8px; outline: none;
    transition: border-color 0.14s ease, box-shadow 0.14s ease, background-color 0.14s ease;
  }
  .probe-box.is-capture { cursor: pointer; user-select: none; }
  .probe-box:hover { border-color: var(--border-strong); background: var(--surface); }
  .probe-box:focus-within, .probe-box.is-capture:focus-visible {
    background: var(--surface); border-color: var(--accent); box-shadow: 0 0 0 3px var(--ring);
  }
  .probe-box.is-rec {
    background: var(--accent-soft); border-color: var(--accent); box-shadow: 0 0 0 3px var(--ring);
  }
  .probe-ico { display: inline-flex; align-items: center; color: var(--muted); flex-shrink: 0; }
  .probe-dot {
    width: 7px; height: 7px; border-radius: 999px; background: var(--accent); flex-shrink: 0;
    animation: probe-pulse 1.1s ease-in-out infinite;
  }
  @keyframes probe-pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.4; transform: scale(0.8); }
  }
  .probe-rec-txt {
    flex: 1; min-width: 0; font-size: 12px; color: var(--accent); font-weight: 500;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .probe-ph {
    flex: 1; min-width: 0; font-size: 12.5px; color: var(--muted);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .probe-keys { display: inline-flex; align-items: center; gap: 4px; flex-shrink: 0; }
  .probe-cap { min-width: 22px; text-align: center; box-shadow: 0 1px 0 var(--border-strong); }
  .probe-plus { font-size: 11px; color: var(--muted); font-weight: 500; }
  .probe-ready {
    flex: 1; min-width: 0; font-size: 11.5px; color: var(--muted); margin-left: 4px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .probe-inp {
    flex: 1; min-width: 0; height: 100%; border: 0; outline: none; background: transparent;
    color: var(--text); font-family: var(--font-mono); font-size: 12.5px;
  }
  .probe-inp::placeholder { color: var(--muted); font-family: inherit; }
  .probe-tools { display: inline-flex; align-items: center; gap: 4px; flex-shrink: 0; margin-left: auto; }
  .probe-clr {
    display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px;
    border: 0; border-radius: 5px; background: transparent; color: var(--muted); cursor: pointer;
  }
  .probe-clr:hover { background: var(--hover); color: var(--text); }
  .mode-btn {
    display: inline-flex; align-items: center; gap: 4px; height: 22px; padding: 0 7px;
    border-radius: 5px; border: 1px solid var(--border); background: var(--surface);
    color: var(--text-secondary); font-family: inherit; font-size: 11px; font-weight: 500;
    cursor: pointer; white-space: nowrap; transition: border-color 0.12s ease, color 0.12s ease, background-color 0.12s ease;
  }
  .mode-btn:hover { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
</style>
