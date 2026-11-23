<script lang="ts">
  import { Check, ChevronDown } from '@lucide/svelte';
  import type { SelectOption } from '../types';

  interface Props {
    value?: string;
    options: SelectOption[];
    placeholder?: string;
    size?: 'sm' | 'md';
    disabled?: boolean;
    searchable?: boolean;
    align?: 'left' | 'right';
    minWidth?: string;
    class?: string;
    onchange?: (val: string) => void;
  }

  let {
    value = $bindable(''),
    options,
    placeholder = '请选择…',
    size = 'md',
    disabled = false,
    searchable = false,
    align = 'left',
    minWidth = '128px',
    class: extraClass = '',
    onchange,
  }: Props = $props();

  let open = $state(false);
  let activeIdx = $state(-1);
  let filterQ = $state('');
  let rootEl: HTMLDivElement | undefined = $state();

  let filteredOpts = $derived.by(() => {
    const q = searchable ? filterQ.trim().toLowerCase() : '';
    return q ? options.filter((o) => o.label.toLowerCase().includes(q) || (o.hint || '').toLowerCase().includes(q)) : options;
  });

  let selectedLabel = $derived(
    options.find((o) => o.value === value)?.label ?? placeholder
  );

  function toggleOpen() {
    if (disabled) return;
    open = !open;
    if (open) {
      filterQ = '';
      const idx = filteredOpts.findIndex((o) => o.value === value);
      activeIdx = idx >= 0 ? idx : 0;
    }
  }

  function pick(val: string) {
    value = val;
    open = false;
    filterQ = '';
    onchange?.(val);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!open) {
      if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        toggleOpen();
      }
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      open = false;
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      activeIdx = (activeIdx + 1) % Math.max(1, filteredOpts.length);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      activeIdx = activeIdx <= 0 ? filteredOpts.length - 1 : activeIdx - 1;
    } else if (e.key === 'Enter' && filteredOpts[activeIdx]) {
      e.preventDefault();
      pick(filteredOpts[activeIdx].value);
    }
  }

  function onWinMouseDown(e: MouseEvent) {
    if (open && rootEl && !rootEl.contains(e.target as Node)) {
      open = false;
    }
  }
</script>

<svelte:window onmousedown={onWinMouseDown} />

<div bind:this={rootEl} class="t-select-root {extraClass}" style:min-width={minWidth}>
  <button
    type="button"
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    class="t-select-trigger t-select-{size} {open ? 'is-open' : ''}"
    onclick={toggleOpen}
    onkeydown={handleKeydown}
  >
    <span class="t-select-val {!options.some((o) => o.value === value) ? 'is-ph' : ''}">
      {selectedLabel}
    </span>
    <span class="t-select-chev {open ? 'is-open' : ''}">
      <ChevronDown size={14} />
    </span>
  </button>

  {#if open}
    <div class="t-select-menu {align === 'right' ? 'align-right' : ''}" role="listbox">
      {#if searchable}
        <input class="t-sel-q" type="text" placeholder="搜索…" bind:value={filterQ} onkeydown={handleKeydown} />
      {/if}
      {#each filteredOpts as opt, i}
        <button
          type="button"
          role="option"
          data-opt={opt.value}
          aria-selected={opt.value === value}
          class="t-select-opt {opt.value === value ? 'is-selected' : ''} {i === activeIdx ? 'is-focus' : ''}"
          onmouseenter={() => (activeIdx = i)}
          onclick={() => pick(opt.value)}
        >
          <span class="t-opt-text">
            <span>{opt.label}</span>
            {#if opt.hint}<span class="t-opt-hint">{opt.hint}</span>{/if}
          </span>
          {#if opt.value === value}
            <span class="t-opt-chk"><Check size={14} /></span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .t-select-root {
    position: relative;
    display: inline-block;
    flex-shrink: 0;
  }
  .t-select-trigger {
    width: 100%;
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 0 9px 0 10px;
    font-family: inherit;
    font-size: 13px;
    cursor: pointer;
    outline: none;
    white-space: nowrap;
    transition: border-color 0.12s ease, box-shadow 0.12s ease, background-color 0.12s ease;
  }
  .t-select-md { height: 34px; }
  .t-select-sm { height: 28px; font-size: 12px; }
  .t-select-trigger:hover:not(:disabled) {
    border-color: var(--border-strong);
    background: var(--subtle);
  }
  .t-select-trigger.is-open,
  .t-select-trigger:focus-visible {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--ring);
    background: var(--surface);
  }
  .t-select-trigger:disabled { opacity: 0.45; cursor: not-allowed; }
  .t-select-val { overflow: hidden; text-overflow: ellipsis; }
  .t-select-val.is-ph { color: var(--muted); }
  .t-select-chev {
    display: inline-flex;
    color: var(--muted);
    transition: transform 0.14s ease;
    flex-shrink: 0;
  }
  .t-select-chev.is-open { transform: rotate(180deg); }

  .t-select-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    min-width: 100%;
    max-height: 240px;
    overflow-y: auto;
    z-index: 60;
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow-pop);
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .t-select-menu.align-right { left: auto; right: 0; }
  .t-select-opt {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-height: 30px;
    padding: 5px 8px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    font-size: 12.5px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .t-select-opt.is-focus { background: var(--hover); }
  .t-select-opt.is-selected { color: var(--accent); font-weight: 500; }
  .t-opt-text { display: flex; align-items: center; gap: 6px; }
  .t-opt-hint { font-size: 11px; color: var(--muted); font-weight: 400; }
  .t-opt-chk { display: inline-flex; color: var(--accent); flex-shrink: 0; }
  .t-sel-q {
    width: 100%; height: 28px; margin-bottom: 3px; padding: 0 8px;
    border-radius: 6px; border: 1px solid var(--border); background: var(--surface);
    color: var(--text); font-family: inherit; font-size: 12px; outline: none;
  }
  .t-sel-q:focus { border-color: var(--accent); }
</style>
