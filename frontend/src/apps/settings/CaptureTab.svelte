<script lang="ts">
  import { Camera, Languages, Pin, RotateCcw, ScanText } from '@lucide/svelte';
  import { defaultCaptureToolHotkeys } from '../../lib/defaults';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { CaptureToolHotkeys, HotkeySlotStatus, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import PathPicker from '../../lib/ui/PathPicker.svelte';
  import Slider from '../../lib/ui/Slider.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    capHotkeyStatus?: HotkeySlotStatus;
    onJumpDoctor?: () => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, capHotkeyStatus, onJumpDoctor, onSave }: Props = $props();
  let isEn = $derived(lang === 'en');
  let editingToolKey = $state<keyof CaptureToolHotkeys | null>(null);

  const capPresets = [
    { value: 'F1', label: 'F1（推荐单键）' },
    { value: 'PrintScreen', label: 'PrintScreen' },
    { value: 'Alt+S', label: 'Alt+S' },
    { value: 'Alt+1', label: 'Alt+1' },
    { value: 'Alt+A', label: 'Alt+A' },
    { value: 'Ctrl+Shift+A', label: 'Ctrl+Shift+A' },
  ];

  const toolKeyItems: Array<{ key: keyof CaptureToolHotkeys; zh: string; en: string }> = [
    { key: 'rect', zh: '矩形框', en: 'Rect' },
    { key: 'ellipse', zh: '椭圆圈', en: 'Ellipse' },
    { key: 'arrow', zh: '箭头 / 弯曲箭', en: 'Arrow' },
    { key: 'pen', zh: '平滑画笔', en: 'Pen' },
    { key: 'highlighter', zh: '荧光高亮笔', en: 'Highlighter' },
    { key: 'mosaic', zh: '马赛克 / 模糊', en: 'Mosaic / Blur' },
    { key: 'step', zh: '序号标号', en: 'Step Badge' },
    { key: 'text', zh: '文字批注', en: 'Text' },
    { key: 'scroll', zh: '纵/横向长截图', en: 'Long Scroll' },
    { key: 'pin', zh: '桌面贴图', en: 'Pin' },
    { key: 'ocr', zh: '截图识字 OCR', en: 'OCR' },
    { key: 'translate', zh: '截图翻译', en: 'Translate' },
  ];

  function launch(mode: string) {
    postHost({ cmd: 'start_capture', mode });
  }

  function onToolKeyDown(k: keyof CaptureToolHotkeys, e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      editingToolKey = null;
      return;
    }
    const ch = (e.key || '').trim().toUpperCase();
    if (/^[A-Z0-9]$/.test(ch)) {
      // Swap cleanly if another tool already holds this single key
      const prev = cfg.capture.tool_hotkeys[k];
      for (const item of toolKeyItems) {
        if (item.key !== k && cfg.capture.tool_hotkeys[item.key].toUpperCase() === ch) {
          cfg.capture.tool_hotkeys[item.key] = prev;
        }
      }
      cfg.capture.tool_hotkeys[k] = ch;
      editingToolKey = null;
      onSave(0);
    }
  }

  function resetToolHotkeys() {
    cfg.capture.tool_hotkeys = defaultCaptureToolHotkeys();
    editingToolKey = null;
    onSave(0);
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
      {onJumpDoctor}
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

<div class="t-card hk-card" style="margin-top:14px">
  <div class="hk-head">
    <div>
      <div class="t-label">{isEn ? 'In-Capture Tool Shortcuts (Click & Press A–Z / 0–9)' : '截图内小工具快捷键（点击后按任意字母或数字即改）'}</div>
      <div class="t-desc">{isEn ? 'Duplicate keys swap automatically; tooltips inside screenshot overlay update live.' : '冲突键自动互换，截图工具栏悬停提示与按键实时同步。'}</div>
    </div>
    <Button variant="ghost" size="sm" onclick={resetToolHotkeys}>
      <RotateCcw size={12} />
      {isEn ? 'Reset Defaults' : '恢复默认'}
    </Button>
  </div>

  <div class="tool-hk-grid">
    {#each toolKeyItems as item}
      {@const isEditing = editingToolKey === item.key}
      <div class="tool-hk-item">
        <span class="tool-hk-lbl">{isEn ? item.en : item.zh}</span>
        <button
          type="button"
          class="tool-hk-btn {isEditing ? 'is-edit' : ''}"
          onclick={() => (editingToolKey = item.key)}
          onblur={() => { if (editingToolKey === item.key) editingToolKey = null; }}
          onkeydown={(e) => isEditing && onToolKeyDown(item.key, e)}
        >
          {isEditing ? (isEn ? 'Key…' : '按键…') : (cfg.capture.tool_hotkeys[item.key] || '-')}
        </button>
      </div>
    {/each}
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
  .hk-card { padding: 14px 16px; }
  .hk-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 10px; margin-bottom: 12px; }
  .tool-hk-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
  }
  .tool-hk-item {
    display: flex; align-items: center; justify-content: space-between; gap: 6px;
    padding: 6px 10px; border-radius: 7px; border: 1px solid var(--border); background: var(--canvas);
  }
  .tool-hk-lbl { font-size: 12px; color: var(--text-secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tool-hk-btn {
    min-width: 34px; height: 24px; padding: 0 7px; border-radius: 5px;
    border: 1px solid var(--border-strong); background: var(--surface); color: var(--text);
    font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; font-weight: 600;
    cursor: pointer; white-space: nowrap; flex-shrink: 0;
  }
  .tool-hk-btn:hover { border-color: var(--accent); color: var(--accent); }
  .tool-hk-btn.is-edit {
    border-color: var(--accent); background: var(--accent-soft); color: var(--accent);
    box-shadow: 0 0 0 2px var(--ring);
  }
  .tips-card { padding: 14px 16px; }
  .tips-body { margin: 6px 0 0; font-size: 12px; color: var(--muted); line-height: 1.7; }
</style>
