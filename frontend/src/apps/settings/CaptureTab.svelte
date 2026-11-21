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
    pinHotkeyStatus?: HotkeySlotStatus;
    ocrHotkeyStatus?: HotkeySlotStatus;
    onJumpDoctor?: () => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, capHotkeyStatus, pinHotkeyStatus, ocrHotkeyStatus, onJumpDoctor, onSave }: Props = $props();
  let isEn = $derived(lang === 'en');
  let editingToolKey = $state<keyof CaptureToolHotkeys | null>(null);
  let toolKeyHint = $state('');

  const capPresets = [
    { value: 'F1', label: 'F1（推荐单键）' },
    { value: 'PrintScreen', label: 'PrintScreen' },
    { value: 'Alt+S', label: 'Alt+S' },
    { value: 'Alt+1', label: 'Alt+1' },
    { value: 'Alt+A', label: 'Alt+A' },
    { value: 'Ctrl+Shift+A', label: 'Ctrl+Shift+A' },
  ];

  const pinPresets = [
    { value: 'F3', label: 'F3（推荐单键）' },
    { value: 'F4', label: 'F4' },
    { value: 'Alt+3', label: 'Alt+3' },
    { value: 'Alt+P', label: 'Alt+P' },
    { value: 'Ctrl+Shift+P', label: 'Ctrl+Shift+P' },
    { value: 'Alt+2', label: 'Alt+2' },
  ];

  const ocrPresets = [
    { value: 'Alt+Shift+C', label: 'Alt+Shift+C（推荐）' },
    { value: 'Ctrl+Shift+O', label: 'Ctrl+Shift+O' },
    { value: 'Alt+O', label: 'Alt+O' },
    { value: 'F4', label: 'F4' },
    { value: 'Alt+2', label: 'Alt+2' },
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
    { key: 'ocr', zh: 'OCR 审校窗口', en: 'OCR Inspector' },
    { key: 'ocr_copy', zh: '直接提取文字', en: 'Extract Text' },
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
      toolKeyHint = '';
      return;
    }
    if (['Shift', 'Control', 'Alt', 'Meta'].includes(e.key)) {
      return;
    }
    if (/^F([1-9]|1[0-2])$/i.test(e.key) || e.key === 'PrintScreen') {
      editingToolKey = null;
      toolKeyHint = isEn
        ? `${e.key} is a global shortcut — set it in the Global Hotkey recorders above.`
        : `「${e.key}」属于全局快捷键，请在上方「全局截图 / 贴图 / 识字快捷键」录入框中设置。`;
      return;
    }
    const code = e.code || '';
    let main = '';
    if (code.startsWith('Key') && code.length === 4) main = code.slice(3).toUpperCase();
    else if (code.startsWith('Digit') && code.length === 6) main = code.slice(5);
    else if (/^[A-Za-z0-9]$/.test(e.key)) main = e.key.toUpperCase();
    if (main) {
      const parts: string[] = [];
      if (e.ctrlKey) parts.push('Ctrl');
      if (e.altKey) parts.push('Alt');
      if (e.shiftKey) parts.push('Shift');
      parts.push(main);
      const combo = parts.join('+');
      const prev = cfg.capture.tool_hotkeys[k];
      for (const item of toolKeyItems) {
        if (item.key !== k && cfg.capture.tool_hotkeys[item.key].toUpperCase() === combo.toUpperCase()) {
          cfg.capture.tool_hotkeys[item.key] = prev;
        }
      }
      cfg.capture.tool_hotkeys[k] = combo;
      editingToolKey = null;
      toolKeyHint = '';
      onSave(0);
    }
  }

  function resetToolHotkeys() {
    cfg.capture.tool_hotkeys = defaultCaptureToolHotkeys();
    editingToolKey = null;
    toolKeyHint = '';
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
      <div class="t-label">{tr(lang, 'lbl_pin_hk')}</div>
      <div class="t-desc">{tr(lang, 'desc_pin_hk')}</div>
    </div>
    <HotkeyRecorder
      value={cfg.capture.pin_hotkey}
      presets={pinPresets}
      allowDisable={true}
      status={pinHotkeyStatus}
      presetPlaceholder={tr(lang, 'preset_opt')}
      disabledLabel={tr(lang, 'hk_off')}
      okLabel={tr(lang, 'hk_ok')}
      conflictLabel={tr(lang, 'hk_conflict')}
      recordPrompt={tr(lang, 'rec_prompt')}
      needModPrompt={tr(lang, 'rec_need_mod')}
      {onJumpDoctor}
      onchange={(v) => { cfg.capture.pin_hotkey = v; onSave(0); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_ocr_hk')}</div>
      <div class="t-desc">{tr(lang, 'desc_ocr_hk')}</div>
    </div>
    <HotkeyRecorder
      value={cfg.capture.ocr_hotkey}
      presets={ocrPresets}
      allowDisable={true}
      status={ocrHotkeyStatus}
      presetPlaceholder={tr(lang, 'preset_opt')}
      disabledLabel={tr(lang, 'hk_off')}
      okLabel={tr(lang, 'hk_ok')}
      conflictLabel={tr(lang, 'hk_conflict')}
      recordPrompt={tr(lang, 'rec_prompt')}
      needModPrompt={tr(lang, 'rec_need_mod')}
      {onJumpDoctor}
      onchange={(v) => { cfg.capture.ocr_hotkey = v; onSave(0); }}
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
      <div class="t-label">{isEn ? 'In-Capture Canvas Tool Keys (A–Z / 0–9 or Shift/Ctrl/Alt Combos)' : '截图画布内小工具快捷键（支持单键或 Shift+C 等组合键）'}</div>
      <div class="t-desc">{isEn ? 'Works inside screenshot selection (e.g. Shift+C extracts & copies text directly); duplicate keys swap automatically.' : '仅截图框选后生效（默认 Shift+C 直接提取文字并复制，O 打开审校窗）；冲突键自动互换。'}</div>
    </div>
    <Button variant="ghost" size="sm" onclick={resetToolHotkeys}>
      <RotateCcw size={12} />
      {isEn ? 'Reset Defaults' : '恢复默认'}
    </Button>
  </div>

  {#if toolKeyHint}
    <div class="tool-hk-hint">{toolKeyHint}</div>
  {/if}

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
  .tool-hk-hint {
    margin-bottom: 10px; padding: 7px 10px; border-radius: 7px;
    background: var(--accent-soft); color: var(--accent); font-size: 12px; line-height: 1.45;
  }
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
