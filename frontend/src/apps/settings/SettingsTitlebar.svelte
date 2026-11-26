<script lang="ts">
  import tomoyoLogo from '../../assets/tomoyo-logo.png';
  import { postHost } from '../../lib/ipc';
  import type { Lang } from '../../lib/i18n';

  let { lang }: { lang: Lang } = $props();

  function onBarMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement | null)?.closest('button')) return;
    postHost({ cmd: 'start_window_drag' });
  }

  function onBarDblClick(e: MouseEvent) {
    if ((e.target as HTMLElement | null)?.closest('button')) return;
    postHost({ cmd: 'toggle_maximize_window' });
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header class="titlebar" onmousedown={onBarMouseDown} ondblclick={onBarDblClick}>
  <div class="brand-area">
    <img class="mark" src={tomoyoLogo} alt="Tomoyo" width="16" height="16" draggable="false" />
    <span class="title">{lang === 'en' ? 'Tomoyo Settings Center' : 'Tomoyo 设置中心'}</span>
  </div>

  <div class="win-controls">
    <button
      type="button"
      class="ctl-btn"
      title={lang === 'en' ? 'Minimize' : '最小化'}
      aria-label="Minimize"
      onclick={() => postHost({ cmd: 'minimize_window' })}
    >
      <svg viewBox="0 0 12 12" width="11" height="11" fill="none">
        <path d="M2 6h8" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
      </svg>
    </button>
    <button
      type="button"
      class="ctl-btn"
      title={lang === 'en' ? 'Maximize / Restore' : '最大化 / 还原'}
      aria-label="Maximize"
      onclick={() => postHost({ cmd: 'toggle_maximize_window' })}
    >
      <svg viewBox="0 0 12 12" width="11" height="11" fill="none">
        <rect x="2.2" y="2.2" width="7.6" height="7.6" rx="1" stroke="currentColor" stroke-width="1.15" />
      </svg>
    </button>
    <button
      type="button"
      class="ctl-btn is-close"
      title={lang === 'en' ? 'Close (Esc)' : '关闭 (Esc)'}
      aria-label="Close"
      onclick={() => postHost({ cmd: 'close' })}
    >
      <svg viewBox="0 0 12 12" width="11" height="11" fill="none">
        <path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
      </svg>
    </button>
  </div>
</header>

<style>
  .titlebar {
    height: 36px;
    padding: 0 0 0 14px;
    background: var(--sidebar);
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    justify-content: space-between;
    user-select: none;
    -webkit-user-select: none;
    flex-shrink: 0;
  }
  .brand-area {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    pointer-events: none;
  }
  .mark { width: 16px; height: 16px; border-radius: 4px; display: block; flex-shrink: 0; }
  .title {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--fg);
    letter-spacing: 0.1px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .win-controls {
    display: flex;
    align-items: stretch;
    height: 100%;
    flex-shrink: 0;
  }
  .ctl-btn {
    width: 44px;
    height: 36px;
    border: 0;
    background: transparent;
    color: var(--muted);
    display: grid;
    place-items: center;
    cursor: pointer;
    transition: background-color 120ms ease, color 120ms ease;
  }
  .ctl-btn:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .ctl-btn.is-close:hover {
    background: #e81123;
    color: #ffffff;
  }
</style>
