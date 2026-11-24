<script lang="ts">
  import { Camera, Languages, Pin, RotateCcw, ScanText, Video } from '@lucide/svelte';
  import { defaultCaptureToolHotkeys } from '../../lib/defaults';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { CaptureToolHotkeys, HotkeySlotStatus, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import PathPicker from '../../lib/ui/PathPicker.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Slider from '../../lib/ui/Slider.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    capHotkeyStatus?: HotkeySlotStatus;
    pinHotkeyStatus?: HotkeySlotStatus;
    ocrHotkeyStatus?: HotkeySlotStatus;
    recordHotkeyStatus?: HotkeySlotStatus;
    showRecord?: boolean;
    embedded?: boolean;
    onJumpDoctor?: () => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, capHotkeyStatus, pinHotkeyStatus, ocrHotkeyStatus, recordHotkeyStatus, showRecord = true, embedded = false, onJumpDoctor, onSave }: Props = $props();
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

  const recordPresets = [
    { value: 'Alt+Shift+R', label: 'Alt+Shift+R（推荐）' },
    { value: 'Ctrl+Shift+R', label: 'Ctrl+Shift+R' },
    { value: 'Alt+R', label: 'Alt+R' },
    { value: 'F9', label: 'F9' },
  ];

  let toolKeyItems = $derived<Array<{ key: keyof CaptureToolHotkeys; zh: string; en: string }>>([
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
    ...(showRecord ? [{ key: 'record' as const, zh: '区域录屏', en: 'Record Video/GIF' }] : []),
  ]);

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

{#if !embedded}
<div class="tab-head">
  <h2>{tr(lang, 'h_capture')}</h2>
  <p class="tab-lead">{tr(lang, 'lead_capture')}</p>
</div>
{/if}

<div class="quick-bar">
  <Button variant="primary" onclick={() => launch('region')}>
    <Camera size={14} />
    {tr(lang, 'btn_cap_region')}
  </Button>
  {#if showRecord}
    <Button variant="secondary" onclick={() => launch('record')}>
      <Video size={14} />
      {isEn ? 'Record Region' : '区域录屏'}
    </Button>
  {/if}
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

  {#if showRecord}
  <div class="t-row">
    <div>
      <div class="t-label">{isEn ? 'Global Screen Recording Hotkey' : '全局区域录屏快捷键'}</div>
      <div class="t-desc">{isEn ? 'Direct region MP4/GIF recording (press again to stop & save)' : '一键进入区域录屏模式（录制中再次按下即可结束并自动复制文件）'}</div>
    </div>
    <HotkeyRecorder
      value={cfg.capture.record_hotkey || ''}
      presets={recordPresets}
      allowDisable={true}
      status={recordHotkeyStatus}
      presetPlaceholder={tr(lang, 'preset_opt')}
      disabledLabel={tr(lang, 'hk_off')}
      okLabel={tr(lang, 'hk_ok')}
      conflictLabel={tr(lang, 'hk_conflict')}
      recordPrompt={tr(lang, 'rec_prompt')}
      needModPrompt={tr(lang, 'rec_need_mod')}
      {onJumpDoctor}
      onchange={(v) => { cfg.capture.record_hotkey = v; onSave(0); }}
    />
  </div>
  {/if}

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

  {#if showRecord}
  <div class="t-row">
    <div>
      <div class="t-label">{isEn ? 'Default Recording Format & FPS' : '录屏默认格式与帧率'}</div>
      <div class="t-desc">{isEn ? 'Can also be switched live inside the capture recording bar' : '选区工具栏内也可按需随时切换，MP4 支持系统声与麦克风，GIF 自动调色盘降噪'}</div>
    </div>
    <div style="display:flex;gap:8px;flex-wrap:wrap">
      <Segmented
        value={cfg.capture.record_format || 'mp4'}
        options={[{ value: 'mp4', label: 'MP4' }, { value: 'gif', label: 'GIF' }]}
        onchange={(v) => { cfg.capture.record_format = v; onSave(0); }}
      />
      <Segmented
        value={String(cfg.capture.record_fps || 30)}
        options={[{ value: '15', label: '15 fps' }, { value: '30', label: '30 fps' }, { value: '60', label: '60 fps' }]}
        onchange={(v) => { cfg.capture.record_fps = Number(v); onSave(0); }}
      />
    </div>
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{isEn ? 'Recording Audio & Cursor' : '录屏音频与鼠标指针'}</div>
      <div class="t-desc">{isEn ? 'MP4 mode captures system loopback or microphone; GIF is silent' : 'MP4 模式下可选系统声音或麦克风（GIF 仅录画面）'}</div>
    </div>
    <div style="display:flex;align-items:center;gap:10px;flex-wrap:wrap">
      <Segmented
        value={cfg.capture.record_audio || 'none'}
        options={[
          { value: 'none', label: isEn ? 'Muted' : '静音' },
          { value: 'system', label: isEn ? 'System' : '系统声' },
          { value: 'mic', label: isEn ? 'Mic' : '麦克风' },
          { value: 'both', label: isEn ? 'Both' : '双音混录' },
        ]}
        onchange={(v) => { cfg.capture.record_audio = v; onSave(0); }}
      />
      <Switch checked={cfg.capture.record_cursor !== false} onchange={(v) => { cfg.capture.record_cursor = v; onSave(0); }} />
    </div>
  </div>
  {/if}
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
  .quick-bar { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 14px; }
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
