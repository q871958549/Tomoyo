<script lang="ts">
  import { ClipboardList } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { HotkeySlotStatus, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Combobox from '../../lib/ui/Combobox.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    runningApps: string[];
    clipHotkeyStatus?: HotkeySlotStatus;
    onJumpDoctor?: () => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, runningApps, clipHotkeyStatus, onJumpDoctor, onSave }: Props = $props();

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

  function normalizeExe(raw: string): string | null {
    let s = raw.trim().toLowerCase();
    if (!s || /[\\/]|\.\./.test(s) || s.length > 64) return null;
    if (!s.endsWith('.exe')) s += '.exe';
    const stem = s.slice(0, -4);
    if (!stem || !/^[a-z0-9._ -]+$/.test(stem)) return null;
    return s;
  }
</script>

<div class="clip-drawer">
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
      {onJumpDoctor}
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
  .clip-drawer { padding: 4px 0; }
</style>
