<script lang="ts">
  interface Option {
    value: string;
    label: string;
  }
  interface Props {
    value: string;
    options: Option[];
    onchange: (val: string) => void;
  }

  let { value, options, onchange }: Props = $props();
</script>

<div class="t-seg" role="tablist">
  {#each options as opt}
    <button
      type="button"
      role="tab"
      aria-selected={value === opt.value}
      class="t-seg-btn {value === opt.value ? 'is-active' : ''}"
      onclick={() => onchange(opt.value)}
    >
      {opt.label}
    </button>
  {/each}
</div>

<style>
  .t-seg {
    display: inline-flex;
    align-items: center;
    background: var(--subtle);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 2px;
    gap: 2px;
    flex-shrink: 0;
  }
  .t-seg-btn {
    height: 28px;
    padding: 0 11px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 500;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color 0.12s ease, color 0.12s ease, box-shadow 0.12s ease;
  }
  .t-seg-btn:hover:not(.is-active) {
    color: var(--text);
  }
  .t-seg-btn.is-active {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(15, 23, 42, 0.08);
  }
</style>
