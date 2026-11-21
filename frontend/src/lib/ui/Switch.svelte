<script lang="ts">
  interface Props {
    checked?: boolean;
    disabled?: boolean;
    id?: string;
    ariaLabel?: string;
    onchange?: (checked: boolean) => void;
  }

  let {
    checked = $bindable(false),
    disabled = false,
    id = undefined,
    ariaLabel = undefined,
    onchange,
  }: Props = $props();

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<button
  {id}
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={ariaLabel}
  {disabled}
  class="t-switch {checked ? 'is-on' : ''}"
  onclick={toggle}
>
  <span class="t-switch-thumb"></span>
</button>

<style>
  .t-switch {
    position: relative;
    display: inline-flex;
    align-items: center;
    width: 36px;
    height: 20px;
    padding: 2px;
    border-radius: 999px;
    border: 1px solid var(--border-strong);
    background: var(--subtle);
    cursor: pointer;
    flex-shrink: 0;
    outline: none;
    transition: background-color 0.16s ease, border-color 0.16s ease;
  }
  .t-switch:focus-visible {
    box-shadow: 0 0 0 3px var(--ring);
  }
  .t-switch:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .t-switch.is-on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .t-switch-thumb {
    width: 14px;
    height: 14px;
    border-radius: 999px;
    background: #ffffff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.22);
    transform: translateX(0);
    transition: transform 0.16s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .t-switch.is-on .t-switch-thumb {
    transform: translateX(16px);
  }
</style>
