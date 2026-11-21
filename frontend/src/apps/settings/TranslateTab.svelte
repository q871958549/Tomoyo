<script lang="ts">
  import { KeyRound, Languages, ShieldCheck, Trash2 } from '@lucide/svelte';
  import { postHost } from '../../lib/ipc';
  import { tr, type Lang } from '../../lib/i18n';
  import type { HotkeySlotStatus, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import Select from '../../lib/ui/Select.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    secretsStatus: { deepl: boolean; openai: boolean };
    trHotkeyStatus?: HotkeySlotStatus;
    onJumpDoctor?: () => void;
    onToast: (msg: string, warn?: boolean) => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, secretsStatus, trHotkeyStatus, onJumpDoctor, onToast, onSave }: Props = $props();

  let deeplDraft = $state('');
  let openaiDraft = $state('');

  const trPresets = [
    { value: 'Alt+T', label: 'Alt+T' },
    { value: 'Alt+D', label: 'Alt+D' },
    { value: 'Ctrl+Shift+D', label: 'Ctrl+Shift+D' },
    { value: 'Ctrl+Alt+T', label: 'Ctrl+Alt+T' },
  ];

  const langOptions = [
    { value: 'auto', label: 'Auto (中 ⇄ 英)' },
    { value: 'zh-Hans', label: '简体中文' },
    { value: 'en', label: 'English' },
    { value: 'ja', label: '日本語' },
    { value: 'ko', label: '한국어' },
    { value: 'fr', label: 'Français' },
    { value: 'de', label: 'Deutsch' },
  ];

  const engineMeta = [
    { id: 'google', zh: 'Google 翻译（免费）', en: 'Google Translate (Free)' },
    { id: 'mymemory', zh: 'MyMemory + 有道词典', en: 'MyMemory + Youdao Dict' },
    { id: 'deepl', zh: 'DeepL API（需密钥）', en: 'DeepL API (Key)' },
    { id: 'openai', zh: 'OpenAI 兼容接口（需密钥）', en: 'OpenAI-Compatible (Key)' },
    { id: 'ollama', zh: '本地 Ollama 离线大模型', en: 'Local Ollama (Offline)' },
  ];

  function toggleEngine(id: string) {
    const list = cfg.translate.engines.slice();
    const idx = list.indexOf(id);
    if (idx >= 0) {
      if (list.length <= 1) return;
      list.splice(idx, 1);
    } else {
      list.push(id);
    }
    cfg.translate.engines = list;
    onSave(0);
  }

  function saveSecret(key: 'deepl_api_key' | 'openai_api_key') {
    const raw = (key === 'deepl_api_key' ? deeplDraft : openaiDraft).trim();
    if (!raw) {
      onToast(lang === 'en' ? 'Enter a valid API key first' : '请输入有效的 API Key 后再保存', true);
      return;
    }
    if (key === 'deepl_api_key') deeplDraft = '';
    else openaiDraft = '';
    postHost({ cmd: 'set_secret', key, value: raw });
  }
</script>

<div class="tab-head-bar">
  <div>
    <h2>{tr(lang, 'h_translate')}</h2>
    <p class="tab-lead">{tr(lang, 'lead_translate')}</p>
  </div>
  <Button variant="primary" onclick={() => postHost({ cmd: 'open_translator', text: '', target_lang: '' })}>
    <Languages size={14} />
    {tr(lang, 'btn_open_tr')}
  </Button>
</div>

<div class="t-card">
  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_tr_hk')}</div>
      <div class="t-desc">{tr(lang, 'desc_tr_hk')}</div>
    </div>
    <HotkeyRecorder
      value={cfg.translate.hotkey}
      presets={trPresets}
      allowDisable={true}
      status={trHotkeyStatus}
      presetPlaceholder={tr(lang, 'preset_opt')}
      disabledLabel={tr(lang, 'hk_off')}
      okLabel={tr(lang, 'hk_ok')}
      conflictLabel={tr(lang, 'hk_conflict')}
      recordPrompt={tr(lang, 'rec_prompt')}
      needModPrompt={tr(lang, 'rec_need_mod')}
      {onJumpDoctor}
      onchange={(v) => { cfg.translate.hotkey = v; onSave(0); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_tr_tgt')}</div>
      <div class="t-desc">{tr(lang, 'desc_tr_tgt')}</div>
    </div>
    <Select
      value={cfg.translate.target_lang || 'auto'}
      options={langOptions}
      align="right"
      minWidth="164px"
      onchange={(v) => { cfg.translate.target_lang = v; onSave(0); }}
    />
  </div>

  <div class="t-row col-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_tr_eng')}</div>
      <div class="t-desc">{tr(lang, 'desc_tr_eng')}</div>
    </div>
    <div class="chip-grid">
      {#each engineMeta as m}
        {@const on = cfg.translate.engines.includes(m.id)}
        <button type="button" class="t-chip {on ? 'is-on' : ''}" onclick={() => toggleEngine(m.id)}>
          {on ? '✓ ' : '+ '}{lang === 'en' ? m.en : m.zh}
        </button>
      {/each}
    </div>
  </div>
</div>

<!-- DPAPI Encrypted Secrets & Endpoints -->
<div class="t-card" style="margin-top:14px">
  <div class="t-row col-row">
    <div class="key-head">
      <div class="key-title">
        <KeyRound size={14} />
        <span class="t-label">DeepL API Key</span>
        <span class="sec-badge {secretsStatus.deepl ? 'is-ok' : ''}">
          {#if secretsStatus.deepl}<ShieldCheck size={12} />{/if}
          {secretsStatus.deepl ? tr(lang, 'tr_secret_saved') : tr(lang, 'tr_secret_none')}
        </span>
      </div>
      {#if secretsStatus.deepl}
        <Button variant="danger" size="sm" onclick={() => postHost({ cmd: 'delete_secret', key: 'deepl_api_key' })}>
          <Trash2 size={13} />
          {tr(lang, 'btn_clear_key')}
        </Button>
      {/if}
    </div>
    <div class="field-row">
      <Input
        type="password"
        bind:value={deeplDraft}
        placeholder="输入 DeepL Free (:fx) 或 Pro Key（Windows DPAPI 加密写入）"
      />
      <Button variant="primary" onclick={() => saveSecret('deepl_api_key')}>
        {tr(lang, 'btn_save_key')}
      </Button>
    </div>
  </div>

  <div class="t-row col-row">
    <div class="key-head">
      <div class="key-title">
        <KeyRound size={14} />
        <span class="t-label">OpenAI 兼容接口 Key</span>
        <span class="sec-badge {secretsStatus.openai ? 'is-ok' : ''}">
          {#if secretsStatus.openai}<ShieldCheck size={12} />{/if}
          {secretsStatus.openai ? tr(lang, 'tr_secret_saved') : tr(lang, 'tr_secret_none')}
        </span>
      </div>
      {#if secretsStatus.openai}
        <Button variant="danger" size="sm" onclick={() => postHost({ cmd: 'delete_secret', key: 'openai_api_key' })}>
          <Trash2 size={13} />
          {tr(lang, 'btn_clear_key')}
        </Button>
      {/if}
    </div>
    <div class="field-row">
      <Input
        type="password"
        bind:value={openaiDraft}
        placeholder="输入 OpenAI / DeepSeek / 通义千问等兼容 API Key"
      />
      <Button variant="primary" onclick={() => saveSecret('openai_api_key')}>
        {tr(lang, 'btn_save_key')}
      </Button>
    </div>
    <div class="field-row">
      <Input
        type="url"
        mono
        bind:value={cfg.translate.openai_base_url}
        placeholder="https://api.openai.com/v1"
        onchange={() => onSave(0)}
      />
      <div style="width:190px;flex-shrink:0">
        <Input
          type="text"
          mono
          maxlength={64}
          bind:value={cfg.translate.openai_model}
          placeholder="gpt-4o-mini"
          onchange={() => onSave(0)}
        />
      </div>
    </div>
  </div>

  <div class="t-row col-row">
    <div class="t-label">{tr(lang, 'lbl_tr_ollama')}</div>
    <div class="field-row">
      <Input
        type="url"
        mono
        bind:value={cfg.translate.ollama_url}
        placeholder="http://127.0.0.1:11434"
        onchange={() => onSave(0)}
      />
      <div style="width:190px;flex-shrink:0">
        <Input
          type="text"
          mono
          maxlength={64}
          bind:value={cfg.translate.ollama_model}
          placeholder="qwen2.5:7b"
          onchange={() => onSave(0)}
        />
      </div>
    </div>
  </div>
</div>

<style>
  .tab-head-bar { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; margin-bottom: 18px; }
  .tab-head-bar h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
  .tab-lead { margin: 0; font-size: 12.5px; color: var(--muted); }
  .col-row { flex-direction: column; align-items: stretch; gap: 10px; }
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
  .key-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .key-title { display: flex; align-items: center; gap: 7px; color: var(--text-secondary); }
  .sec-badge {
    display: inline-flex; align-items: center; gap: 4px; padding: 2px 8px; border-radius: 999px;
    background: var(--subtle); color: var(--muted); font-size: 11px; font-weight: 500;
  }
  .sec-badge.is-ok { background: var(--ok-soft); color: var(--ok); }
  .field-row { display: flex; align-items: center; gap: 8px; width: 100%; }
</style>
