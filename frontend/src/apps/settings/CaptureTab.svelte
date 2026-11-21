<script lang="ts">
  import { Camera, Languages, Pin, ScanText } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { HotkeySlotStatus, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import PathPicker from '../../lib/ui/PathPicker.svelte';
  import Slider from '../../lib/ui/Slider.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    capHotkeyStatus?: HotkeySlotStatus;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, capHotkeyStatus, onSave }: Props = $props();

  const capPresets = [
    { value: 'Alt+S', label: 'Alt+S' },
    { value: 'Alt+1', label: 'Alt+1' },
    { value: 'Alt+A', label: 'Alt+A' },
    { value: 'Ctrl+Shift+A', label: 'Ctrl+Shift+A' },
  ];

  function launch(mode: string) {
    postHost({ cmd: 'start_capture', mode });
  }
</script>

<div class="tab-head">
  <h2>{tr(lang, 'h_capture')}</h2>
  <p class="tab-lead">{tr(lang, 'lead_capture')}</p>
</div>

<div class="quick-bar">
  <Button variant="primary" onclick={() => launch('region')}>
    <Camera size={14} />
    {tr(lang, 'btn_cap_region')}
  </Button>
  <Button variant="secondary" onclick={() => launch('ocr')}>
    <ScanText size={14} />
    {tr(lang, 'btn_cap_ocr')}
  </Button>
  <Button variant="secondary" onclick={() => launch('translate')}>
    <Languages size={14} />
    {tr(lang, 'btn_cap_tr')}
  </Button>
  <Button variant="secondary" onclick={() => launch('pin_clipboard')}>
    <Pin size={14} />
    {tr(lang, 'btn_cap_pin')}
  </Button>
</div>

<div class="t-card">
  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_cap_hk')}</div>
      <div class="t-desc">{tr(lang, 'desc_cap_hk')}</div>
    </div>
    <HotkeyRecorder
      value={cfg.capture.hotkey}
      presets={capPresets}
      allowDisable={true}
      status={capHotkeyStatus}
      presetPlaceholder={tr(lang, 'preset_opt')}
      disabledLabel={tr(lang, 'hk_off')}
      okLabel={tr(lang, 'hk_ok')}
      conflictLabel={tr(lang, 'hk_conflict')}
      recordPrompt={tr(lang, 'rec_prompt')}
      needModPrompt={tr(lang, 'rec_need_mod')}
      onchange={(v) => { cfg.capture.hotkey = v; onSave(0); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_cap_opacity')}</div>
      <div class="t-desc">{tr(lang, 'desc_cap_opacity')}</div>
    </div>
    <Slider
      value={cfg.capture.pin_opacity}
      min={30}
      max={100}
      step={5}
      format={(v) => `${v}%`}
      oninput={(v) => { cfg.capture.pin_opacity = v; onSave(160); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_cap_hex')}</div>
      <div class="t-desc">{tr(lang, 'desc_cap_hex')}</div>
    </div>
    <Switch checked={cfg.capture.copy_hex_lowercase} onchange={(v) => { cfg.capture.copy_hex_lowercase = v; onSave(0); }} />
  </div>

  <div class="t-row col-row">
    <div class="t-label">{tr(lang, 'lbl_cap_dir')}</div>
    <PathPicker
      path={cfg.capture.save_dir}
      placeholder={tr(lang, 'cap_dir_default')}
      browseLabel={tr(lang, 'btn_browse')}
      onbrowse={() => postHost({ cmd: 'pick_capture_dir' })}
      onclear={() => { cfg.capture.save_dir = ''; onSave(0); }}
    />
  </div>
</div>

<div class="t-card tips-card" style="margin-top:14px">
  <div class="t-label">{tr(lang, 'lbl_cap_tips')}</div>
  <p class="tips-body">{tr(lang, 'desc_cap_tips')}</p>
</div>

<style>
  .tab-head { margin-bottom: 14px; }
  .tab-head h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
  .tab-lead { margin: 0; font-size: 12.5px; color: var(--muted); }
  .quick-bar { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 14px; }
  .col-row { flex-direction: column; align-items: stretch; gap: 8px; }
  .tips-card { padding: 14px 16px; }
  .tips-body { margin: 6px 0 0; font-size: 12px; color: var(--muted); line-height: 1.7; }
</style>
