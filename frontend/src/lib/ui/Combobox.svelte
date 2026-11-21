<script lang="ts">
  import { Plus, Search, X } from '@lucide/svelte';

  interface Props {
    selected: string[];
    suggestions: string[];
    placeholder?: string;
    emptyText?: string;
    normalize?: (raw: string) => string | null;
    invalidMessage?: string;
    onchange: (next: string[]) => void;
  }

  let {
    selected,
    suggestions,
    placeholder = '搜索或输入…',
    emptyText = '暂无条目',
    normalize = (s: string) => s.trim() || null,
    invalidMessage = '格式无效',
    onchange,
  }: Props = $props();

  let query = $state('');
  let open = $state(false);
  let activeIdx = $state(0);
  let errMsg = $state('');
  let rootEl: HTMLDivElement | undefined = $state();

  let selectedLower = $derived(new Set(selected.map((s) => s.toLowerCase())));

  let candidates = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = suggestions
      .filter((s) => !selectedLower.has(s.toLowerCase()) && (!q || s.toLowerCase().includes(q)))
      .slice(0, 12)
      .map((v) => ({ value: v, isCustom: false }));
    const custom = q ? normalize(q) : null;
    if (custom && !selectedLower.has(custom.toLowerCase()) && !list.some((x) => x.value.toLowerCase() === custom.toLowerCase())) {
      list.push({ value: custom, isCustom: true });
    }
    return list;
  });

  function addItem(raw: string) {
    const norm = normalize(raw);
    if (!norm) {
      errMsg = invalidMessage;
      return;
    }
    errMsg = '';
    if (!selectedLower.has(norm.toLowerCase()) && selected.length < 32) {
      onchange([...selected, norm]);
    }
    query = '';
    open = false;
  }

  function removeItem(idx: number) {
    const next = selected.slice();
    next.splice(idx, 1);
    onchange(next);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      open = false;
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      open = true;
      activeIdx = candidates.length ? (activeIdx + 1) % candidates.length : 0;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      activeIdx = candidates.length ? (activeIdx <= 0 ? candidates.length - 1 : activeIdx - 1) : 0;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (open && candidates[activeIdx]) {
        addItem(candidates[activeIdx].value);
      } else if (query.trim()) {
        addItem(query);
      }
    }
  }

  function onWinMouseDown(e: MouseEvent) {
    if (open && rootEl && !rootEl.contains(e.target as Node)) {
      open = false;
    }
  }
</script>

<svelte:window onmousedown={onWinMouseDown} />

<div bind:this={rootEl} class="t-combo-root">
  <div class="t-combo-input-box {errMsg ? 'is-err' : ''}">
    <span class="t-combo-ico"><Search size={14} /></span>
    <input
      type="text"
      bind:value={query}
      {placeholder}
      maxlength={48}
      autocomplete="off"
      spellcheck="false"
      class="t-combo-input"
      onfocus={() => { open = true; activeIdx = 0; }}
      oninput={() => { open = true; activeIdx = 0; errMsg = ''; }}
      onkeydown={handleKeydown}
    />
    {#if query}
      <button type="button" class="t-combo-clr" tabindex="-1" onclick={() => { query = ''; errMsg = ''; }}>
        <X size={13} />
      </button>
    {/if}
  </div>

  {#if open && candidates.length > 0}
    <div class="t-combo-menu" role="listbox">
      {#each candidates as c, i}
        <button
          type="button"
          role="option"
          aria-selected={i === activeIdx}
          class="t-combo-opt {i === activeIdx ? 'is-focus' : ''}"
          onmouseenter={() => (activeIdx = i)}
          onmousedown={(e) => { e.preventDefault(); addItem(c.value); }}
        >
          {#if c.isCustom}
            <Plus size={13} />
            <span>添加 {c.value}</span>
          {:else}
            <span>{c.value}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}

  {#if errMsg}
    <div class="t-combo-err">{errMsg}</div>
  {/if}

  <div class="t-combo-tags">
    {#if selected.length === 0}
      <span class="t-desc">{emptyText}</span>
    {:else}
      {#each selected as item, idx}
        <span class="t-tag">
          <span>{item}</span>
          <button type="button" class="t-tag-x" title="移除" onclick={() => removeItem(idx)}>
            <X size={11} />
          </button>
        </span>
      {/each}
    {/if}
  </div>
</div>

<style>
  .t-combo-root { position: relative; width: 100%; display: flex; flex-direction: column; gap: 8px; }
  .t-combo-input-box {
    display: flex; align-items: center; gap: 7px; height: 34px; width: 100%; max-width: 380px;
    background: var(--canvas); border: 1px solid var(--border); border-radius: 7px; padding: 0 9px;
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .t-combo-input-box:focus-within {
    background: var(--surface); border-color: var(--accent); box-shadow: 0 0 0 3px var(--ring);
  }
  .t-combo-input-box.is-err { border-color: var(--danger); }
  .t-combo-ico { display: inline-flex; color: var(--muted); flex-shrink: 0; }
  .t-combo-input {
    flex: 1; min-width: 0; height: 100%; border: 0; outline: none; background: transparent;
    color: var(--text); font-family: inherit; font-size: 12.5px;
  }
  .t-combo-input::placeholder { color: var(--muted); }
  .t-combo-clr {
    border: 0; background: transparent; color: var(--muted); cursor: pointer;
    display: inline-flex; align-items: center; padding: 2px; border-radius: 4px;
  }
  .t-combo-clr:hover { background: var(--hover); color: var(--text); }
  .t-combo-menu {
    position: absolute; top: 38px; left: 0; width: 100%; max-width: 380px; max-height: 210px;
    overflow-y: auto; z-index: 60; background: var(--elevated); border: 1px solid var(--border);
    border-radius: 10px; box-shadow: var(--shadow-pop); padding: 4px; display: flex; flex-direction: column; gap: 1px;
  }
  .t-combo-opt {
    display: flex; align-items: center; gap: 6px; padding: 6px 9px; border: 0; border-radius: 6px;
    background: transparent; color: var(--text); font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 12px; text-align: left; cursor: pointer;
  }
  .t-combo-opt.is-focus { background: var(--hover); }
  .t-combo-err { font-size: 12px; color: var(--danger); }
  .t-combo-tags { display: flex; flex-wrap: wrap; gap: 6px; }
  .t-tag {
    display: inline-flex; align-items: center; gap: 5px; height: 26px; padding: 0 6px 0 9px;
    border-radius: 6px; background: var(--subtle); border: 1px solid var(--border);
    color: var(--text-secondary); font-family: "Cascadia Mono", Consolas, monospace; font-size: 11.5px;
  }
  .t-tag-x {
    display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 16px;
    border: 0; border-radius: 4px; background: transparent; color: var(--muted); cursor: pointer;
  }
  .t-tag-x:hover { background: var(--danger-soft); color: var(--danger); }
</style>
