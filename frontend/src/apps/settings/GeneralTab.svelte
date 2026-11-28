<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
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
    syncDir: string;
    hotkeyStatus?: HotkeySlotStatus;
    onLanguageChange: (code: Lang) => void;
    onAutostartChange: (checked: boolean) => void;
    onJumpDoctor?: () => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, autostart, syncDir, hotkeyStatus, onLanguageChange, onAutostartChange, onJumpDoctor, onSave }: Props = $props();
  let fileInput: HTMLInputElement | undefined = $state();
  let isEn = $derived(lang === 'en');
  let sp = $derived(
    cfg.super_panel ?? {
      enabled: true,
      trigger_mode: 'right_hold',
      hold_ms: 320,
      auto_translate_selection: true,
      excluded_apps: [],
    }
  );

  function patchSp(patch: Partial<NonNullable<TomoyoConfig['super_panel']>>, delayMs = 0) {
    cfg.super_panel = { ...sp, ...patch };
    onSave(delayMs);
  }

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
      <div class="t-label">{isEn ? 'Native Super Panel (Mouse Trigger & Inline Translate)' : '原生超级面板（鼠标长按唤起 · 选中文本即刻翻译）'}</div>
      <div class="t-desc">
        {isEn
          ? 'Zero-WebView cursor popup that losslessly detects selected text/files and streams instant translations; auto-yields in fullscreen & remote desktop.'
          : '零 WebView 原生光标浮层面板：无痕读取选中文本/文件并直出内联译文，自动遵循全屏与远程桌面避让名单。'}
      </div>
    </div>
    <Switch checked={sp.enabled} onchange={(v) => patchSp({ enabled: v })} />
  </div>

  {#if sp.enabled}
    <div class="t-row">
      <div>
        <div class="t-label">{isEn ? 'Trigger Gesture' : '唤起手势（右键 / 中键任选其一）'}</div>
        <div class="t-desc">{isEn ? 'Hold modes keep quick right-click & browser middle-click intact; moving >8px cancels.' : '长按模式不干扰短按右键菜单与浏览器中键标签页；移动超 8px 自动取消。'}</div>
      </div>
      <Segmented
        value={sp.trigger_mode || 'right_hold'}
        options={[
          { value: 'right_hold', label: isEn ? 'Right Hold' : '长按右键' },
          { value: 'middle_hold', label: isEn ? 'Middle Hold' : '长按中键' },
          { value: 'middle_click', label: isEn ? 'Middle Click' : '单击中键' },
        ]}
        onchange={(v) => patchSp({ trigger_mode: v })}
      />
    </div>

    {#if (sp.trigger_mode || 'right_hold') !== 'middle_click'}
      <div class="t-row">
        <div>
          <div class="t-label">{isEn ? 'Long-Press Duration' : '长按触发时长'}</div>
          <div class="t-desc">{isEn ? 'Stationary hold time before Super Panel pops up (180–800 ms).' : '鼠标静止按住达到该时长后自动呼出（推荐 280–360 ms）。'}</div>
        </div>
        <Slider
          value={sp.hold_ms}
          min={180}
          max={800}
          step={20}
          format={(v) => `${v} ms`}
          oninput={(v) => patchSp({ hold_ms: v }, 160)}
        />
      </div>
    {/if}

    <div class="t-row">
      <div>
        <div class="t-label">{isEn ? 'Auto Inline Translate on Text Selection' : '选中文本时自动内联翻译'}</div>
        <div class="t-desc">{isEn ? 'Streams multi-engine translation right inside the top of Super Panel with one-click copy.' : '检测到选中文本时在顶部极简卡片直出译文，点击译文即可复制。'}</div>
      </div>
      <Switch checked={sp.auto_translate_selection} onchange={(v) => patchSp({ auto_translate_selection: v })} />
    </div>
  {/if}
</div>

<div class="t-card" style="margin-top:14px">
  <div class="t-row">
    <div style="min-width:0">
      <div class="t-label">{isEn ? 'Zero-Account Local Sync Folder (OneDrive / Nutstore / Git)' : '免账号本地同步目录（OneDrive / 坚果云 / Git）'}</div>
      <div class="t-desc">
        {syncDir
          ? (isEn ? `Active sync root: ${syncDir} (DPAPI keys & clipboard DB stay local)` : `当前同步目录：${syncDir}（API 密钥与剪贴板库严格留在本机）`)
          : (isEn ? 'Store config.toml, snippets.toml & notes/ in any local sync folder for instant multi-PC sync.' : '将 config.toml、snippets.toml 与备忘录目录托管至本地网盘/Git 文件夹，无账号跨机同步。')}
      </div>
    </div>
    <div class="bak-acts">
      <Button variant="secondary" size="sm" class="btn-pick-sync" onclick={() => postHost({ cmd: 'pick_sync_dir' })}>
        <Icon name="FolderSync" size={13} />
        {isEn ? 'Choose Folder…' : '选择目录…'}
      </Button>
      {#if syncDir}
        <Button variant="ghost" size="sm" onclick={() => postHost({ cmd: 'clear_sync_dir' })}>
          <Icon name="RotateCcw" size={13} />
          {isEn ? 'Local Default' : '恢复默认'}
        </Button>
      {/if}
    </div>
  </div>
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
        <Icon name="Download" size={13} />
        {isEn ? 'Export' : '导出备份'}
      </Button>
      <Button variant="secondary" size="sm" onclick={() => fileInput?.click()}>
        <Icon name="Upload" size={13} />
        {isEn ? 'Import' : '导入恢复'}
      </Button>
      <input bind:this={fileInput} type="file" accept=".json,application/json" style="display:none" onchange={onPickBackupFile} />
    </div>
  </div>
</div>

<style>
  .bak-acts { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }
</style>
