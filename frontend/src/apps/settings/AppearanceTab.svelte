<script lang="ts">
  import { tr, type Lang } from '../../lib/i18n';
  import type { TomoyoConfig, TypographyConfig } from '../../lib/types';
  import Button from '../../lib/ui/Button.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Slider from '../../lib/ui/Slider.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    systemFonts?: string[];
    onThemePick: (name: string) => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg = $bindable(), lang, systemFonts = [], onThemePick, onSave }: Props = $props();
  const t = (zh: string, en: string) => (lang === 'en' ? en : zh);

  const themes = [
    { id: 'light', nameKey: 'thm_light', descKey: 'thm_light_d', bg: '#F4F5F8', card: '#FFF', txt: '#1E293B', bar: '#4F6AF5' },
    { id: 'dark', nameKey: 'thm_dark', descKey: 'thm_dark_d', bg: '#16161B', card: '#22222A', txt: '#E2E8F0', bar: '#3B82F6' },
    { id: 'midnight', nameKey: 'thm_mid', descKey: 'thm_mid_d', bg: '#0E111B', card: '#171C2D', txt: '#DCE2F2', bar: '#6366F1' },
  ];

  const DEF: TypographyConfig = { font_family: '', title_weight: 500, subtitle_weight: 400, title_size: 16 };
  let typo = $derived<TypographyConfig>({ ...DEF, ...(cfg.typography || {}) });
  let previewFont = $derived(
    typo.font_family
      ? `"${typo.font_family.replace(/"/g, '')}","Microsoft YaHei UI",sans-serif`
      : '"Alibaba PuHuiTi 3.0","Microsoft YaHei UI",sans-serif'
  );

  function setTypo(patch: Partial<TypographyConfig>) {
    cfg.typography = { ...typo, ...patch };
    onSave(0);
  }

  const wOpts = $derived([
    { value: '300', label: t('细 300', 'Light') },
    { value: '400', label: t('常规 400', 'Regular') },
    { value: '500', label: t('中等 500', 'Medium') },
    { value: '600', label: t('半粗 600', 'Semi') },
    { value: '700', label: t('粗体 700', 'Bold') },
  ]);
  const sOpts = [14, 15, 16, 17, 18].map((n) => ({ value: String(n), label: `${n}px` }));
  const posText = $derived(t(`已记忆位置 (${cfg.window.custom_x}, ${cfg.window.custom_y})`, `Saved (${cfg.window.custom_x}, ${cfg.window.custom_y})`));
</script>

<div class="t-page-h">{tr(lang, 'h_appearance')}</div>
<p class="t-page-lead">{tr(lang, 'lead_appearance')}</p>

<div class="t-card">
  <div class="t-card-h"><div class="t-label">{tr(lang, 'lbl_theme')}</div></div>
  <div class="thm-grid">
    {#each themes as th}
      <button type="button" class="thm-card {cfg.theme === th.id ? 'is-on' : ''}" onclick={() => onThemePick(th.id)}>
        <div class="thm-sw" style:background={th.bg}>
          <div class="sw-b" style:background={th.bar}></div>
          <div class="sw-r" style:background={th.card}><span style:background={th.txt}></span></div>
          <div class="sw-r" style:background={th.card} style:opacity=".65"><span style:background={th.txt}></span></div>
        </div>
        <div class="t-label">{tr(lang, th.nameKey)}</div>
        <div class="t-desc">{tr(lang, th.descKey)}</div>
      </button>
    {/each}
  </div>
</div>

<div class="t-card">
  <div class="t-card-h fc-r">
    <div>
      <div class="t-label">{t('搜索结果字体与现代排版', 'Result Typography')}</div>
      <div class="t-desc">{t('支持内置普惠体或本机已安装字体，独立调节字重与字号', 'Font family, weights & size (live sync)')}</div>
    </div>
    <Button size="sm" onclick={() => setTypo(DEF)}>{t('恢复默认', 'Reset')}</Button>
  </div>

  <div class="tp-box" style:font-family={previewFont}>
    <div class="tp-r is-on">
      <span class="tp-i">T</span>
      <div class="tp-m">
        <div style:font-weight={typo.title_weight} style:font-size="{typo.title_size}px"><mark>Tomoyo</mark> {t('快速启动器 · 排版预览', 'Launcher · Typography')}</div>
        <div class="tp-s" style:font-weight={typo.subtitle_weight} style:font-size="{Math.max(11, typo.title_size - 3)}px">{t('系统应用 · ClearType 清晰渲染', 'App · Crisp ClearType text')}</div>
      </div>
      <span class="tp-k is-on">Enter</span>
    </div>
    <div class="tp-r">
      <span class="tp-i" style:background="#dc2626">PDF</span>
      <div class="tp-m">
        <div style:font-weight={typo.title_weight} style:font-size="{typo.title_size}px">{t('2026 体验优化方案.pdf', 'Architecture Notes.pdf')}</div>
        <div class="tp-s" style:font-weight={typo.subtitle_weight} style:font-size="{Math.max(11, typo.title_size - 3)}px">D:\Docs · 2.4 MB</div>
      </div>
      <span class="tp-k">Alt+2</span>
    </div>
  </div>

  <div class="t-row">
    <div class="t-row-info"><div class="t-label">{t('字体选择', 'Font Family')}</div><div class="t-desc">{t('默认阿里巴巴普惠体 3.0，可选系统字体', 'Bundled PuHuiTi 3.0 or OS font')}</div></div>
    <select class="t-input f-sel" value={typo.font_family} onchange={(e) => setTypo({ font_family: (e.currentTarget as HTMLSelectElement).value })}>
      <option value="">{t('内置阿里巴巴普惠体 3.0（推荐）', 'Alibaba PuHuiTi 3.0 (Default)')}</option>
      {#each systemFonts as f}<option value={f}>{f}</option>{/each}
    </select>
  </div>
  <div class="t-row"><div class="t-row-info"><div class="t-label">{t('标题粗细', 'Title Weight')}</div></div><Segmented options={wOpts} value={String(typo.title_weight)} onchange={(v) => setTypo({ title_weight: Number(v) })} /></div>
  <div class="t-row"><div class="t-row-info"><div class="t-label">{t('副标题粗细', 'Subtitle Weight')}</div></div><Segmented options={wOpts.slice(0, 4)} value={String(typo.subtitle_weight)} onchange={(v) => setTypo({ subtitle_weight: Number(v) })} /></div>
  <div class="t-row"><div class="t-row-info"><div class="t-label">{t('标题字号', 'Title Size')}</div></div><Segmented options={sOpts} value={String(typo.title_size)} onchange={(v) => setTypo({ title_size: Number(v) })} /></div>
</div>

<div class="t-card">
  <div class="t-row">
    <div class="t-row-info"><div class="t-label">{tr(lang, 'lbl_width')}</div><div class="t-desc">{tr(lang, 'desc_width')}</div></div>
    <Slider value={cfg.window.width_dip} min={520} max={1200} step={20} format={(v) => `${v} px`} oninput={(v) => { cfg.window.width_dip = v; onSave(120); }} />
  </div>
  <div class="t-row">
    <div class="t-row-info"><div class="t-label">{tr(lang, 'lbl_rows')}</div><div class="t-desc">{tr(lang, 'desc_rows')}</div></div>
    <Slider value={cfg.window.max_visible_rows} min={4} max={16} step={1} format={(v) => (lang === 'en' ? `${v}` : `${v} 行`)} oninput={(v) => { cfg.window.max_visible_rows = v; onSave(120); }} />
  </div>
  <div class="t-row">
    <div class="t-row-info"><div class="t-label">{tr(lang, 'lbl_preview_w')}</div><div class="t-desc">{tr(lang, 'desc_preview_w')}</div></div>
    <Slider value={cfg.window.preview_width_dip} min={220} max={560} step={10} format={(v) => `${v} px`} oninput={(v) => { cfg.window.preview_width_dip = v; onSave(120); }} />
  </div>
  <div class="t-row">
    <div class="t-row-info"><div class="t-label">{tr(lang, 'lbl_pos')}</div><div class="t-desc">{cfg.window.custom_x !== null ? posText : tr(lang, 'pos_auto')}</div></div>
    <Button size="sm" disabled={cfg.window.custom_x === null} onclick={() => { cfg.window.custom_x = null; cfg.window.custom_y = null; onSave(0); }}>{tr(lang, 'btn_reset_pos')}</Button>
  </div>
</div>

<style>
  .thm-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }
  .thm-card { text-align: left; padding: 10px; border-radius: 10px; border: 1.5px solid var(--border); background: var(--canvas); }
  .thm-card.is-on { border-color: var(--accent); box-shadow: 0 0 0 2px var(--accent-soft); }
  .thm-sw { height: 64px; border-radius: 7px; padding: 7px; display: flex; flex-direction: column; gap: 5px; margin-bottom: 7px; border: 1px solid rgba(128,128,128,.2); }
  .sw-b { height: 6px; width: 42%; border-radius: 3px; }
  .sw-r { height: 14px; border-radius: 4px; display: flex; align-items: center; padding: 0 6px; }
  .sw-r span { height: 4px; width: 65%; border-radius: 2px; opacity: .75; }
  .fc-r { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .f-sel { min-width: 220px; max-width: 270px; }
  .tp-box { border-radius: 9px; padding: 5px; margin-bottom: 8px; display: flex; flex-direction: column; gap: 4px; background: var(--canvas); border: 1px solid var(--border); }
  .tp-r { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: 7px; }
  .tp-r.is-on { background: var(--accent-soft); border: 1px solid var(--accent); }
  .tp-i { width: 25px; height: 25px; border-radius: 6px; background: var(--accent); color: #fff; font-weight: 600; font-size: 9.5px; display: grid; place-items: center; flex-shrink: 0; }
  .tp-m { flex: 1; min-width: 0; line-height: 1.3; white-space: nowrap; overflow: hidden; }
  .tp-m div { overflow: hidden; text-overflow: ellipsis; }
  .tp-m mark { background: none; color: var(--accent); }
  .tp-s { color: var(--muted); }
  .tp-k { font-size: 10px; padding: 1px 6px; border-radius: 5px; border: 1px solid var(--border); color: var(--muted); flex-shrink: 0; }
  .tp-k.is-on { color: var(--accent); border-color: var(--accent); font-weight: 500; }
</style>
