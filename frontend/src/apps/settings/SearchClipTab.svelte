<script lang="ts">
  import { FolderOpen, FolderPlus, Trash2 } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { CustomFilterTab, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Slider from '../../lib/ui/Slider.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import FilterTabsCard from './FilterTabsCard.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    folderErr: string;
    onUpdateFilters: (hidden: string[], custom: CustomFilterTab[], layout?: string) => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, folderErr, onUpdateFilters, onSave }: Props = $props();

  const evPresets = [
    { tok: '!*.tmp', zh: '临时文件 (.tmp)', en: 'Temp (.tmp)' },
    { tok: '!node_modules', zh: 'node_modules', en: 'node_modules' },
    { tok: '!.git', zh: '.git 目录', en: '.git' },
    { tok: '!*.log', zh: '日志 (.log)', en: 'Logs (.log)' },
    { tok: '!target', zh: 'Rust target', en: 'Rust target' },
    { tok: '!dist', zh: '构建 dist', en: 'Build dist' },
    { tok: '!*.bak', zh: '备份 (.bak)', en: 'Backups (.bak)' },
  ];

  let activeEvTokens = $derived(
    new Set((cfg.everything.extra_filter || '').split(/\s+/).filter(Boolean))
  );

  function toggleEvToken(tok: string) {
    const next = new Set(activeEvTokens);
    if (next.has(tok)) next.delete(tok);
    else next.add(tok);
    cfg.everything.extra_filter = [...next].join(' ');
    onSave(0);
  }

  function removeDir(idx: number) {
    cfg.apps.extra_dirs = cfg.apps.extra_dirs.filter((_, i) => i !== idx);
    onSave(0);
  }
</script>

<div class="tab-head">
  <h2>{tr(lang, 'h_search')}</h2>
  <p class="tab-lead">{tr(lang, 'lead_search')}</p>
</div>

<!-- Apps Indexing -->
<div class="t-card">
  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_apps_org')}</div>
      <div class="t-desc">{tr(lang, 'desc_apps_org')}</div>
    </div>
    <Switch checked={cfg.apps.auto_detect_desktop_organizers} onchange={(v) => { cfg.apps.auto_detect_desktop_organizers = v; onSave(0); }} />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_apps_depth')}</div>
      <div class="t-desc">{tr(lang, 'desc_apps_depth')}</div>
    </div>
    <Slider
      value={cfg.apps.extra_scan_depth}
      min={1}
      max={12}
      step={1}
      format={(v) => (lang === 'en' ? `${v} lvl${v > 1 ? 's' : ''}` : `${v} 层`)}
      oninput={(v) => { cfg.apps.extra_scan_depth = v; onSave(160); }}
    />
  </div>

  <div class="t-row col-row">
    <div class="col-head">
      <div>
        <div class="t-label">{tr(lang, 'lbl_apps_dirs')}</div>
        <div class="t-desc">{tr(lang, 'desc_apps_dirs')}</div>
      </div>
      <Button variant="secondary" onclick={() => postHost({ cmd: 'pick_folder' })}>
        <FolderPlus size={14} />
        {tr(lang, 'btn_browse_dir')}
      </Button>
    </div>

    {#if cfg.apps.extra_dirs.length === 0}
      <div class="empty-box">{tr(lang, 'dirs_empty')}</div>
    {:else}
      <div class="dir-list">
        {#each cfg.apps.extra_dirs as dir, idx}
          <div class="dir-item">
            <FolderOpen size={14} class="dir-ico" />
            <span class="dir-txt" title={dir}>{dir}</span>
            <Button variant="danger" size="sm" onclick={() => removeDir(idx)}>
              <Trash2 size={13} />
              {tr(lang, 'del')}
            </Button>
          </div>
        {/each}
      </div>
    {/if}
    {#if folderErr}
      <div class="err-msg">{folderErr}</div>
    {/if}
  </div>
</div>

<!-- Everything -->
<div class="t-card" style="margin-top:14px">
  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_ev_global')}</div>
      <div class="t-desc">{tr(lang, 'desc_ev_global')}</div>
    </div>
    <Slider
      value={cfg.everything.max_global_results}
      min={0}
      max={10}
      step={1}
      format={(v) => (v === 0 ? (lang === 'en' ? 'Off' : '关闭') : lang === 'en' ? `${v}` : `${v} 条`)}
      oninput={(v) => { cfg.everything.max_global_results = v; onSave(160); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_ev_ded')}</div>
      <div class="t-desc">{tr(lang, 'desc_ev_ded')}</div>
    </div>
    <Slider
      value={cfg.everything.max_dedicated_results}
      min={10}
      max={50}
      step={5}
      format={(v) => (lang === 'en' ? `${v}` : `${v} 条`)}
      oninput={(v) => { cfg.everything.max_dedicated_results = v; onSave(160); }}
    />
  </div>

  <div class="t-row col-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_ev_filter')}</div>
      <div class="t-desc">{tr(lang, 'desc_ev_filter')}</div>
    </div>
    <div class="chip-grid">
      {#each evPresets as p}
        <button
          type="button"
          class="t-chip {activeEvTokens.has(p.tok) ? 'is-on' : ''}"
          onclick={() => toggleEvToken(p.tok)}
        >
          {activeEvTokens.has(p.tok) ? '✓ ' : '+ '}{lang === 'en' ? p.en : p.zh}
        </button>
      {/each}
    </div>
    <div class="t-desc">
      {cfg.everything.extra_filter ? `${lang === 'en' ? 'Active:' : '当前过滤：'} ${cfg.everything.extra_filter}` : tr(lang, 'ev_filter_none')}
    </div>
  </div>
</div>

<!-- User-Extensible Search Filter Tabs -->
<FilterTabsCard {cfg} {lang} {onUpdateFilters} />

<style>
  .col-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .empty-box {
    padding: 10px 12px; border-radius: 8px; border: 1px dashed var(--border-strong);
    color: var(--muted); font-size: 12px;
  }
  .dir-list { display: flex; flex-direction: column; gap: 6px; }
  .dir-item {
    display: flex; align-items: center; gap: 8px; height: 36px; padding: 0 8px 0 11px;
    border-radius: 8px; background: var(--canvas); border: 1px solid var(--border);
  }
  .dir-txt {
    flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px;
  }
  .err-msg { font-size: 12px; color: var(--danger); }
</style>
