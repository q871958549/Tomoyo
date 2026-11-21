<script lang="ts">
  import { Plus, Tag, Trash2 } from '@lucide/svelte';
  import type { Lang } from '../../lib/i18n';
  import type { CustomFilterTab, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    onUpdateFilters: (hidden: string[], custom: CustomFilterTab[]) => void;
  }

  let { cfg, lang, onUpdateFilters }: Props = $props();

  let newLabel = $state('');
  let newRule = $state('');
  let formErr = $state('');

  let isEn = $derived(lang === 'en');

  const builtinTabs = [
    { id: 'app', zh: '应用', en: 'Apps' },
    { id: 'folder', zh: '文件夹', en: 'Folders' },
    { id: 'pdf', zh: 'PDF', en: 'PDF' },
    { id: 'excel', zh: 'Excel', en: 'Excel' },
    { id: 'word', zh: 'Word', en: 'Word' },
    { id: 'ppt', zh: 'PPT', en: 'PPT' },
    { id: 'image', zh: '图片', en: 'Images' },
    { id: 'archive', zh: '压缩', en: 'Zip' },
  ];

  const customPresets: Array<{
    id: string;
    zh: string;
    en: string;
    tag: string;
    extensions: string[];
  }> = [
    { id: 'code', zh: '代码', en: 'Code', tag: '@code', extensions: ['rs', 'ts', 'tsx', 'js', 'py', 'go', 'cpp', 'h', 'java', 'json', 'toml'] },
    { id: 'video', zh: '视频', en: 'Video', tag: '@video', extensions: ['mp4', 'mkv', 'avi', 'mov', 'webm', 'flv'] },
    { id: 'audio', zh: '音频', en: 'Audio', tag: '@audio', extensions: ['mp3', 'flac', 'wav', 'aac', 'm4a', 'ogg'] },
    { id: 'md', zh: 'Markdown', en: 'Markdown', tag: '@md', extensions: ['md', 'markdown', 'txt'] },
    { id: 'setup', zh: '安装包', en: 'Installers', tag: '@setup', extensions: ['exe', 'msi', 'msix', 'dmg', 'apk'] },
  ];

  let hiddenSet = $derived(new Set((cfg.hidden_builtin_filters || []).map((s) => s.toLowerCase())));
  let customTabs = $derived(cfg.custom_filters || []);

  function toggleBuiltin(id: string) {
    const next = new Set(hiddenSet);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onUpdateFilters([...next], [...customTabs]);
  }

  function hasPreset(id: string): boolean {
    return customTabs.some((t) => t.id.toLowerCase() === id);
  }

  function addPreset(p: (typeof customPresets)[number]) {
    if (hasPreset(p.id) || customTabs.length >= 12) return;
    onUpdateFilters([...hiddenSet], [
      ...customTabs,
      {
        id: p.id,
        label: isEn ? p.en : p.zh,
        tag: p.tag,
        extensions: [...p.extensions],
        everything_expr: '',
        enabled: true,
      },
    ]);
  }

  function addCustomTab() {
    formErr = '';
    const label = newLabel.trim().slice(0, 16);
    const rule = newRule.trim();
    if (!label) {
      formErr = isEn ? 'Enter a short tab name (e.g. Design)' : '请输入简短的标签名称（如：设计稿）';
      return;
    }
    if (!rule) {
      formErr = isEn ? 'Enter extensions (psd;fig) or an Everything expression' : '请输入扩展名（如 psd;fig）或 Everything 表达式';
      return;
    }
    if (customTabs.length >= 12) {
      formErr = isEn ? 'Up to 12 custom tabs are supported' : '最多支持添加 12 个自定义搜索标签';
      return;
    }

    let extensions: string[] = [];
    let everything_expr = '';
    if (rule.includes(':')) {
      everything_expr = rule.replace(/["']/g, '').slice(0, 120);
    } else {
      extensions = rule
        .split(/[\s,;]+/)
        .map((s) => s.trim().replace(/^\./, '').toLowerCase())
        .filter((s) => /^[a-z0-9]{1,16}$/.test(s))
        .slice(0, 24);
      if (extensions.length === 0) {
        formErr = isEn ? 'Provide valid alphanumeric extensions like rs;ts;py' : '请输入合法字母数字扩展名（如 rs;ts;py），或带冒号的 Everything 语法';
        return;
      }
    }

    const baseId = label.toLowerCase().replace(/[^a-z0-9_-]/g, '') || `tab_${customTabs.length + 1}`;
    let id = baseId;
    let n = 2;
    while (customTabs.some((t) => t.id.toLowerCase() === id)) {
      id = `${baseId}_${n++}`;
    }

    const tab: CustomFilterTab = {
      id,
      label,
      tag: `@${id}`,
      extensions,
      everything_expr,
      enabled: true,
    };
    onUpdateFilters([...hiddenSet], [...customTabs, tab]);
    newLabel = '';
    newRule = '';
  }

  function toggleCustom(idx: number, enabled: boolean) {
    onUpdateFilters([...hiddenSet], customTabs.map((t, i) => (i === idx ? { ...t, enabled } : t)));
  }

  function removeCustom(idx: number) {
    onUpdateFilters([...hiddenSet], customTabs.filter((_, i) => i !== idx));
  }

  function ruleSummary(t: CustomFilterTab): string {
    if (t.everything_expr?.trim()) return t.everything_expr.trim();
    return `ext:${(t.extensions || []).join(';')}`;
  }
</script>

<div class="t-card" style="margin-top:14px">
  <div class="t-row col-row">
    <div>
      <div class="t-label">
        {isEn ? 'Search Result Filter Tabs (Customizable & Extensible)' : '搜索结果分类标签（可隐藏内置 / 自由扩展）'}
      </div>
      <div class="t-desc">
        {isEn
          ? 'Shown above search results. Click tabs, press Ctrl+Tab / Ctrl+1..9, or type @tag to filter instantly.'
          : '显示在搜索栏上方：支持鼠标点击、Ctrl+Tab 轮换、Ctrl+1..9 直达，或直接输入 @标签名 过滤。'}
      </div>
    </div>
    <div class="chip-grid">
      <span class="t-chip is-locked">✓ {isEn ? 'All (Pinned)' : '全部（常驻）'}</span>
      {#each builtinTabs as b}
        {@const visible = !hiddenSet.has(b.id)}
        <button
          type="button"
          class="t-chip {visible ? 'is-on' : ''}"
          onclick={() => toggleBuiltin(b.id)}
        >
          {visible ? '✓ ' : '+ '}{isEn ? b.en : b.zh}
        </button>
      {/each}
    </div>
  </div>

  <div class="t-row col-row">
    <div>
      <div class="t-label">{isEn ? 'Custom Extension & Everything Tabs' : '用户自定义扩展标签'}</div>
      <div class="t-desc">
        {isEn
          ? 'One-click add presets or create custom tabs by file extension (rs;ts) or Everything syntax (dm:today).'
          : '一键添加常用预设，或按扩展名（如 psd;fig）与 Everything 表达式（如 dm:today）自由扩展。'}
      </div>
    </div>

    <div class="chip-grid">
      {#each customPresets as p}
        {@const added = hasPreset(p.id)}
        <button
          type="button"
          class="t-chip {added ? 'is-on' : ''}"
          disabled={added}
          data-preset-tab={p.id}
          onclick={() => addPreset(p)}
        >
          {added ? '✓ ' : '+ '}{isEn ? p.en : p.zh}
        </button>
      {/each}
    </div>

    <div class="add-row">
      <input
        class="t-input add-name"
        placeholder={isEn ? 'Tab name (e.g. Design)' : '标签名（如：设计稿）'}
        bind:value={newLabel}
        onkeydown={(e) => e.key === 'Enter' && addCustomTab()}
      />
      <input
        class="t-input add-rule"
        placeholder={isEn ? 'Extensions (psd;fig) or Everything expr (dm:today)' : '扩展名（psd;fig）或 Everything 表达式（dm:today）'}
        bind:value={newRule}
        onkeydown={(e) => e.key === 'Enter' && addCustomTab()}
      />
      <Button variant="secondary" size="sm" onclick={addCustomTab}>
        <Plus size={13} />
        {isEn ? 'Add Tab' : '添加标签'}
      </Button>
    </div>
    {#if formErr}
      <div class="err-msg">{formErr}</div>
    {/if}

    {#if customTabs.length > 0}
      <div class="custom-list">
        {#each customTabs as tab, idx}
          <div class="custom-item">
            <Tag size={13} class="tag-ico" />
            <span class="custom-name">{tab.label}</span>
            <span class="custom-tag">{tab.tag}</span>
            <span class="custom-rule" title={ruleSummary(tab)}>{ruleSummary(tab)}</span>
            <Switch checked={tab.enabled} onchange={(v) => toggleCustom(idx, v)} />
            <Button variant="danger" size="sm" onclick={() => removeCustom(idx)}>
              <Trash2 size={12} />
            </Button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .col-row { flex-direction: column; align-items: stretch; gap: 10px; }
  .chip-grid { display: flex; flex-wrap: wrap; gap: 6px; }
  .t-chip {
    height: 28px; padding: 0 10px; border-radius: 7px; border: 1px solid var(--border);
    background: var(--canvas); color: var(--text-secondary); font-family: inherit;
    font-size: 12px; cursor: pointer; white-space: nowrap; transition: all 0.12s ease;
    display: inline-flex; align-items: center;
  }
  .t-chip:hover:not(:disabled) { border-color: var(--border-strong); color: var(--text); }
  .t-chip:disabled { opacity: 0.72; cursor: default; }
  .t-chip.is-on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); font-weight: 500; }
  .t-chip.is-locked { background: var(--sidebar); color: var(--muted); cursor: default; }
  .add-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .add-name { width: 138px; flex-shrink: 0; }
  .add-rule { flex: 1; min-width: 180px; font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; }
  .err-msg { font-size: 12px; color: var(--danger); }
  .custom-list { display: flex; flex-direction: column; gap: 6px; }
  .custom-item {
    display: flex; align-items: center; gap: 8px; min-height: 36px; padding: 4px 8px 4px 11px;
    border-radius: 8px; background: var(--canvas); border: 1px solid var(--border);
  }
  :global(.tag-ico) { color: var(--accent); flex-shrink: 0; }
  .custom-name { font-size: 12.5px; font-weight: 500; white-space: nowrap; }
  .custom-tag {
    font-size: 11px; padding: 1px 6px; border-radius: 5px; background: var(--accent-soft);
    color: var(--accent); font-family: "Cascadia Mono", Consolas, monospace; white-space: nowrap;
  }
  .custom-rule {
    flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 11.5px; color: var(--muted); font-family: "Cascadia Mono", Consolas, monospace;
  }
</style>
