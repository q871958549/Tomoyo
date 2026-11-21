<script lang="ts">
  import { Keyboard, SlidersHorizontal, Zap } from '@lucide/svelte';
  import type { PluginEntry, PluginHotkeyDecl, PluginSettingField, SelectOption, TomoyoConfig } from '../../lib/types';
  import type { Lang } from '../../lib/i18n';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import Slider from '../../lib/ui/Slider.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    plugin: PluginEntry;
    cfg: TomoyoConfig;
    lang: Lang;
    onChangeHotkey: (slotKey: string, spec: string) => void;
    onChangeSetting: (pluginId: string, key: string, value: string | number | boolean) => void;
  }

  let { plugin, cfg, lang, onChangeHotkey, onChangeSetting }: Props = $props();
  let isEn = $derived(lang === 'en');
  let pidLow = $derived(plugin.id.toLowerCase());
  let hotkeys = $derived(plugin.declared_hotkeys || []);
  let fields = $derived(plugin.settings_schema || []);
  let events = $derived(plugin.declared_events || []);

  const HK_PRESETS: SelectOption[] = [
    { value: 'Alt+N', label: 'Alt+N' },
    { value: 'Alt+O', label: 'Alt+O' },
    { value: 'Ctrl+Alt+N', label: 'Ctrl+Alt+N' },
    { value: 'Ctrl+Alt+O', label: 'Ctrl+Alt+O' },
    { value: 'Ctrl+Shift+M', label: 'Ctrl+Shift+M' },
  ];

  function hotkeySlotKey(hk: PluginHotkeyDecl): string {
    return `${pidLow}:${hk.id.toLowerCase()}`;
  }

  function currentHotkey(hk: PluginHotkeyDecl): string {
    const map = cfg.plugin_hotkeys || {};
    const k = hotkeySlotKey(hk);
    return k in map ? String(map[k] ?? '') : String(hk.default_hotkey ?? '');
  }

  function currentVal(f: PluginSettingField): string | number | boolean {
    const bag = (cfg.plugin_settings || {})[pidLow] || (cfg.plugin_settings || {})[plugin.id];
    if (bag && f.key in bag) return bag[f.key]!;
    if (f.default !== undefined) return f.default;
    if (f.type === 'bool') return false;
    if (f.type === 'number') return f.min ?? 0;
    return '';
  }
</script>

<div class="v2-drawer" data-plugin-drawer={plugin.id}>
  {#if hotkeys.length > 0}
    <div class="v2-sec">
      <div class="v2-sec-title">
        <Keyboard size={13} />
        <span>{isEn ? 'Declarative Global Hotkeys (Governed by Tomoyo)' : '声明式全局热键（由 Tomoyo 内核统一治理与避让）'}</span>
      </div>
      {#each hotkeys as hk}
        <div class="v2-row">
          <div class="v2-meta">
            <div class="v2-lbl">{hk.label}</div>
            <div class="v2-desc">
              {#if hk.trigger_query}
                {isEn ? `Opens launcher with "${hk.trigger_query}"` : `按下即呼出搜索框并预填「${hk.trigger_query.trim()}」`}
              {:else}
                {isEn ? 'Dispatches Protocol v2 hotkey_triggered event' : '触发 Protocol v2 热键事件'}
              {/if}
            </div>
          </div>
          <HotkeyRecorder
            value={currentHotkey(hk)}
            presets={HK_PRESETS}
            allowDisable={true}
            presetPlaceholder={isEn ? 'Preset' : '预设'}
            disabledLabel={isEn ? 'Unbound' : '未绑定'}
            onchange={(spec) => onChangeHotkey(hotkeySlotKey(hk), spec)}
          />
        </div>
      {/each}
    </div>
  {/if}

  {#if fields.length > 0}
    <div class="v2-sec">
      <div class="v2-sec-title">
        <SlidersHorizontal size={13} />
        <span>{isEn ? 'Plugin Parameters (Protocol v2 Schema)' : '插件运行参数（声明式 Schema 自动生成）'}</span>
      </div>
      {#each fields as f}
        {@const val = currentVal(f)}
        <div class="v2-row">
          <div class="v2-meta">
            <div class="v2-lbl">{f.label}</div>
            {#if f.description}<div class="v2-desc">{f.description}</div>{/if}
          </div>
          <div class="v2-ctrl">
            {#if f.type === 'bool'}
              <Switch
                checked={Boolean(val)}
                ariaLabel={`${plugin.id}:${f.key}`}
                onchange={(v) => onChangeSetting(pidLow, f.key, v)}
              />
            {:else if f.type === 'number'}
              <Slider
                value={Number(val)}
                min={f.min ?? 0}
                max={f.max ?? 100}
                step={f.step ?? 1}
                oninput={(v) => onChangeSetting(pidLow, f.key, v)}
              />
            {:else if f.type === 'select'}
              <Select
                size="sm"
                align="right"
                minWidth="168px"
                value={String(val)}
                options={f.options || []}
                onchange={(v) => onChangeSetting(pidLow, f.key, v)}
              />
            {:else}
              <div class="v2-input">
                <Input
                  size="sm"
                  value={String(val)}
                  placeholder={f.placeholder || ''}
                  oninput={(v) => onChangeSetting(pidLow, f.key, v)}
                />
              </div>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  {#if events.length > 0}
    <div class="v2-events">
      <Zap size={12} />
      <span>{isEn ? 'Subscribed events:' : '已订阅内核事件：'}</span>
      {#each events as ev}<span class="t-pill">{ev}</span>{/each}
    </div>
  {/if}
</div>

<style>
  .v2-drawer {
    margin: 4px 0 10px;
    padding: 12px 14px;
    border-radius: 9px;
    background: var(--canvas);
    border: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .v2-sec { display: flex; flex-direction: column; gap: 8px; }
  .v2-sec-title {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 11.5px; font-weight: 600; color: var(--accent);
  }
  .v2-row {
    display: flex; align-items: center; justify-content: space-between; gap: 14px;
    padding-top: 8px; border-top: 1px dashed var(--border);
  }
  .v2-meta { min-width: 0; flex: 1; }
  .v2-lbl { font-size: 12.5px; font-weight: 500; color: var(--text); }
  .v2-desc { font-size: 11.5px; color: var(--muted); margin-top: 2px; }
  .v2-ctrl { flex-shrink: 0; }
  .v2-input { width: 250px; }
  .v2-events {
    display: flex; align-items: center; gap: 6px; flex-wrap: wrap;
    padding-top: 8px; border-top: 1px dashed var(--border);
    font-size: 11.5px; color: var(--muted);
  }
</style>
