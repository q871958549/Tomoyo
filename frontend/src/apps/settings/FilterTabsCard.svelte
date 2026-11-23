<script lang="ts">
  import { ChevronDown, ChevronUp, Plus, SlidersHorizontal, Tag, Trash2, X } from '@lucide/svelte';
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
  let newTag = $state('');
  let newRule = $state('');
  let editingIdx = $state<number | null>(null);
  let previewId = $state('all');
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

  const customPresets = [
    { id: 'code', zh: '代码', en: 'Code', tag: '@code', extensions: ['rs', 'ts', 'tsx', 'js', 'py', 'go', 'cpp', 'h', 'java', 'json', 'toml'] },
    { id: 'video', zh: '视频', en: 'Video', tag: '@video', extensions: ['mp4', 'mkv', 'avi', 'mov', 'webm', 'flv'] },
    { id: 'audio', zh: '音频', en: 'Audio', tag: '@audio', extensions: ['mp3', 'flac', 'wav', 'aac', 'm4a', 'ogg'] },
    { id: 'md', zh: 'Markdown', en: 'Markdown', tag: '@md', extensions: ['md', 'markdown', 'txt'] },
    { id: 'setup', zh: '安装包', en: 'Installers', tag: '@setup', extensions: ['deb', 'rpm', 'appimage', 'exe', 'msi', 'dmg', 'apk'] },
  ];

  const quickSnippets = [
    { label: '设计稿', tag: '@design', rule: 'psd;fig;sketch;ai;xd' },
    { label: '电子书', tag: '@book', rule: 'epub;pdf;mobi;azw3' },
    { label: '配置文件', tag: '@conf', rule: 'json;yaml;yml;toml;ini' },
    { label: '今日文件', tag: '@today', rule: 'dm:today' },
  ];

  let hiddenSet = $derived(new Set((cfg.hidden_builtin_filters || []).map((s) => s.toLowerCase())));
  let customTabs = $derived(cfg.custom_filters || []);

  let previewTabs = $derived([
    { id: 'all', label: isEn ? 'All' : '全部', custom: false },
    ...builtinTabs.filter((b) => !hiddenSet.has(b.id)).map((b) => ({ id: b.id, label: isEn ? b.en : b.zh, custom: false })),
    ...customTabs.filter((t) => t.enabled).map((t) => ({ id: `c:${t.id}`, label: t.label, custom: true })),
  ]);

  let estBarPx = $derived(
    14 + previewTabs.reduce((sum, t) => sum + Math.max(44, Math.min(108, t.label.length * 11 + 22)) + 6, 0),
  );
  let barLimitPx = $derived((cfg.window?.width_dip || 720) - 24);

  let autoTagPreview = $derived(() => {
    const raw = newTag.trim().replace(/^@+/, '').replace(/\s+/g, '');
    if (raw) return `@${raw}`;
    const lbl = newLabel.trim().replace(/\s+/g, '');
    return lbl ? `@${lbl.slice(0, 14)}` : '@tag';
  });

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
      { id: p.id, label: isEn ? p.en : p.zh, tag: p.tag, extensions: [...p.extensions], everything_expr: '', enabled: true },
    ]);
  }

  function resetForm() {
    newLabel = '';
    newTag = '';
    newRule = '';
    editingIdx = null;
    formErr = '';
  }

  function startEdit(idx: number) {
    const t = customTabs[idx];
    if (!t) return;
    editingIdx = idx;
    newLabel = t.label;
    newTag = t.tag || '';
    newRule = t.everything_expr?.trim() || (t.extensions || []).join(';');
    formErr = '';
  }

  function applySnippet(s: (typeof quickSnippets)[number]) {
    if (!newLabel.trim()) newLabel = s.label;
    if (!newTag.trim()) newTag = s.tag;
    newRule = s.rule;
    formErr = '';
  }

  function saveCustomTab() {
    formErr = '';
    const label = newLabel.trim().slice(0, 16);
    const rule = newRule.trim();
    if (!label) {
      formErr = isEn ? 'Enter a short tab name (e.g. Design)' : '请输入简短的标签名称（如：设计稿）';
      return;
    }
    if (!rule) {
      formErr = isEn ? 'Enter extensions (psd;fig) or an expression (dm:today)' : '请输入扩展名（如 psd;fig）或规则表达式（如 dm:today）';
      return;
    }
    if (editingIdx === null && customTabs.length >= 12) {
      formErr = isEn ? 'Up to 12 custom tabs are supported' : '最多支持添加 12 个自定义搜索标签';
      return;
    }

    let extensions: string[] = [];
    let everything_expr = '';
    if (rule.includes(':')) {
      everything_expr = rule.replace(/["']/g, '').slice(0, 120);
      const m = everything_expr.toLowerCase().match(/(?:^|\s)ext:([a-z0-9;.,_-]+)/);
      if (m) {
        extensions = m[1].split(/[;,\s]+/).map((s) => s.replace(/^\./, '')).filter((s) => /^[a-z0-9]{1,16}$/.test(s)).slice(0, 24);
      }
    } else {
      extensions = rule.split(/[\s,;]+/).map((s) => s.trim().replace(/^\./, '').toLowerCase()).filter((s) => /^[a-z0-9]{1,16}$/.test(s)).slice(0, 24);
      if (extensions.length === 0) {
        formErr = isEn ? 'Provide valid alphanumeric extensions like rs;ts;py' : '请输入合法字母数字扩展名（如 rs;ts;py），或带冒号的规则语法';
        return;
      }
    }

    const cleanTagBody = (newTag.trim().replace(/^@+/, '').replace(/\s+/g, '') || label.replace(/\s+/g, '')).slice(0, 16);
    const tag = `@${cleanTagBody}`;

    if (editingIdx !== null && customTabs[editingIdx]) {
      const prev = customTabs[editingIdx];
      const updated: CustomFilterTab = { ...prev, label, tag, extensions, everything_expr };
      onUpdateFilters([...hiddenSet], customTabs.map((t, i) => (i === editingIdx ? updated : t)));
      resetForm();
      return;
    }

    const rawId = cleanTagBody.toLowerCase().replace(/[^a-z0-9_-]/g, '') || `tab_${customTabs.length + 1}`;
    let id = rawId;
    let n = 2;
    while (customTabs.some((t) => t.id.toLowerCase() === id)) id = `${rawId}_${n++}`;

    onUpdateFilters([...hiddenSet], [
      ...customTabs,
      { id, label, tag, extensions, everything_expr, enabled: true },
    ]);
    resetForm();
  }

  function moveCustom(idx: number, delta: -1 | 1) {
    const nextIdx = idx + delta;
    if (nextIdx < 0 || nextIdx >= customTabs.length) return;
    const arr = [...customTabs];
    const [picked] = arr.splice(idx, 1);
    arr.splice(nextIdx, 0, picked);
    if (editingIdx === idx) editingIdx = nextIdx;
    onUpdateFilters([...hiddenSet], arr);
  }

  function toggleCustom(idx: number, enabled: boolean) {
    onUpdateFilters([...hiddenSet], customTabs.map((t, i) => (i === idx ? { ...t, enabled } : t)));
  }

  function removeCustom(idx: number) {
    if (editingIdx === idx) resetForm();
    onUpdateFilters([...hiddenSet], customTabs.filter((_, i) => i !== idx));
  }

  function ruleSummary(t: CustomFilterTab): string {
    if (t.everything_expr?.trim()) return t.everything_expr.trim();
    return `ext:${(t.extensions || []).join(';')}`;
  }
</script>

<div class="t-card" style="margin-top:14px">
  <div class="t-row col-row">
    <div class="hdr-split">
      <div>
        <div class="t-label">{isEn ? 'Launcher Filter Bar Preview' : '搜索框分类栏实时预览（Ctrl+Tab / Ctrl+1..9）'}</div>
        <div class="t-desc">
          {isEn
            ? 'Click chips to hide built-ins. Preview shows exact tab order; overflow tabs can be exposed by hiding unused built-ins.'
            : '点击下方按钮显隐内置标签，支持 ↑↓ 排序与编辑；右侧实时预览搜索栏上的最终排列效果。'}
        </div>
      </div>
      <span class="fit-badge {estBarPx > barLimitPx ? 'is-warn' : ''}">
        {previewTabs.length} {isEn ? 'tabs' : '个展示位'} · {estBarPx > barLimitPx ? (isEn ? 'May clip narrow width' : '可能超出默认宽度') : (isEn ? 'Fits neatly' : '排布宽松')}
      </span>
    </div>

    <div class="preview-bar" aria-label="filter-bar-preview">
      {#each previewTabs as p, i}
        <button
          type="button"
          class="pv-chip {previewId === p.id ? 'is-active' : ''} {p.custom ? 'is-custom' : ''}"
          onclick={() => (previewId = p.id)}
          title={i < 9 ? `Ctrl+${i + 1}` : ''}
        >
          {#if i < 9}<span class="pv-kbd">{i + 1}</span>{/if}
          <span>{p.label}</span>
        </button>
      {/each}
    </div>

    <div class="chip-grid">
      <span class="t-chip is-locked">✓ {isEn ? 'All (Pinned)' : '全部（常驻）'}</span>
      {#each builtinTabs as b}
        {@const visible = !hiddenSet.has(b.id)}
        <button type="button" class="t-chip {visible ? 'is-on' : ''}" onclick={() => toggleBuiltin(b.id)}>
          {visible ? '✓ ' : '+ '}{isEn ? b.en : b.zh}
        </button>
      {/each}
    </div>
  </div>

  <div class="t-row col-row">
    <div class="hdr-split">
      <div>
        <div class="t-label">{isEn ? 'Custom Extension & Filter Tabs' : '用户自定义扩展标签'}</div>
        <div class="t-desc">
          {isEn
            ? 'One-click presets or define name, @tag alias, and file extensions (e.g. rs;ts) or rules (dm:today).'
            : '点选预设快速启用，或在下方自定义名称、@快捷词与后缀规则；搜索框内直接输入 @词 即可瞬时过滤。'}
        </div>
      </div>
      <span class="count-pill">{customTabs.length} / 12</span>
    </div>

    <div class="chip-grid">
      {#each customPresets as p}
        {@const added = hasPreset(p.id)}
        <button
          type="button"
          class="t-chip {added ? 'is-on' : ''}"
          disabled={added}
          data-preset-tab={p.id}
          title={p.extensions.join(';')}
          onclick={() => addPreset(p)}
        >
          {added ? '✓ ' : '+ '}{isEn ? p.en : p.zh}
          <span class="chip-sub">{p.extensions.length}</span>
        </button>
      {/each}
    </div>

    <div class="editor-box {editingIdx !== null ? 'is-editing' : ''}">
      <div class="add-row">
        <input
          class="t-input add-name"
          placeholder={isEn ? 'Tab name (Design)' : '显示名称（设计稿）'}
          bind:value={newLabel}
          onkeydown={(e) => e.key === 'Enter' && saveCustomTab()}
        />
        <input
          class="t-input add-tag"
          placeholder={autoTagPreview()}
          bind:value={newTag}
          onkeydown={(e) => e.key === 'Enter' && saveCustomTab()}
        />
        <input
          class="t-input add-rule"
          placeholder={isEn ? 'Extensions (psd;fig) or rule (dm:today)' : '扩展名（psd;fig）或语法（dm:today / folder:）'}
          bind:value={newRule}
          onkeydown={(e) => { if (e.key === 'Enter') saveCustomTab(); else if (e.key === 'Escape' && editingIdx !== null) resetForm(); }}
        />
        <Button variant={editingIdx !== null ? 'primary' : 'secondary'} size="sm" onclick={saveCustomTab}>
          <Plus size={13} />
          {editingIdx !== null ? (isEn ? 'Save' : '保存修改') : (isEn ? 'Add Tab' : '添加标签')}
        </Button>
        {#if editingIdx !== null}
          <Button variant="ghost" size="sm" onclick={resetForm} title={isEn ? 'Cancel' : '取消编辑'}>
            <X size={13} />
          </Button>
        {/if}
      </div>

      <div class="snip-row">
        <span class="snip-hint">{isEn ? 'Templates:' : '快速填入：'}</span>
        {#each quickSnippets as s}
          <button type="button" class="snip-btn" onclick={() => applySnippet(s)}>{s.label} ({s.rule})</button>
        {/each}
      </div>
      {#if formErr}<div class="err-msg">{formErr}</div>{/if}
    </div>

    {#if customTabs.length > 0}
      <div class="custom-list">
        {#each customTabs as tab, idx}
          <div class="custom-item {editingIdx === idx ? 'is-active-edit' : ''} {!tab.enabled ? 'is-dim' : ''}">
            <div class="ord-col">
              <button type="button" class="ord-btn" disabled={idx === 0} title="上移" onclick={() => moveCustom(idx, -1)}>
                <ChevronUp size={12} />
              </button>
              <button type="button" class="ord-btn" disabled={idx === customTabs.length - 1} title="下移" onclick={() => moveCustom(idx, 1)}>
                <ChevronDown size={12} />
              </button>
            </div>
            <Tag size={13} class="tag-ico" />
            <span class="custom-name">{tab.label}</span>
            <span class="custom-tag">{tab.tag}</span>
            <span class="custom-rule" title={ruleSummary(tab)}>{ruleSummary(tab)}</span>
            <Switch checked={tab.enabled} onchange={(v) => toggleCustom(idx, v)} />
            <Button variant="ghost" size="sm" title={isEn ? 'Edit' : '编辑规则'} onclick={() => startEdit(idx)}>
              <SlidersHorizontal size={12} />
            </Button>
            <Button variant="danger" size="sm" title={isEn ? 'Delete' : '删除'} onclick={() => removeCustom(idx)}>
              <Trash2 size={12} />
            </Button>
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty-tip">{isEn ? 'No custom tabs yet — pick a preset above or craft your own.' : '尚未添加自定义标签：可直接点选上方预设，或用快捷模板快速创建。'}</div>
    {/if}
  </div>
</div>

<style>
  .hdr-split { display: flex; align-items: flex-start; justify-content: space-between; gap: 10px; }
  .fit-badge, .count-pill {
    font-size: 11px; padding: 2px 8px; border-radius: 999px; white-space: nowrap; flex-shrink: 0;
    background: var(--sidebar); border: 1px solid var(--border); color: var(--muted);
  }
  .fit-badge.is-warn { border-color: var(--warn); color: var(--warn); }
  .preview-bar {
    display: flex; align-items: center; gap: 6px; padding: 8px 10px; border-radius: 10px;
    background: var(--canvas); border: 1px dashed var(--border-strong); overflow-x: auto;
  }
  .pv-chip {
    display: inline-flex; align-items: center; gap: 5px; height: 24px; padding: 0 9px; border-radius: 6px;
    border: 1px solid var(--border); background: var(--surface); color: var(--text-secondary);
    font-size: 11.5px; white-space: nowrap; cursor: pointer; font-family: inherit; transition: all 0.14s ease;
  }
  .pv-chip.is-custom { border-style: dashed; }
  .pv-chip.is-active { background: var(--accent-soft); border-color: var(--accent); border-style: solid; color: var(--accent); font-weight: 600; }
  .pv-kbd { font-size: 10px; opacity: 0.65; font-family: "Cascadia Mono", Consolas, monospace; }
  .t-chip { display: inline-flex; align-items: center; gap: 5px; }
  .t-chip:hover:not(:disabled) { border-color: var(--border-strong); color: var(--text); }
  .t-chip:disabled { opacity: 0.72; cursor: default; }
  .t-chip.is-locked { background: var(--sidebar); color: var(--muted); cursor: default; }
  .chip-sub { font-size: 10.5px; opacity: 0.65; }
  .editor-box {
    display: flex; flex-direction: column; gap: 8px; padding: 10px; border-radius: 9px;
    background: var(--canvas); border: 1px solid var(--border);
  }
  .editor-box.is-editing { border-color: var(--accent); background: var(--accent-soft); }
  .add-row { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .add-name { width: 128px; flex-shrink: 0; }
  .add-tag { width: 104px; flex-shrink: 0; font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; }
  .add-rule { flex: 1; min-width: 170px; font-family: "Cascadia Mono", Consolas, monospace; font-size: 12px; }
  .snip-row { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .snip-hint { font-size: 11px; color: var(--muted); }
  .snip-btn {
    border: 1px solid var(--border); background: var(--surface); color: var(--text-secondary);
    border-radius: 5px; padding: 2px 7px; font-size: 11px; cursor: pointer; font-family: inherit;
  }
  .snip-btn:hover { border-color: var(--accent); color: var(--accent); }
  .err-msg { font-size: 12px; color: var(--danger); }
  .custom-list { display: flex; flex-direction: column; gap: 6px; }
  .custom-item {
    display: flex; align-items: center; gap: 7px; min-height: 38px; padding: 4px 8px;
    border-radius: 8px; background: var(--canvas); border: 1px solid var(--border); transition: opacity 0.14s ease;
  }
  .custom-item.is-active-edit { border-color: var(--accent); }
  .custom-item.is-dim { opacity: 0.58; }
  .ord-col { display: flex; flex-direction: column; gap: 1px; }
  .ord-btn {
    display: flex; align-items: center; justify-content: center; width: 18px; height: 14px; padding: 0;
    border: 1px solid transparent; border-radius: 3px; background: transparent; color: var(--muted); cursor: pointer;
  }
  .ord-btn:hover:not(:disabled) { border-color: var(--border); color: var(--text); background: var(--surface); }
  .ord-btn:disabled { opacity: 0.28; cursor: default; }
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
  .empty-tip {
    font-size: 12px; color: var(--muted); padding: 10px 12px; border-radius: 8px;
    border: 1px dashed var(--border); text-align: center;
  }
</style>
