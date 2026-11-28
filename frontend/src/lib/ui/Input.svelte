<script lang="ts">
  import Icon from './Icon.svelte';

  interface Props {
    value?: string;
    type?: 'text' | 'password' | 'url' | 'search';
    placeholder?: string;
    maxlength?: number;
    disabled?: boolean;
    readonly?: boolean;
    invalid?: boolean;
    mono?: boolean;
    size?: 'sm' | 'md';
    id?: string;
    inputEl?: HTMLInputElement;
    autocomplete?: string;
    class?: string;
    oninput?: (val: string) => void;
    onchange?: (val: string) => void;
    onfocus?: (e: FocusEvent) => void;
    onblur?: (e: FocusEvent) => void;
    onkeydown?: (e: KeyboardEvent) => void;
  }

  let {
    value = $bindable(''),
    type = 'text',
    placeholder = '',
    maxlength = undefined,
    disabled = false,
    readonly = false,
    invalid = false,
    mono = false,
    size = 'md',
    id = undefined,
    inputEl = $bindable(),
    autocomplete = 'off',
    class: extraClass = '',
    oninput,
    onchange,
    onfocus,
    onblur,
    onkeydown,
  }: Props = $props();

  let revealed = $state(false);
  let effectiveType = $derived(type === 'password' && revealed ? 'text' : type === 'search' ? 'text' : type);

  function handleInput(e: Event) {
    const v = (e.currentTarget as HTMLInputElement).value;
    value = v;
    oninput?.(v);
  }

  function handleChange(e: Event) {
    const v = (e.currentTarget as HTMLInputElement).value;
    value = v;
    onchange?.(v);
  }

  function clearSearch() {
    value = '';
    oninput?.('');
    onchange?.('');
  }
</script>

<div class="t-input-wrap t-input-{size} {invalid ? 'is-invalid' : ''} {disabled ? 'is-disabled' : ''} {extraClass}">
  {#if type === 'search'}
    <span class="t-input-lead"><Icon name="Search" size={14} /></span>
  {/if}
  <input
    bind:this={inputEl}
    {id}
    type={effectiveType}
    {value}
    {placeholder}
    {maxlength}
    {disabled}
    {readonly}
    autocomplete={autocomplete as AutoFill}
    spellcheck="false"
    class="t-input-el {mono ? 'is-mono' : ''}"
    oninput={handleInput}
    onchange={handleChange}
    {onfocus}
    {onblur}
    {onkeydown}
  />
  {#if type === 'password'}
    <button
      type="button"
      class="t-input-act"
      tabindex="-1"
      title={revealed ? '隐藏' : '显示'}
      onclick={() => (revealed = !revealed)}
    >
      {#if revealed}<Icon name="EyeOff" size={14} />{:else}<Icon name="Eye" size={14} />{/if}
    </button>
  {:else if type === 'search' && value}
    <button type="button" class="t-input-act" tabindex="-1" title="清空" onclick={clearSearch}>
      <Icon name="X" size={13} />
    </button>
  {/if}
</div>

<style>
  .t-input-wrap {
    display: flex;
    align-items: center;
    width: 100%;
    background: var(--canvas);
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 0 9px;
    gap: 6px;
    transition: border-color 0.12s ease, box-shadow 0.12s ease, background-color 0.12s ease;
  }
  .t-input-md { height: 34px; }
  .t-input-sm { height: 30px; }
  .t-input-wrap:hover:not(.is-disabled) {
    border-color: var(--border-strong);
  }
  .t-input-wrap:focus-within {
    background: var(--surface);
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--ring);
  }
  .t-input-wrap.is-invalid {
    border-color: var(--danger);
    box-shadow: 0 0 0 3px var(--danger-soft);
  }
  .t-input-wrap.is-disabled {
    opacity: 0.55;
  }
  .t-input-lead {
    display: inline-flex;
    color: var(--muted);
    flex-shrink: 0;
  }
  .t-input-el {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    font-size: 13px;
  }
  .t-input-el.is-mono {
    font-family: var(--font-mono);
    font-size: 12.2px;
  }
  .t-input-el::placeholder {
    color: var(--muted);
    opacity: 0.85;
  }
  .t-input-act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    flex-shrink: 0;
  }
  .t-input-act:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
