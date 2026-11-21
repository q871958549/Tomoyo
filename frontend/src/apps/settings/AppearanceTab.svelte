<script lang="ts">
  import { RotateCcw } from '@lucide/svelte';
  import type { TomoyoConfig } from '../../lib/types';
  import { tr, type Lang } from '../../lib/i18n';
  import Button from '../../lib/ui/Button.svelte';
  import Slider from '../../lib/ui/Slider.svelte';

  interface Props {
    cfg: TomoyoConfig;
    lang: Lang;
    onThemePick: (name: string) => void;
    onSave: (delayMs?: number) => void;
  }

  let { cfg, lang, onThemePick, onSave }: Props = $props();

  const themes = [
    { id: 'light', bg: '#f7f8fa', dot: '#2563eb', bar: '#111827', titleKey: 'thm_light', descKey: 'thm_light_d' },
    { id: 'dark', bg: '#15161c', dot: '#6c7bf7', bar: '#f3f4f6', titleKey: 'thm_dark', descKey: 'thm_dark_d' },
    { id: 'midnight', bg: '#0f1320', dot: '#6366f1', bar: '#cbd5e1', titleKey: 'thm_mid', descKey: 'thm_mid_d' },
  ];

  let posText = $derived.by(() => {
    const { custom_x, custom_y } = cfg.window;
    if (custom_x != null && custom_y != null) {
      return lang === 'en' ? `Remembered (${custom_x}, ${custom_y})` : `已记忆坐标 (${custom_x}, ${custom_y})`;
    }
    return tr(lang, 'pos_auto');
  });

  function resetPos() {
    cfg.window.custom_x = null;
    cfg.window.custom_y = null;
    onSave(0);
  }
</script>

<div class="tab-head">
  <h2>{tr(lang, 'h_appearance')}</h2>
  <p class="tab-lead">{tr(lang, 'lead_appearance')}</p>
</div>

<div class="t-card theme-card-wrap">
  <div class="t-label" style="margin-bottom:10px">{tr(lang, 'lbl_theme')}</div>
  <div class="theme-grid">
    {#each themes as th}
      <button
        type="button"
        class="theme-tile {cfg.theme === th.id ? 'is-selected' : ''}"
        onclick={() => onThemePick(th.id)}
      >
        <div class="swatch" style:background={th.bg}>
          <span class="sw-dot" style:background={th.dot}></span>
          <span class="sw-bar" style:background={th.bar}></span>
        </div>
        <div class="sw-title">{tr(lang, th.titleKey)}</div>
        <div class="sw-desc">{tr(lang, th.descKey)}</div>
      </button>
    {/each}
  </div>
</div>

<div class="t-card" style="margin-top:14px">
  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_width')}</div>
      <div class="t-desc">{tr(lang, 'desc_width')}</div>
    </div>
    <Slider
      value={cfg.window.width_dip}
      min={520}
      max={1280}
      step={20}
      format={(v) => `${v} px`}
      oninput={(v) => { cfg.window.width_dip = v; onSave(180); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_rows')}</div>
      <div class="t-desc">{tr(lang, 'desc_rows')}</div>
    </div>
    <Slider
      value={cfg.window.max_visible_rows}
      min={4}
      max={16}
      step={1}
      format={(v) => (lang === 'en' ? `${v} rows` : `${v} 行`)}
      oninput={(v) => { cfg.window.max_visible_rows = v; onSave(180); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_preview_w')}</div>
      <div class="t-desc">{tr(lang, 'desc_preview_w')}</div>
    </div>
    <Slider
      value={cfg.window.preview_width_dip}
      min={220}
      max={560}
      step={10}
      format={(v) => `${v} px`}
      oninput={(v) => { cfg.window.preview_width_dip = v; onSave(180); }}
    />
  </div>

  <div class="t-row">
    <div>
      <div class="t-label">{tr(lang, 'lbl_pos')}</div>
      <div class="t-desc">{posText}</div>
    </div>
    <Button variant="secondary" onclick={resetPos}>
      <RotateCcw size={13} />
      {tr(lang, 'btn_reset_pos')}
    </Button>
  </div>
</div>

<style>
  .tab-head { margin-bottom: 18px; }
  .tab-head h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
  .tab-lead { margin: 0; font-size: 12.5px; color: var(--muted); }
  .theme-card-wrap { padding: 14px 16px; }
  .theme-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }
  .theme-tile {
    text-align: left; background: var(--canvas); border: 1.5px solid var(--border);
    border-radius: 10px; padding: 10px; cursor: pointer; font-family: inherit;
    transition: border-color 0.14s ease, box-shadow 0.14s ease, transform 0.1s ease;
  }
  .theme-tile:hover { border-color: var(--border-strong); transform: translateY(-1px); }
  .theme-tile.is-selected {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--ring);
    background: var(--surface);
  }
  .swatch {
    height: 40px; border-radius: 7px; border: 1px solid rgba(128, 128, 128, 0.2);
    display: flex; align-items: center; gap: 6px; padding: 0 10px; margin-bottom: 8px;
  }
  .sw-dot { width: 12px; height: 12px; border-radius: 4px; }
  .sw-bar { height: 6px; flex: 1; border-radius: 4px; opacity: 0.35; }
  .sw-title { font-size: 13px; font-weight: 500; color: var(--text); }
  .sw-desc { font-size: 11.5px; color: var(--muted); margin-top: 2px; }
</style>
