<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger' | 'icon';
    size?: 'sm' | 'md' | 'lg';
    active?: boolean;
    disabled?: boolean;
    type?: 'button' | 'submit' | 'reset';
    title?: string;
    class?: string;
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
  }

  let {
    variant = 'secondary',
    size = 'md',
    active = false,
    disabled = false,
    type = 'button',
    title = undefined,
    class: extraClass = '',
    onclick,
    children,
  }: Props = $props();
</script>

<button
  {type}
  {disabled}
  {title}
  class="t-btn t-btn-{variant} t-btn-{size} {active ? 'is-active' : ''} {extraClass}"
  {onclick}
>
  {@render children?.()}
</button>

<style>
  .t-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    white-space: nowrap;
    flex-shrink: 0;
    font-family: inherit;
    font-weight: 500;
    border-radius: 7px;
    border: 1px solid transparent;
    cursor: pointer;
    user-select: none;
    outline: none;
    transition: background-color 0.12s ease, border-color 0.12s ease, color 0.12s ease, transform 0.08s ease;
  }
  .t-btn:focus-visible {
    box-shadow: 0 0 0 3px var(--ring);
    border-color: var(--accent);
  }
  .t-btn:active:not(:disabled) {
    transform: scale(0.98);
  }
  .t-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .t-btn-sm { height: 28px; padding: 0 10px; font-size: 12px; }
  .t-btn-md { height: 34px; padding: 0 13px; font-size: 13px; }
  .t-btn-lg { height: 38px; padding: 0 16px; font-size: 13.5px; }

  .t-btn-icon.t-btn-sm { width: 28px; padding: 0; }
  .t-btn-icon.t-btn-md { width: 34px; padding: 0; }
  .t-btn-icon.t-btn-lg { width: 38px; padding: 0; }

  .t-btn-primary {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .t-btn-primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .t-btn-secondary {
    background: var(--surface);
    color: var(--text);
    border-color: var(--border);
  }
  .t-btn-secondary:hover:not(:disabled) {
    background: var(--subtle);
    border-color: var(--border-strong);
  }
  .t-btn-secondary.is-active {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }

  .t-btn-ghost,
  .t-btn-icon {
    background: transparent;
    color: var(--text-secondary);
  }
  .t-btn-ghost:hover:not(:disabled),
  .t-btn-icon:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }
  .t-btn-ghost.is-active,
  .t-btn-icon.is-active {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .t-btn-danger {
    background: transparent;
    color: var(--danger);
    border-color: transparent;
  }
  .t-btn-danger:hover:not(:disabled) {
    background: var(--danger-soft);
    border-color: color-mix(in srgb, var(--danger) 28%, transparent);
  }
</style>
