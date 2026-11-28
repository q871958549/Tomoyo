<script lang="ts">
  import { FolderOpen, RotateCcw } from '@lucide/svelte';
  import Button from './Button.svelte';

  interface Props {
    path: string;
    placeholder: string;
    browseLabel?: string;
    onbrowse: () => void;
    onclear?: () => void;
  }

  let {
    path,
    placeholder,
    browseLabel = '浏览…',
    onbrowse,
    onclear,
  }: Props = $props();
</script>

<div class="t-path-picker">
  <div class="t-path-box" title={path || placeholder}>
    <span class="t-path-ico"><FolderOpen size={14} /></span>
    <span class="t-path-txt {path ? '' : 'is-ph'}">{path || placeholder}</span>
  </div>
  {#if path && onclear}
    <Button variant="icon" size="md" title="恢复默认" onclick={onclear}>
      <RotateCcw size={14} />
    </Button>
  {/if}
  <Button variant="secondary" size="md" onclick={onbrowse}>
    {browseLabel}
  </Button>
</div>

<style>
  .t-path-picker {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    max-width: 420px;
  }
  .t-path-box {
    flex: 1;
    min-width: 0;
    height: 34px;
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 0 10px;
    border-radius: 7px;
    background: var(--canvas);
    border: 1px solid var(--border);
  }
  .t-path-ico {
    display: inline-flex;
    color: var(--muted);
    flex-shrink: 0;
  }
  .t-path-txt {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text);
  }
  .t-path-txt.is-ph {
    font-family: inherit;
    color: var(--muted);
  }
</style>
