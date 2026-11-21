<script lang="ts">
  import { ClipboardList, FolderOpen, FolderPlus, Trash2 } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { HotkeySlotStatus, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Combobox from '../../lib/ui/Combobox.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import Slider from '../../lib/ui/Slider.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    runningApps: string[];
    folderErr: string;
    clipHotkeyStatus?: HotkeySlotStatus;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, runningApps, folderErr, clipHotkeyStatus, onSave }: Props = $props();

  const evPresets = [
    { tok: '!*.tmp', zh: '临时文件 (.tmp)', en: 'Temp (.tmp)' },
    { tok: '!node_modules', zh: 'node_modules', en: 'node_modules' },
    { tok: '!.git', zh: '.git 目录', en: '.git' },
    { tok: '!*.log', zh: '日志 (.log)', en: 'Logs (.log)' },
    { tok: '!target', zh: 'Rust target', en: 'Rust target' },
    { tok: '!dist', zh: '构建 dist', en: 'Build dist' },
    { tok: '!*.bak', zh: '备份 (.bak)', en: 'Backups (.bak)' },
  ];

  const clipPresets = [
    { value: 'Alt+V', label: 'Alt+V' },
    { value: 'Ctrl+Shift+V', label: 'Ctrl+Shift+V' },
    { value: 'Alt+`', label: 'Alt+`' },
    { value: 'Ctrl+Alt+V', label: 'Ctrl+Alt+V' },
  ];

  let maxItemsOptions = $derived([
    { value: '100', label: '100' },
    { value: '200', label: '200' },
    { value: '500', label: lang === 'en' ? '500 (Recommended)' : '500（推荐）' },
    { value: '1000', label: '1000' },
    { value: '2000', label: '2000' },
  ]);

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

  function normalizeExe(raw: string): string | null {
    let s = raw.trim().toLowerCase();
    if (!s || /[\\/]|\.\./.test(s) || s.length > 64) return null;
    if (!s.endsWith('.exe')) s += '.exe';
    const stem = s.slice(0, -4);
    if (!stem || !/^[a-z0-9._ -]+$/.test(stem)) return null;
    return s;
  }

  let depthOptions = $derived(
    [1, 2, 3, 4].map((d) => ({ value: String(d), label: lang === 'en' ? `${d} lvl` : `${d} 层` }))
  );
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
    <Segmented
      value={String(cfg.apps.extra_scan_depth)}
      options={depthOptions}
      onchange={(v) => { cfg.apps.extra_scan_depth = Number(v) || 2; onSave(0); }}
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

<!-- Clipboard -->
<div class="t-card" style="margin-top:14px">
  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_clip_hk')}</div>
      <div class="t-desc">{tr(lang, 'desc_clip_hk')}</div>
    </div>
    <HotkeyRecorder
      value={cfg.clipboard.hotkey}
      presets={clipPresets}
      allowDisable={true}
      status={clipHotkeyStatus}
      presetPlaceholder={tr(lang, 'preset_opt')}
      disabledLabel={tr(lang, 'hk_off')}
      okLabel={tr(lang, 'hk_ok')}
      conflictLabel={tr(lang, 'hk_conflict')}
      recordPrompt={tr(lang, 'rec_prompt')}
      needModPrompt={tr(lang, 'rec_need_mod')}
      onchange={(v) => { cfg.clipboard.hotkey = v; onSave(0); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_clip_cap')}</div>
      <div class="t-desc">{tr(lang, 'desc_clip_cap')}</div>
    </div>
    <div class="row-actions">
      <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'open_clipboard_manager' })}>
        <ClipboardList size={13} />
        {tr(lang, 'btn_open_clip_mgr')}
      </Button>
      <Switch checked={cfg.clipboard.capture_enabled} onchange={(v) => { cfg.clipboard.capture_enabled = v; onSave(0); }} />
    </div>
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_clip_img')}</div>
      <div class="t-desc">{tr(lang, 'desc_clip_img')}</div>
    </div>
    <Switch checked={cfg.clipboard.capture_images} onchange={(v) => { cfg.clipboard.capture_images = v; onSave(0); }} />
  </div>

  <div class="t-row col-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_clip_excl')}</div>
      <div class="t-desc">{tr(lang, 'desc_clip_excl')}</div>
    </div>
    <Combobox
      selected={cfg.clipboard.excluded_apps}
      suggestions={runningApps}
      placeholder={tr(lang, 'clip_excl_ph')}
      emptyText={lang === 'en' ? 'No excluded apps configured' : '当前未添加额外屏蔽应用（系统机密标记默认生效）'}
      normalize={normalizeExe}
      invalidMessage={lang === 'en' ? 'Enter a valid .exe filename' : '请输入有效的 .exe 进程名（禁止路径与非法字符）'}
      onchange={(next) => { cfg.clipboard.excluded_apps = next; onSave(0); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_clip_persist')}</div>
      <div class="t-desc">{tr(lang, 'desc_clip_persist')}</div>
    </div>
    <Switch checked={cfg.clipboard.persist_sqlite} onchange={(v) => { cfg.clipboard.persist_sqlite = v; onSave(0); }} />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_clip_max')}</div>
      <div class="t-desc">{tr(lang, 'desc_clip_max')}</div>
    </div>
    <Select
      value={String(cfg.clipboard.max_persisted_items)}
      options={maxItemsOptions}
      align="right"
      minWidth="136px"
      onchange={(v) => { cfg.clipboard.max_persisted_items = Number(v) || 500; onSave(0); }}
    />
  </div>
</div>

<style>
  .tab-head { margin-bottom: 18px; }
  .tab-head h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
  .tab-lead { margin: 0; font-size: 12.5px; color: var(--muted); }
  .col-row { flex-direction: column; align-items: stretch; gap: 10px; }
  .col-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .row-actions { display: flex; align-items: center; gap: 10px; }
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
  .chip-grid { display: flex; flex-wrap: wrap; gap: 6px; }
  .t-chip {
    height: 28px; padding: 0 10px; border-radius: 7px; border: 1px solid var(--border);
    background: var(--canvas); color: var(--text-secondary); font-family: inherit;
    font-size: 12px; cursor: pointer; white-space: nowrap; transition: all 0.12s ease;
  }
  .t-chip:hover { border-color: var(--border-strong); color: var(--text); }
  .t-chip.is-on {
    background: var(--accent-soft); border-color: var(--accent); color: var(--accent); font-weight: 500;
  }
</style>
