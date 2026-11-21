<script lang="ts">
  import { Check } from '@lucide/svelte';
  import type { ContextMenuItem } from '../types';

  interface Props {
    open: boolean;
    x: number;
    y: number;
    items: ContextMenuItem[];
    onselect: (id: string) => void;
    onclose: () => void;
  }

  let { open, x, y, items, onselect, onclose }: Props = $props();
  let menuEl: HTMLDivElement | undefined = $state();

  let clampedPos = $derived.by(() => {
    const mw = 204;
    const mh = items.length * 32 + 16;
    const vw = typeof window !== 'undefined' ? window.innerWidth : 1024;
    const vh = typeof window !== 'undefined' ? window.innerHeight : 768;
    return {
      left: Math.max(6, Math.min(x, vw - mw - 8)),
      top: Math.max(6, Math.min(y, vh - mh - 8)),
    };
  });

  function onWinDown(e: MouseEvent) {
    if (open && menuEl && !menuEl.contains(e.target as Node)) {
      onclose();
    }
  }

  function onWinKey(e: KeyboardEvent) {
    if (open && e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
</script>

<svelte:window onmousedown={onWinDown} onkeydown={onWinKey} />

{#if open}
  <div
    bind:this={menuEl}
    role="menu"
    class="t-ctx-menu"
    style:left="{clampedPos.left}px"
    style:top="{clampedPos.top}px"
  >
    {#each items as item}
      {#if item.separator}
        <div class="t-ctx-sep" role="separator"></div>
      {:else}
        <button
          type="button"
          role="menuitem"
          disabled={item.disabled}
          class="t-ctx-item {item.danger ? 'is-danger' : ''}"
          onclick={() => {
            if (!item.disabled) {
              onselect(item.id);
              onclose();
            }
          }}
        >
          <span class="t-ctx-slot">
            {#if item.checked}<Check size={13} />{/if}
          </span>
          <span class="t-ctx-label">{item.label}</span>
          {#if item.shortcut}
            <span class="t-ctx-sc">{item.shortcut}</span>
          {/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .t-ctx-menu {
    position: fixed;
    z-index: 100;
    min-width: 196px;
    max-width: 300px;
    padding: 4px;
    border-radius: 10px;
    background: var(--elevated);
    border: 1px solid var(--border);
    box-shadow: var(--shadow-pop);
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .t-ctx-sep {
    height: 1px;
    background: var(--border);
    margin: 4px 8px;
  }
  .t-ctx-item {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    font-size: 12.5px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
    outline: none;
    transition: background-color 0.1s ease, color 0.1s ease;
  }
  .t-ctx-item:hover:not(:disabled) {
    background: var(--hover);
  }
  .t-ctx-item.is-danger {
    color: var(--danger);
  }
  .t-ctx-item.is-danger:hover:not(:disabled) {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .t-ctx-item:disabled {
    opacity: 0.42;
    cursor: not-allowed;
  }
  .t-ctx-slot {
    width: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
    flex-shrink: 0;
  }
  .t-ctx-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t-ctx-sc {
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 11px;
    color: var(--muted);
    margin-left: 12px;
  }
</style>
