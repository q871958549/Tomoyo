<script lang="ts">
  import type { Lang } from '../../lib/i18n';
  import type { CustomFilterTab, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    onUpdateFilters: (hidden: string[], custom: CustomFilterTab[], layout?: string) => void;
  }

  let { cfg, lang, onUpdateFilters }: Props = $props();
  const t = (zh: string, en: string) => (lang === 'en' ? en : zh);

  const MAX_CUSTOM = 32;
  const BUILTINS = ['app:应用:Apps', 'folder:文件夹:Folders', 'pdf:PDF:PDF', 'excel:Excel:Excel', 'word:Word:Word', 'ppt:PPT:PPT', 'image:图片:Images', 'archive:压缩:Zip'].map((s) => {
    const [id, zh, en] = s.split(':'); return { id, zh, en };
  });
  const PRESETS = [
    ['code', '代码', 'Code', '@code', 'rs;ts;tsx;js;py;go;c;cpp;h;toml', ''],
    ['video', '视频', 'Video', '@video', 'mp4;mkv;mov;avi;webm;flv', ''],
    ['audio', '音频', 'Audio', '@audio', 'mp3;flac;wav;aac;m4a;ogg', ''],
    ['markdown', 'Markdown', 'Markdown', '@md', 'md;markdown;txt', ''],
    ['installer', '安装包', 'Installer', '@setup', 'exe;msi;msix;dmg;AppImage;deb', ''],
    ['design', '设计稿', 'Design', '@design', 'psd;fig;sketch;ai;xd;blend', ''],
    ['ebook', '电子书', 'eBook', '@book', 'epub;mobi;azw3;djvu', ''],
    ['config', '配置', 'Config', '@cfg', 'json;yaml;yml;toml;ini;env', ''],
    ['today', '今日', 'Today', '@today', '', 'dm:today'],
  ] as const;

  let customTabs = $derived(cfg.custom_filters || []);
  let hiddenSet = $derived(new Set((cfg.hidden_builtin_filters || []).map((s) => s.toLowerCase())));
  let layoutMode = $derived(cfg.filter_bar_layout === 'single' ? 'single' : 'wrap');

  let editorOpen = $state(false);
  let editIndex = $state<number | null>(null);
  let draftLabel = $state('');
  let draftTag = $state('');
  let draftRule = $state('');
  let filterKw = $state('');
  let filterErr = $state('');

  let shortcutMap = $derived.by(() => {
    const m = new Map<string, number>();
    let slot = 2;
    for (const b of BUILTINS) if (!hiddenSet.has(b.id) && slot <= 9) m.set(`b:${b.id}`, slot++);
    customTabs.forEach((c, i) => { if (c.enabled && slot <= 9) m.set(`c:${i}`, slot++); });
    return m;
  });
  let totalVisible = $derived(BUILTINS.filter((b) => !hiddenSet.has(b.id)).length + customTabs.filter((c) => c.enabled).length + 1);
  let availPresets = $derived(PRESETS.filter((p) => !customTabs.some((c) => c.id === p[0] || c.tag.toLowerCase() === p[3])));
  let visibleCustom = $derived.by(() => {
    const q = filterKw.trim().toLowerCase();
    return customTabs.map((tab, index) => ({ tab, index })).filter(({ tab }) => !q || tab.label.toLowerCase().includes(q) || tab.tag.toLowerCase().includes(q));
  });

  function toggleBuiltin(id: string) {
    const next = new Set(hiddenSet);
    if (next.has(id)) next.delete(id); else next.add(id);
    onUpdateFilters([...next], customTabs);
  }
  function addPreset(p: (typeof PRESETS)[number]) {
    if (customTabs.length >= MAX_CUSTOM) { filterErr = t(`最多 ${MAX_CUSTOM} 个`, `Max ${MAX_CUSTOM}`); return; }
    filterErr = '';
    onUpdateFilters([...hiddenSet], [...customTabs, { id: p[0], label: lang === 'en' ? p[2] : p[1], tag: p[3], extensions: p[4] ? p[4].split(';') : [], everything_expr: p[5], enabled: true }]);
  }
  function openEdit(i: number) {
    const x = customTabs[i]; if (!x) return;
    editIndex = i; draftLabel = x.label; draftTag = x.tag;
    draftRule = x.extensions.length ? x.extensions.join(';') : x.everything_expr;
    filterErr = ''; editorOpen = true;
  }
  function removeCustom(i: number) {
    if (editIndex === i) editorOpen = false;
    else if (editIndex !== null && editIndex > i) editIndex -= 1;
    onUpdateFilters([...hiddenSet], customTabs.filter((_, k) => k !== i));
  }
  function moveCustom(i: number, d: -1 | 1) {
    const j = i + d; if (j < 0 || j >= customTabs.length) return;
    const arr = [...customTabs]; [arr[i], arr[j]] = [arr[j], arr[i]];
    if (editIndex === i) editIndex = j;
    onUpdateFilters([...hiddenSet], arr);
  }
  function saveEditor() {
    const label = draftLabel.trim(), rawTag = draftTag.trim().replace(/^@+/, ''), rule = draftRule.trim();
    if (!label || !rawTag || !rule) { filterErr = t('请填写完整项', 'Fill all fields'); return; }
    const tag = `@${rawTag.toLowerCase()}`;
    const isExpr = /[: >]/.test(rule);
    const extensions = isExpr ? [] : rule.split(/[;,\s]+/).map((s) => s.trim().replace(/^\.+/, '').toLowerCase()).filter(Boolean).slice(0, 24);
    const expr = isExpr ? rule : '';
    if (!extensions.length && !expr) { filterErr = t('规则无效', 'Invalid rule'); return; }
    if (editIndex !== null && customTabs[editIndex]) {
      onUpdateFilters([...hiddenSet], customTabs.map((x, k) => (k === editIndex ? { ...x, label, tag, extensions, everything_expr: expr } : x)));
      editorOpen = false; filterErr = ''; return;
    }
    if (customTabs.length >= MAX_CUSTOM) { filterErr = t(`上限 ${MAX_CUSTOM}`, `Max ${MAX_CUSTOM}`); return; }
    if (customTabs.some((c) => c.tag.toLowerCase() === tag)) { filterErr = `${tag} ${t('已存在', 'exists')}`; return; }
    onUpdateFilters([...hiddenSet], [...customTabs, { id: `c_${Date.now().toString(36)}`, label, tag, extensions, everything_expr: expr, enabled: true }]);
    editorOpen = false; filterErr = '';
  }
</script>

<div class="t-card">
  <div class="t-card-h fc-h">
    <div class="t-row-info">
      <div class="t-label">{t('搜索分类栏与扩展标签 (Ctrl+Tab / Ctrl+1..9)', 'Filter Tabs & Custom Tags (Ctrl+Tab / 1..9)')}</div>
      <div class="t-desc">{t('点击内置标签直接显隐；点击自定义标签就地编辑，支持 32 个（@词 直达）', 'Click built-ins to toggle; click custom chips to edit (up to 32 tabs)')}</div>
    </div>
    <div class="fc-r">
      <Segmented value={layoutMode} options={[{ value: 'wrap', label: t('多行换行', 'Wrap') }, { value: 'single', label: t('单行 +N', '1-Row') }]} onchange={(v) => onUpdateFilters([...hiddenSet], customTabs, v)} />
      <span class="t-tag">{totalVisible} {t('启用', 'on')} · {customTabs.length}/{MAX_CUSTOM}</span>
    </div>
  </div>

  {#if customTabs.length >= 6}
    <input class="t-input fc-q" placeholder={t('筛选已有自定义标签…', 'Filter custom tags…')} bind:value={filterKw} />
  {/if}

  <div class="fc-cloud">
    <span class="fc-c is-on"><i class="fc-n">1</i>{t('全部', 'All')}</span>
    {#each BUILTINS as b}
      {@const on = !hiddenSet.has(b.id)}
      {@const num = shortcutMap.get(`b:${b.id}`)}
      <button type="button" class="fc-c {on ? 'is-on' : 'is-off'}" onclick={() => toggleBuiltin(b.id)}>
        {#if num}<i class="fc-n">{num}</i>{/if}{lang === 'en' ? b.en : b.zh}
      </button>
    {/each}
    {#each visibleCustom as { tab, index }}
      {@const num = shortcutMap.get(`c:${index}`)}
      <div class="fc-c fc-x {tab.enabled ? 'is-on' : 'is-off'} {editorOpen && editIndex === index ? 'is-ed' : ''}">
        <button type="button" onclick={() => onUpdateFilters([...hiddenSet], customTabs.map((x, k) => (k === index ? { ...x, enabled: !x.enabled } : x)))}>
          {#if num}<i class="fc-n">{num}</i>{:else}{tab.enabled ? '✓' : '○'}{/if}
        </button>
        <button type="button" onclick={() => openEdit(index)}>{tab.label} <small>{tab.tag}</small></button>
        <button type="button" class="fc-del" onclick={() => removeCustom(index)}>×</button>
      </div>
    {/each}
  </div>

  <div class="fc-bar">
    <Button size="sm" variant={editorOpen && editIndex === null ? 'primary' : 'secondary'} onclick={() => { if (editorOpen && editIndex === null) editorOpen = false; else { editIndex = null; draftLabel = draftTag = draftRule = filterErr = ''; editorOpen = true; } }}>{t('+ 新建标签', '+ New Tag')}</Button>
    {#if availPresets.length}
      <span class="t-desc">{t('预设:', 'Presets:')}</span>
      {#each availPresets as p}
        <button type="button" data-preset-tab={p[0]} class="t-chip" onclick={() => addPreset(p)}>+ {lang === 'en' ? p[2] : p[1]}</button>
      {/each}
    {/if}
  </div>

  {#if editorOpen}
    <div class="t-subpanel fc-ed">
      <div class="fc-g">
        <input class="t-input" placeholder={t('名称 (设计稿)', 'Name')} bind:value={draftLabel} maxlength={16} />
        <input class="t-input" placeholder="@tag" bind:value={draftTag} maxlength={16} />
        <input class="t-input" placeholder="psd;fig / dm:today / folder:" bind:value={draftRule} maxlength={120} />
      </div>
      <div class="fc-r">
        <div class="fc-r">
          {#each [['fig;psd;sketch', '后缀;', 'Ext'], ['folder:', '仅目录', 'Folders'], ['size:>50mb', '50MB+', '50MB+'], ['dm:today', '今日', 'Today']] as tpl}
            <button type="button" class="t-chip" onclick={() => (draftRule = tpl[0])}>{lang === 'en' ? tpl[2] : tpl[1]}</button>
          {/each}
        </div>
        <div class="fc-r">
          {#if editIndex !== null}
            <Button size="sm" disabled={editIndex === 0} onclick={() => editIndex !== null && moveCustom(editIndex, -1)}>←</Button>
            <Button size="sm" disabled={editIndex >= customTabs.length - 1} onclick={() => editIndex !== null && moveCustom(editIndex, 1)}>→</Button>
            <Button size="sm" variant="danger" onclick={() => editIndex !== null && removeCustom(editIndex)}>{t('删除', 'Delete')}</Button>
          {/if}
          <Button size="sm" onclick={() => (editorOpen = false)}>{t('取消', 'Cancel')}</Button>
          <Button size="sm" variant="primary" onclick={saveEditor}>{editIndex !== null ? t('保存', 'Save') : t('添加', 'Add')}</Button>
        </div>
      </div>
      {#if filterErr}<div class="t-err">{filterErr}</div>{/if}
    </div>
  {/if}
</div>

<style>
  .fc-h, .fc-r, .fc-bar { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .fc-h { justify-content: space-between; align-items: flex-start; }
  .fc-q { width: 210px; margin-bottom: 8px; }
  .fc-cloud { display: flex; flex-wrap: wrap; gap: 6px; padding: 8px; border-radius: 9px; background: var(--canvas); border: 1px solid var(--border); max-height: 170px; overflow-y: auto; }
  .fc-c { display: inline-flex; align-items: center; gap: 4px; height: 25px; padding: 0 8px; border-radius: 6px; font-size: 12px; border: 1px solid var(--border); background: var(--surface); color: var(--fg); }
  .fc-c.is-on { border-color: var(--accent); background: var(--accent-soft); color: var(--accent); font-weight: 500; }
  .fc-c.is-off { border-style: dashed; opacity: .55; color: var(--muted); }
  .fc-x { padding: 0 4px 0 6px; }
  .fc-x.is-ed { box-shadow: 0 0 0 2px var(--accent-soft); }
  .fc-x small { font-family: "Cascadia Mono", Consolas, monospace; font-size: 10.5px; opacity: .78; }
  .fc-del:hover { color: var(--danger); }
  .fc-n { font-style: normal; font-size: 10px; opacity: .75; }
  .fc-bar { margin-top: 9px; }
  .fc-ed { margin-top: 9px; display: flex; flex-direction: column; gap: 7px; }
  .fc-g { display: grid; grid-template-columns: 1fr 88px 1.6fr; gap: 7px; }
  .fc-ed > .fc-r { justify-content: space-between; }
</style>
