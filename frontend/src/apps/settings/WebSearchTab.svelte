<script lang="ts">
  import { Trash2 } from '@lucide/svelte';
  import { tr, type Lang } from '../../lib/i18n';
  import type { TomoyoConfig, WebSearchEngine } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import Select from '../../lib/ui/Select.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, onSave }: Props = $props();

  const RESERVED = new Set(['f', 'ev', 'cb', 'tr', 'fy', 'cap', 'shot', 'jt', 'pin', 'jtocr', 'jtfy', 'w', 'kill', 'ps', 'snip', '=', 'b64', 'b64d']);

  const PRESETS: Record<string, WebSearchEngine> = {
    bing: { keyword: 'bing', name: 'Bing Search', url: 'https://www.bing.com/search?q={q}' },
    wiki: { keyword: 'wiki', name: 'Wikipedia', url: 'https://en.wikipedia.org/wiki/Special:Search?search={q}' },
    bili: { keyword: 'bili', name: 'Bilibili', url: 'https://search.bilibili.com/all?keyword={q}' },
    npm: { keyword: 'npm', name: 'npm Packages', url: 'https://www.npmjs.com/search?q={q}' },
    pypi: { keyword: 'pypi', name: 'PyPI Python', url: 'https://pypi.org/search/?q={q}' },
    so: { keyword: 'so', name: 'StackOverflow', url: 'https://stackoverflow.com/search?q={q}' },
    custom: { keyword: 'my', name: 'Custom Search', url: 'https://example.com/search?q={q}' },
  };

  const presetOptions = [
    { value: 'bing', label: 'Bing (bing)' },
    { value: 'wiki', label: 'Wikipedia (wiki)' },
    { value: 'bili', label: 'Bilibili (bili)' },
    { value: 'npm', label: 'npm (npm)' },
    { value: 'pypi', label: 'PyPI (pypi)' },
    { value: 'so', label: 'StackOverflow (so)' },
    { value: 'custom', label: '自定义空白项…' },
  ];

  function isValidKw(kw: string): boolean {
    const k = kw.trim().toLowerCase();
    return /^[a-z0-9_-]{1,16}$/.test(k) && !RESERVED.has(k);
  }

  function isValidUrl(url: string): boolean {
    const u = url.trim();
    return /^https?:\/\/[^\s]{4,500}$/i.test(u) && u.includes('{q}');
  }

  let hasInvalid = $derived(
    cfg.web_searches.some((w) => !isValidKw(w.keyword) || !w.name.trim() || !isValidUrl(w.url))
  );

  function addPreset(key: string) {
    const p = PRESETS[key];
    if (!p || cfg.web_searches.length >= 24) return;
    if (key !== 'custom' && cfg.web_searches.some((w) => w.keyword === p.keyword)) return;
    cfg.web_searches = [...cfg.web_searches, { ...p }];
    if (!hasInvalid) onSave(0);
  }

  function removeRow(idx: number) {
    cfg.web_searches = cfg.web_searches.filter((_, i) => i !== idx);
    if (!hasInvalid) onSave(0);
  }

  function onEdit() {
    if (!hasInvalid) onSave(320);
  }
</script>

<div class="tab-head">
  <h2>{tr(lang, 'h_web')}</h2>
  <p class="tab-lead">{tr(lang, 'lead_web')}</p>
</div>

<div class="t-card web-card">
  <div class="web-bar">
    <span class="t-desc">{tr(lang, 'web_preset_hint')}</span>
    <Select
      value=""
      options={presetOptions}
      placeholder={tr(lang, 'web_preset_opt')}
      align="right"
      minWidth="172px"
      onchange={addPreset}
    />
  </div>

  <div class="web-grid web-head">
    <span>{tr(lang, 'th_kw')}</span>
    <span>{tr(lang, 'th_name')}</span>
    <span>{tr(lang, 'th_url')}</span>
    <span></span>
  </div>

  <div class="web-rows">
    {#each cfg.web_searches as row, idx}
      <div class="web-grid web-row">
        <Input
          size="sm"
          mono
          maxlength={16}
          bind:value={row.keyword}
          invalid={!isValidKw(row.keyword)}
          oninput={onEdit}
        />
        <Input
          size="sm"
          maxlength={48}
          bind:value={row.name}
          invalid={!row.name.trim()}
          oninput={onEdit}
        />
        <Input
          size="sm"
          type="url"
          mono
          maxlength={300}
          bind:value={row.url}
          invalid={!isValidUrl(row.url)}
          oninput={onEdit}
        />
        <Button variant="danger" size="sm" onclick={() => removeRow(idx)}>
          <Trash2 size={13} />
          {tr(lang, 'del')}
        </Button>
      </div>
    {/each}
  </div>

  {#if hasInvalid}
    <div class="web-err">
      {lang === 'en'
        ? 'Fix invalid rows (keyword: 1–16 a-z0-9_- not conflicting with built-ins; URL: http(s):// with {q}).'
        : '存在格式错误的行（关键词 1–16 位英文数字且不可与内置前缀冲突；URL 需为 https:// 或 http:// 且含 {q}）。'}
    </div>
  {/if}
</div>

<style>
  .web-card { padding: 14px 16px; }
  .web-bar {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding-bottom: 12px; border-bottom: 1px solid var(--border); margin-bottom: 10px;
  }
  .web-grid {
    display: grid;
    grid-template-columns: 84px 146px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
  }
  .web-head {
    font-size: 11.5px; font-weight: 600; color: var(--muted);
    padding: 0 2px 6px; border-bottom: 1px solid var(--border);
  }
  .web-rows { display: flex; flex-direction: column; gap: 7px; margin-top: 8px; }
  .web-err { margin-top: 10px; font-size: 12px; color: var(--danger); }
</style>
