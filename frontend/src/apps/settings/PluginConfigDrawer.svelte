<script lang="ts">
  import { AppWindow, Download, ExternalLink, Keyboard, SlidersHorizontal, Zap } from '@lucide/svelte';
  import { PLUGIN_MD_EDITOR } from '../../lib/defaults';
  import { postHost } from '../../lib/ipc';
  import type { PluginEntry, PluginHotkeyDecl, PluginSettingField, SelectOption, TomoyoConfig } from '../../lib/types';
  import type { Lang } from '../../lib/i18n';
  import Button from '../../lib/ui/Button.svelte';
  import HotkeyRecorder from '../../lib/ui/HotkeyRecorder.svelte';
  import Input from '../../lib/ui/Input.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Select from '../../lib/ui/Select.svelte';
  import Slider from '../../lib/ui/Slider.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  interface Props {
    plugin: PluginEntry;
    cfg: TomoyoConfig;
    lang: Lang;
    quietMdPath?: string;
    installingQuietMd?: boolean;
    onInstallQuietMd?: () => void;
    onChangeDisplayMode: (pluginId: string, mode: string) => void;
    onChangeHotkey: (slotKey: string, spec: string) => void;
    onChangeSetting: (pluginId: string, key: string, value: string | number | boolean) => void;
  }

  let {
    plugin, cfg, lang, quietMdPath = '', installingQuietMd = false, onInstallQuietMd,
    onChangeDisplayMode, onChangeHotkey, onChangeSetting,
  }: Props = $props();
  let isEn = $derived(lang === 'en');
  let pidLow = $derived(plugin.id.toLowerCase());
  let hotkeys = $derived(plugin.declared_hotkeys || []);
  let fields = $derived(plugin.settings_schema || []);
  let events = $derived(plugin.declared_events || []);
  let curMode = $derived((cfg.plugin_display_modes || {})[pidLow] || 'auto');
  let defLabel = $derived(
    plugin.display_mode === 'inline'
      ? (isEn ? 'Auto (Inline)' : '跟随默认 (内嵌)')
      : plugin.display_mode === 'window'
        ? (isEn ? 'Auto (Window)' : '跟随默认 (独立窗)')
        : (isEn ? 'Auto' : '跟随插件默认')
  );

  const HK_PRESETS: SelectOption[] = [
    { value: 'Alt+N', label: 'Alt+N' },
    { value: 'Alt+O', label: 'Alt+O' },
    { value: 'Alt+M', label: 'Alt+M' },
    { value: 'Ctrl+Alt+N', label: 'Ctrl+Alt+N' },
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
  <div class="v2-sec">
    <div class="v2-sec-title">
      <AppWindow size={13} />
      <span>{isEn ? 'Presentation Mode (Inline vs Independent Window)' : '窗口呈现形式（搜索内嵌预览 vs 独立窗口分离）'}</span>
    </div>
    <div class="v2-row" style="border-top:0;padding-top:2px">
      <div class="v2-meta">
        <div class="v2-lbl">{isEn ? 'Launch & View Placement' : '运行显示位置'}</div>
        <div class="v2-desc">{isEn ? 'Inline docks inside Tomoyo preview pane; Window detaches with its own logo' : '内嵌在搜索结果右侧展示（可随时点右上角分离）；独立窗带专属 Logo 图标'}</div>
      </div>
      <Segmented
        value={curMode}
        options={[
          { value: 'auto', label: defLabel },
          { value: 'inline', label: isEn ? 'Inline' : '搜索结果内嵌' },
          { value: 'window', label: isEn ? 'Window' : '独立窗口' },
        ]}
        onchange={(m) => onChangeDisplayMode(pidLow, m)}
      />
    </div>
  </div>

  {#if pidLow === PLUGIN_MD_EDITOR}
    <div class="v2-sec">
      <div class="v2-row" style="border-top:0;padding-top:2px">
        <div class="v2-meta">
          <div class="v2-lbl">{isEn ? 'Quiet.md Engine Status' : 'Quiet.md 桌面编辑器就绪状态 (s1dashu/quiet-md · MIT)'}</div>
          <div class="v2-desc">
            {#if quietMdPath}
              {isEn ? `Detected: ${quietMdPath}` : `已就绪：${quietMdPath}`}
            {:else}
              {isEn ? 'Not installed yet — one-click SHA-256 verified install keeps tomoyo.exe lean' : '暂未检测到本机 Quiet.md — 支持从官方 GitHub Releases 一键 SHA-256 校验安装'}
            {/if}
          </div>
        </div>
        <div style="display:flex;gap:6px;flex-shrink:0">
          {#if quietMdPath}
            <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'open_path', path: quietMdPath })}>
              <ExternalLink size={13} />{isEn ? 'Launch Quiet.md' : '立即启动'}
            </Button>
          {:else}
            <Button variant="primary" size="sm" disabled={installingQuietMd} onclick={() => onInstallQuietMd?.()}>
              <Download size={13} />{installingQuietMd ? (isEn ? 'Installing…' : '安装中…') : (isEn ? 'Install Quiet.md' : '一键校验安装')}
            </Button>
          {/if}
        </div>
      </div>
    </div>
  {/if}

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
              <Switch checked={Boolean(val)} ariaLabel={`${plugin.id}:${f.key}`} onchange={(v) => onChangeSetting(pidLow, f.key, v)} />
            {:else if f.type === 'number'}
              <Slider value={Number(val)} min={f.min ?? 0} max={f.max ?? 100} step={f.step ?? 1} oninput={(v) => onChangeSetting(pidLow, f.key, v)} />
            {:else if f.type === 'select'}
              <Select size="sm" align="right" minWidth="168px" value={String(val)} options={f.options || []} onchange={(v) => onChangeSetting(pidLow, f.key, v)} />
            {:else}
              <div class="v2-input">
                <Input size="sm" value={String(val)} placeholder={f.placeholder || ''} oninput={(v) => onChangeSetting(pidLow, f.key, v)} />
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
    margin: 4px 0 10px; padding: 12px 14px; border-radius: 9px;
    background: var(--canvas); border: 1px solid var(--border);
    display: flex; flex-direction: column; gap: 12px;
  }
  .v2-sec { display: flex; flex-direction: column; gap: 8px; }
  .v2-sec-title { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; font-weight: 600; color: var(--accent); }
  .v2-row { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding-top: 8px; border-top: 1px dashed var(--border); }
  .v2-meta { min-width: 0; flex: 1; }
  .v2-lbl { font-size: 12.5px; font-weight: 500; color: var(--text); }
  .v2-desc { font-size: 11.5px; color: var(--muted); margin-top: 2px; }
  .v2-ctrl { flex-shrink: 0; }
  .v2-input { width: 250px; }
  .v2-events { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; padding-top: 8px; border-top: 1px dashed var(--border); font-size: 11.5px; color: var(--muted); }
</style>
