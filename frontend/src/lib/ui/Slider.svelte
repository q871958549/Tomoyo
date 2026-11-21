<script lang="ts">
  interface Props {
    value: number;
    min: number;
    max: number;
    step?: number;
    format?: (v: number) => string;
    oninput: (v: number) => void;
  }

  let {
    value,
    min,
    max,
    step = 1,
    format = (v: number) => String(v),
    oninput,
  }: Props = $props();

  let pct = $derived(max > min ? ((value - min) / (max - min)) * 100 : 0);

  function handle(e: Event) {
    const n = Number((e.currentTarget as HTMLInputElement).value);
    if (!Number.isNaN(n)) oninput(n);
  }
</script>

<div class="t-slider-wrap">
  <input
    type="range"
    {min}
    {max}
    {step}
    {value}
    style="--pct: {pct}%"
    class="t-slider"
    oninput={handle}
  />
  <span class="t-slider-val">{format(value)}</span>
</div>

<style>
  .t-slider-wrap {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }
  .t-slider {
    width: 148px;
    height: 4px;
    appearance: none;
    border-radius: 999px;
    background: linear-gradient(to right, var(--accent) 0%, var(--accent) var(--pct), var(--border-strong) var(--pct), var(--border-strong) 100%);
    outline: none;
    cursor: pointer;
  }
  .t-slider::-webkit-slider-thumb {
    appearance: none;
    width: 15px;
    height: 15px;
    border-radius: 999px;
    background: var(--surface);
    border: 2px solid var(--accent);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
    transition: transform 0.1s ease;
  }
  .t-slider::-webkit-slider-thumb:hover {
    transform: scale(1.12);
  }
  .t-slider-val {
    min-width: 54px;
    text-align: right;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
