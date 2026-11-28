<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
  import { postHost } from '../../lib/ipc';
  import type { SuspectProcess } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';

  interface Props {
    spec: string;
    suspect: SuspectProcess;
    isEn: boolean;
    onClose: () => void;
  }

  let { spec, suspect, isEn, onClose }: Props = $props();
  let confirmKill = $state(false);
  let killing = $state(false);

  $effect(() => {
    void suspect.pid;
    confirmKill = false;
    killing = false;
  });

  function onKillClick() {
    if (!confirmKill) {
      confirmKill = true;
      return;
    }
    killing = true;
    postHost({ cmd: 'terminate_conflict_process', pid: suspect.pid, process_name: suspect.process_name });
  }
</script>

<div class="sus-card" data-suspect-detail={suspect.process_name}>
  <div class="sus-head">
    <div class="sus-title">
      <Icon name="Info" size={14} class="sus-ico" />
      <strong>{suspect.process_name}</strong>
      <span class="sus-pid">PID {suspect.pid}</span>
      <kbd class="t-kbd">{spec}</kbd>
      <span class="sus-conf {suspect.confidence === 'high' ? 'is-high' : ''}">
        {suspect.confidence === 'high'
          ? (isEn ? 'Config Confirmed' : '配置文件实锤')
          : (isEn ? 'Default Key Reference' : '常见默认键参考')}
      </span>
    </div>
    <button type="button" class="sus-close" onclick={onClose} aria-label="Close"><Icon name="X" size={13} /></button>
  </div>

  <p class="sus-rem">
    {suspect.remediation || (isEn
      ? `Open ${suspect.process_name} settings to unbind ${spec}, then rescan.`
      : `请在 ${suspect.process_name} 的设置中关闭或修改 ${spec} 快捷键，再点击「重新扫描热键」。`)}
  </p>

  {#if suspect.exe_path}
    <div class="sus-path">{suspect.exe_path}</div>
  {/if}

  <div class="sus-acts">
    {#if suspect.exe_path}
      <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'reveal_path', path: suspect.exe_path })}>
        <Icon name="FolderOpen" size={13} />
        {isEn ? 'Reveal Program' : '定位程序所在目录'}
      </Button>
    {/if}
    {#if suspect.can_terminate}
      <Button variant={confirmKill ? 'danger' : 'secondary'} size="sm" disabled={killing} onclick={onKillClick}>
        <Icon name="PowerOff" size={13} />
        {#if killing}
          {isEn ? 'Terminating…' : '正在结束并重扫…'}
        {:else if confirmKill}
          {isEn ? `Confirm Terminate ${suspect.process_name}` : `再次点击确认结束 ${suspect.process_name}`}
        {:else}
          {isEn ? 'Terminate Process' : '结束占用进程'}
        {/if}
      </Button>
    {/if}
  </div>
</div>

<style>
  .sus-card {
    margin: 10px 0; padding: 12px 14px; border-radius: 9px;
    border: 1px solid var(--accent); background: var(--accent-soft);
    display: flex; flex-direction: column; gap: 7px;
  }
  .sus-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .sus-title { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; font-size: 12.5px; }
  .sus-pid { font-size: 11px; color: var(--muted); font-family: var(--font-mono); }
  .sus-conf {
    padding: 1px 7px; border-radius: 999px; font-size: 10.5px; font-weight: 500;
    background: var(--surface); color: var(--text-secondary); border: 1px solid var(--border);
  }
  .sus-conf.is-high { color: var(--danger); border-color: rgba(220, 38, 38, 0.35); background: var(--danger-soft); }
  .sus-close {
    border: 0; background: transparent; color: var(--muted); cursor: pointer;
    width: 24px; height: 24px; border-radius: 6px; display: inline-grid; place-items: center;
  }
  .sus-close:hover { background: var(--hover); color: var(--text); }
  .sus-rem { margin: 0; font-size: 12.5px; color: var(--text); line-height: 1.55; }
  .sus-path { font-size: 11px; color: var(--text-secondary); font-family: var(--font-mono); word-break: break-all; }
  .sus-acts { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-top: 2px; }
  :global(.sus-ico) { color: var(--accent); flex-shrink: 0; }
</style>
