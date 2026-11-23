<script lang="ts">
  import { Copy, FolderOpen, RefreshCw } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import Button from '../../lib/ui/Button.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    lang: Lang;
    source: string;
    errorsOnly: boolean;
    content: string;
    metaLine: string;
    onSourceChange: (src: string) => void;
    onErrorsOnlyChange: (v: boolean) => void;
    onRefresh: () => void;
  }

  let {
    lang,
    source,
    errorsOnly,
    content,
    metaLine,
    onSourceChange,
    onErrorsOnlyChange,
    onRefresh,
  }: Props = $props();

  let srcOptions = $derived([
    { value: 'tomoyo.log', label: tr(lang, 'log_main') },
    { value: 'webview.log', label: tr(lang, 'log_wv') },
    { value: 'update.log', label: tr(lang, 'log_upd') },
    { value: 'inno_setup.log', label: tr(lang, 'log_inno') },
    { value: 'last_crash.txt', label: tr(lang, 'log_crash') },
  ]);
</script>

<div class="tab-head-bar">
  <div>
    <h2>{tr(lang, 'h_logs')}</h2>
    <p class="tab-lead">{tr(lang, 'lead_logs')}</p>
  </div>
  <div class="head-acts">
    <Button variant="secondary" onclick={() => postHost({ cmd: 'open_logs_dir' })}>
      <FolderOpen size={14} />
      {tr(lang, 'btn_open_logs')}
    </Button>
    <Button variant="primary" onclick={() => postHost({ cmd: 'copy_diagnostic_bundle' })}>
      <Copy size={14} />
      {tr(lang, 'btn_copy_diag')}
    </Button>
  </div>
</div>

<div class="t-card log-card">
  <div class="log-toolbar">
    <Segmented value={source} options={srcOptions} onchange={onSourceChange} />
    <div class="log-right">
      <span class="chk-lbl">{tr(lang, 'log_err_only')}</span>
      <Switch checked={errorsOnly} onchange={onErrorsOnlyChange} />
      <Button variant="secondary" size="sm" onclick={onRefresh}>
        <RefreshCw size={13} />
        {tr(lang, 'btn_refresh_logs')}
      </Button>
      <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'clear_diagnostic_logs', source, errors_only: errorsOnly })}>
        {tr(lang, 'btn_clear_logs')}
      </Button>
    </div>
  </div>

  <div class="t-desc" style="margin:10px 0 8px">{metaLine}</div>
  <pre class="log-pre">{content || '(空)'}</pre>
</div>

<style>
  .head-acts { display: flex; gap: 8px; }
  .log-card { padding: 14px 16px; }
  .log-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap; }
  .log-right { display: flex; align-items: center; gap: 8px; }
  .chk-lbl { font-size: 12.5px; color: var(--text-secondary); white-space: nowrap; }
  .log-pre {
    margin: 0; padding: 12px 14px; border-radius: 9px; background: var(--canvas);
    border: 1px solid var(--border); font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 11.6px; line-height: 1.55; color: var(--text); white-space: pre-wrap;
    word-break: break-word; max-height: 360px; overflow-y: auto;
  }
</style>
