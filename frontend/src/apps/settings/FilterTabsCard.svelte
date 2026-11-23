<script lang="ts">
  import type { Lang } from '../../lib/i18n';
  import type { CustomFilterTab, SelectOption, TomoyoConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

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

  let totalVisible = $derived(BUILTINS.filter((b) => !hiddenSet.has(b.id)).length + customTabs.filter((c) => c.enabled).length + 1);
  let presetOptions = $derived<SelectOption[]>(
    PRESETS
      .filter((p) => !customTabs.some((c) => c.id === p[0] || c.tag.toLowerCase() === p[3]))
      .map((p) => ({ value: p[0], label: lang === 'en' ? p[2] : p[1], hint: p[3] }))
  );
  let visibleCustom = $derived.by(() => {
    const q = filterKw.trim().toLowerCase();
    return customTabs.map((tab, index) => ({ tab, index })).filter(({ tab }) => !q || tab.label.toLowerCase().includes(q) || tab.tag.toLowerCase().includes(q));
  });

  function toggleBuiltin(id: string) {
    const next = new Set(hiddenSet);
    if (next.has(id)) next.delete(id); else next.add(id);
    onUpdateFilters([...next], customTabs);
  }
  function addPresetById(id: string) {
    const p = PRESETS.find((x) => x[0] === id); if (!p) return;
    if (customTabs.length >= MAX_CUSTOM) { filterErr = t(`已达 ${MAX_CUSTOM} 个上限`, `Max ${MAX_CUSTOM}`); return; }
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
    if (!label || !rawTag || !rule) { filterErr = t('请填写名称、@词 与规则', 'Fill label, @tag and rule'); return; }
    const tag = `@${rawTag.toLowerCase()}`;
    const isExpr = /[: >]/.test(rule);
    const extensions = isExpr ? [] : rule.split(/[;,\s]+/).map((s) => s.trim().replace(/^\.+/, '').toLowerCase()).filter(Boolean).slice(0, 24);
    const expr = isExpr ? rule : '';
    if (!extensions.length && !expr) { filterErr = t('规则格式无效', 'Invalid rule'); return; }
    if (editIndex !== null && customTabs[editIndex]) {
      onUpdateFilters([...hiddenSet], customTabs.map((x, k) => (k === editIndex ? { ...x, label, tag, extensions, everything_expr: expr } : x)));
      editorOpen = false; filterErr = ''; return;
    }
    if (customTabs.length >= MAX_CUSTOM) { filterErr = t(`已达 ${MAX_CUSTOM} 个上限`, `Max ${MAX_CUSTOM}`); return; }
    if (customTabs.some((c) => c.tag.toLowerCase() === tag)) { filterErr = `${tag} ${t('已存在', 'exists')}`; return; }
    onUpdateFilters([...hiddenSet], [...customTabs, { id: `c_${Date.now().toString(36)}`, label, tag, extensions, everything_expr: expr, enabled: true }]);
    editorOpen = false; filterErr = '';
  }
  const ruleText = (tab: CustomFilterTab) => (tab.extensions.length ? tab.extensions.join(' · ') : tab.everything_expr);
</script>

<div class="t-card" style="margin-top:14px">
  <div class="t-row">
    <div>
      <div class="t-label">{t('搜索分类栏与排版', 'Filter Bar & Layout')} <span class="t-pill" style="margin-left:6px">{totalVisible} {t('启用', 'on')}</span></div>
      <div class="t-desc">{t('支持多行换行或单行 +N 折叠；Ctrl+Tab / Ctrl+1..9 切换', 'Wrap up to 3 rows or +N collapse; Ctrl+Tab / 1..9')}</div>
    </div>
    <Segmented value={layoutMode} options={[{ value: 'wrap', label: t('多行换行', 'Wrap') }, { value: 'single', label: t('单行 +N', '1-Row') }]} onchange={(v) => onUpdateFilters([...hiddenSet], customTabs, v)} />
  </div>

  <div class="t-row col-row">
    <div class="t-label">{t('内置基础分类', 'Built-in Categories')} <span class="t-desc" style="display:inline;margin-left:6px">{t('点击切换显隐 · 「全部」常驻', 'Click to toggle · All pinned')}</span></div>
    <div class="chip-grid">
      <span class="fc-lock">{t('✓ 全部（常驻）', '✓ All')}</span>
      {#each BUILTINS as b}
        {@const on = !hiddenSet.has(b.id)}
        <button type="button" class="t-chip {on ? 'fc-on' : 'fc-off'}" onclick={() => toggleBuiltin(b.id)}>
          {on ? '✓ ' : ''}{lang === 'en' ? b.en : b.zh}
        </button>
      {/each}
    </div>
  </div>

  <div class="t-row col-row">
    <div class="fc-head">
      <div>
        <div class="t-label">{t('自定义扩展标签', 'Custom Filter Tabs')} <span class="t-pill" style="margin-left:6px">{customTabs.length} / {MAX_CUSTOM}</span></div>
        <div class="t-desc">{t('搜索框打 @词 即刻过滤，支持后缀列表或 Everything 规则', 'Type @tag to filter by extensions or Everything rules')}</div>
      </div>
      <div class="row-actions">
        {#if presetOptions.length}
          <Select class="fc-preset-sel" value="" size="sm" align="right" minWidth="136px" placeholder={t('+ 添加预设…', '+ Preset…')} options={presetOptions} onchange={addPresetById} />
        {/if}
        <Button size="sm" variant={editorOpen && editIndex === null ? 'primary' : 'secondary'} onclick={() => { if (editorOpen && editIndex === null) editorOpen = false; else { editIndex = null; draftLabel = draftTag = draftRule = filterErr = ''; editorOpen = true; } }}>
          {t('+ 新建标签', '+ New')}
        </Button>
      </div>
    </div>

    {#if customTabs.length >= 6}
      <Input size="sm" type="search" placeholder={t('筛选标签或 @词…', 'Filter tags…')} bind:value={filterKw} />
    {/if}

    {#if customTabs.length === 0}
      <div class="fc-empty">{t('暂无自定义标签，可从右上角预设快加或新建。', 'No custom tabs yet.')}</div>
    {:else}
      <div class="fc-list">
        {#each visibleCustom as { tab, index }}
          <div class="fc-row {tab.enabled ? '' : 'is-off'}">
            <Switch checked={tab.enabled} ariaLabel={tab.label} onchange={(v) => onUpdateFilters([...hiddenSet], customTabs.map((x, k) => (k === index ? { ...x, enabled: v } : x)))} />
            <span class="fc-lbl">{tab.label}</span>
            <span class="t-kbd">{tab.tag}</span>
            <span class="fc-rule mono" title={ruleText(tab)}>{ruleText(tab)}</span>
            <div class="fc-acts">
              <Button size="sm" variant="ghost" disabled={index === 0} title={t('前移', 'Up')} onclick={() => moveCustom(index, -1)}>↑</Button>
              <Button size="sm" variant="ghost" disabled={index >= customTabs.length - 1} title={t('后移', 'Down')} onclick={() => moveCustom(index, 1)}>↓</Button>
              <Button size="sm" variant="ghost" onclick={() => openEdit(index)}>{t('编辑', 'Edit')}</Button>
              <Button size="sm" variant="danger" onclick={() => removeCustom(index)}>{t('移除', 'Del')}</Button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    {#if editorOpen}
      <div class="fc-ed">
        <div class="fc-grid">
          <Input size="sm" maxlength={16} placeholder={t('名称（如 设计稿）', 'Label (Design)')} bind:value={draftLabel} />
          <Input size="sm" mono maxlength={16} placeholder="@tag" bind:value={draftTag} />
          <Input size="sm" mono maxlength={120} placeholder="psd;fig / dm:today / folder:" bind:value={draftRule} />
        </div>
        <div class="fc-ed-foot">
          <div class="chip-grid">
            {#each [['fig;psd;sketch', '后缀;', 'Ext'], ['folder:', '仅目录', 'Folders'], ['size:>50mb', '50MB+', '50MB+'], ['dm:today', '今日', 'Today']] as tpl}
              <button type="button" class="t-chip" onclick={() => (draftRule = tpl[0])}>{lang === 'en' ? tpl[2] : tpl[1]}</button>
            {/each}
          </div>
          <div class="row-actions">
            <Button size="sm" variant="ghost" onclick={() => (editorOpen = false)}>{t('取消', 'Cancel')}</Button>
            <Button size="sm" variant="primary" onclick={saveEditor}>{editIndex !== null ? t('保存修改', 'Save') : t('确认添加', 'Add')}</Button>
          </div>
        </div>
        {#if filterErr}<div class="fc-err">{filterErr}</div>{/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .fc-lock, .fc-on { background: var(--subtle); border: 1px solid var(--border); color: var(--text); font-weight: 500; }
  .fc-lock { display: inline-flex; align-items: center; height: 28px; padding: 0 10px; border-radius: 7px; font-size: 12px; color: var(--text-secondary); }
  .fc-on { border-color: var(--border-strong); }
  .fc-off { border-style: dashed; opacity: .55; }
  .fc-head, .fc-ed-foot { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
  .fc-empty { padding: 10px 12px; border-radius: 8px; border: 1px dashed var(--border-strong); color: var(--muted); font-size: 12px; }
  .fc-list { display: flex; flex-direction: column; gap: 6px; max-height: 230px; overflow-y: auto; }
  .fc-row { display: flex; align-items: center; gap: 9px; height: 38px; padding: 0 8px 0 10px; border-radius: 8px; background: var(--canvas); border: 1px solid var(--border); }
  .fc-row.is-off { opacity: .6; }
  .fc-lbl { font-size: 13px; font-weight: 500; white-space: nowrap; }
  .fc-rule { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11.5px; color: var(--muted); }
  .fc-acts { display: flex; align-items: center; gap: 2px; flex-shrink: 0; }
  .fc-ed { padding: 12px; border-radius: 9px; background: var(--canvas); border: 1px solid var(--border-strong); display: flex; flex-direction: column; gap: 8px; }
  .fc-grid { display: grid; grid-template-columns: 1fr 100px 1.6fr; gap: 8px; }
  .fc-err { font-size: 12px; color: var(--danger); }
</style>
