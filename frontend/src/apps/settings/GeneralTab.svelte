<script lang="ts">
  import { Download, Upload } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import type { HotkeySlotStatus, TomoyoConfig } from '../../lib/types';
  import { tr, type Lang } from '../../lib/i18n';
  import Button from '../../lib/ui/Button.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Slider from '../../lib/ui/Slider.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    autostart: boolean;
    hotkeyStatus?: HotkeySlotStatus;
    onLanguageChange: (code: Lang) => void;
    onAutostartChange: (checked: boolean) => void;
    onJumpDoctor?: () => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, autostart, hotkeyStatus, onLanguageChange, onAutostartChange, onJumpDoctor, onSave }: Props = $props();
  let fileInput: HTMLInputElement | undefined = $state();
  let isEn = $derived(lang === 'en');

  const mainPresets = [
    { value: 'Alt+Space', label: 'Alt+Space' },
    { value: 'Ctrl+Space', label: 'Ctrl+Space' },
    { value: 'Ctrl+Shift+Space', label: 'Ctrl+Shift+Space' },
    { value: 'Alt+Q', label: 'Alt+Q' },
  ];

  async function onPickBackupFile(ev: Event) {
    const input = ev.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    const text = await file.text();
    input.value = '';
    postHost({ cmd: 'import_config_backup', json: text });
  }
</script>

<div class="tab-head">
  <h2>{tr(lang, 'h_general')}</h2>
  <p class="tab-lead">{tr(lang, 'lead_general')}</p>
</div>

<div class="t-card">
  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_lang')}</div>
      <div class="t-desc">{tr(lang, 'desc_lang')}</div>
    </div>
    <Segmented
      value={lang}
      options={[
        { value: 'zh', label: '简体中文' },
        { value: 'en', label: 'English' },
      ]}
      onchange={(v) => onLanguageChange(v === 'en' ? 'en' : 'zh')}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_hotkey')}</div>
      <div class="t-desc">{tr(lang, 'desc_hotkey')}</div>
    </div>
    <HotkeyRecorder
      value={cfg.hotkey}
      presets={mainPresets}
      status={hotkeyStatus}
      presetPlaceholder={tr(lang, 'preset_opt')}
      okLabel={tr(lang, 'hk_ok')}
      conflictLabel={tr(lang, 'hk_conflict')}
      recordPrompt={tr(lang, 'rec_prompt')}
      needModPrompt={tr(lang, 'rec_need_mod')}
      {onJumpDoctor}
      onchange={(v) => { cfg.hotkey = v; onSave(0); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_autostart')}</div>
      <div class="t-desc">{tr(lang, 'desc_autostart')}</div>
    </div>
    <Switch checked={autostart} onchange={(v) => onAutostartChange(v)} />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_hibernate')}</div>
      <div class="t-desc">{tr(lang, 'desc_hibernate')}</div>
    </div>
    <Slider
      value={cfg.hibernate_delay_secs}
      min={5}
      max={120}
      step={5}
      format={(v) => `${v} s`}
      oninput={(v) => { cfg.hibernate_delay_secs = v; onSave(180); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_autopreview')}</div>
      <div class="t-desc">{tr(lang, 'desc_autopreview')}</div>
    </div>
    <Switch checked={cfg.auto_preview} onchange={(v) => { cfg.auto_preview = v; onSave(0); }} />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_filter_bar')}</div>
      <div class="t-desc">{tr(lang, 'desc_filter_bar')}</div>
    </div>
    <Switch checked={cfg.show_filter_bar} onchange={(v) => { cfg.show_filter_bar = v; onSave(0); }} />
  </div>
</div>

<div class="t-card" style="margin-top:14px">
  <div class="t-row">
    <div>
      <div class="t-label">{isEn ? 'Desensitized Config Backup & Migration' : '脱敏配置备份与一键换机迁移'}</div>
      <div class="t-desc">
        {isEn
          ? 'Exports config.toml & snippets.toml as a single JSON (DPAPI API keys are stripped). Imports run full sanitization.'
          : '单文件 JSON 导出 config.toml 与 snippets.toml（自动剥离本机 DPAPI API 密钥），导入经安全校验后即时生效。'}
      </div>
    </div>
    <div class="bak-acts">
      <Button variant="secondary" size="sm" class="btn-export-backup" onclick={() => postHost({ cmd: 'export_config_backup' })}>
        <Download size={13} />
        {isEn ? 'Export' : '导出备份'}
      </Button>
      <Button variant="secondary" size="sm" onclick={() => fileInput?.click()}>
        <Upload size={13} />
        {isEn ? 'Import' : '导入恢复'}
      </Button>
      <input bind:this={fileInput} type="file" accept=".json,application/json" style="display:none" onchange={onPickBackupFile} />
    </div>
  </div>
</div>

<style>
  .tab-head { margin-bottom: 18px; }
  .tab-head h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; letter-spacing: -0.01em; }
  .tab-lead { margin: 0; font-size: 12.5px; color: var(--muted); }
  .bak-acts { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }
</style>
