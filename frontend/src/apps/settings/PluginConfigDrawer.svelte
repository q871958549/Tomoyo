<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
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
    pluginSecrets?: Record<string, boolean>;
    onChangeDisplayMode: (pluginId: string, mode: string) => void;
    onChangeHotkey: (slotKey: string, spec: string) => void;
    onChangeSetting: (pluginId: string, key: string, value: string | number | boolean) => void;
  }

  let {
    plugin, cfg, lang, pluginSecrets = {},
    onChangeDisplayMode, onChangeHotkey, onChangeSetting,
  }: Props = $props();
  let isEn = $derived(lang === 'en');
  let pidLow = $derived(plugin.id.toLowerCase());
  let hotkeys = $derived(plugin.declared_hotkeys || []);
  let fields = $derived(plugin.settings_schema || []);
  let events = $derived(plugin.declared_events || []);
  let secretDrafts = $state<Record<string, string>>({});
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

  function isFieldVisible(f: PluginSettingField): boolean {
    if (!f.visible_if) return true;
    const target = fields.find((x) => x.key === f.visible_if!.key);
    const actual = target
      ? currentVal(target)
      : ((cfg.plugin_settings || {})[pidLow] || {})[f.visible_if.key];
    return actual === f.visible_if.equals;
  }

  let visibleFields = $derived(fields.filter((f) => isFieldVisible(f)));

  function secretFullKey(fieldKey: string): string {
    return `plugin:${pidLow}:${fieldKey}`;
  }

  function saveSecretField(fieldKey: string) {
    const val = (secretDrafts[fieldKey] || '').trim();
    if (!val) return;
    postHost({ cmd: 'set_secret', key: secretFullKey(fieldKey), value: val });
    secretDrafts = { ...secretDrafts, [fieldKey]: '' };
  }

  function clearSecretField(fieldKey: string) {
    postHost({ cmd: 'delete_secret', key: secretFullKey(fieldKey) });
  }

  function fieldHotkeyPresets(f: PluginSettingField): SelectOption[] {
    if (f.presets && f.presets.length > 0) {
      return f.presets.map((p) => ({ value: p, label: p }));
    }
    return HK_PRESETS;
  }
</script>

<div class="v2-drawer" data-plugin-drawer={plugin.id}>
  {#if plugin.has_ui}
    <div class="v3-ui-launch">
      <Button variant="secondary" size="sm" onclick={() => postHost({ cmd: 'open_plugin_ui', plugin_id: plugin.id, query: '' })}>
        <Icon name="ExternalLink" size={12} />
        {isEn ? 'Open Plugin Window' : '打开插件工作台'}
      </Button>
    </div>
  {/if}

  <div class="v2-sec">
    <div class="v2-sec-title">
      <Icon name="AppWindow" size={13} />
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

  {#if hotkeys.length > 0}
    <div class="v2-sec">
      <div class="v2-sec-title">
        <Icon name="Keyboard" size={13} />
        <span>{isEn ? 'Declarative Global Hotkeys (Governed by Tomoyo)' : '声明式全局热键（由 Tomoyo 内核统一治理与避让）'}</span>
      </div>
      {#each hotkeys as hk}
        <div class="v2-row">
          <div class="v2-meta">
            <div class="v2-lbl">{hk.label}</div>
            <div class="v2-desc">
              {#if hk.trigger_query}
                {isEn ? `Opens launcher with "${hk.trigger_query}"` : `按下即呼出搜索框并预填「${hk.trigger_query.trim()}」`}
              {:else if hk.capture_selection}
                {isEn ? 'Captures foreground selection losslessly & dispatches hotkey_triggered' : '无痕读取当前选中文本并触发插件热键事件（需 SelectionRead 权限）'}
              {:else}
                {isEn ? 'Dispatches Protocol v3 hotkey_triggered event directly' : '直接触发插件热键事件（可无搜索框直拉插件窗）'}
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

  {#if visibleFields.length > 0}
    <div class="v2-sec">
      <div class="v2-sec-title">
        <Icon name="SlidersHorizontal" size={13} />
        <span>{isEn ? 'Plugin Parameters (Protocol v3 Declarative Schema)' : '插件运行参数（Protocol v3 声明式 Schema 自动生成）'}</span>
      </div>
      {#each visibleFields as f, idx}
        {@const prevSection = idx > 0 ? (visibleFields[idx - 1]?.section || '') : ''}
        {#if f.section && f.section !== prevSection}
          <div class="v3-section">{f.section}</div>
        {/if}
        {@const val = currentVal(f)}
        {@const secretConfigured = f.type === 'secret' && !!pluginSecrets[secretFullKey(f.key)]}
        <div class="v2-row">
          <div class="v2-meta">
            <div class="v2-lbl-row">
              <span class="v2-lbl">{f.label}</span>
              {#if f.type === 'secret'}
                <span class="t-pill {secretConfigured ? 'is-ok' : ''}">
                  <Icon name="KeyRound" size={10} />
                  {secretConfigured ? (isEn ? 'Encrypted' : '已加密保存') : (isEn ? 'Not set' : '未配置')}
                </span>
              {/if}
            </div>
            {#if f.description}<div class="v2-desc">{f.description}</div>{/if}
          </div>
          <div class="v2-ctrl">
            {#if f.type === 'bool'}
              <Switch checked={Boolean(val)} ariaLabel={`${plugin.id}:${f.key}`} onchange={(v) => onChangeSetting(pidLow, f.key, v)} />
            {:else if f.type === 'number'}
              <Slider value={Number(val)} min={f.min ?? 0} max={f.max ?? 100} step={f.step ?? 1} oninput={(v) => onChangeSetting(pidLow, f.key, v)} />
            {:else if f.type === 'select'}
              <Select size="sm" align="right" minWidth="168px" value={String(val)} options={f.options || []} onchange={(v) => onChangeSetting(pidLow, f.key, v)} />
            {:else if f.type === 'hotkey'}
              <HotkeyRecorder
                value={String(val)}
                presets={fieldHotkeyPresets(f)}
                allowDisable={true}
                presetPlaceholder={isEn ? 'Preset' : '预设'}
                disabledLabel={isEn ? 'Unbound' : '未绑定'}
                onchange={(spec) => onChangeSetting(pidLow, f.key, spec)}
              />
            {:else if f.type === 'secret'}
              <div class="v3-inline">
                <div class="v2-input sm">
                  <Input
                    size="sm"
                    type="password"
                    value={secretDrafts[f.key] || ''}
                    placeholder={secretConfigured ? (isEn ? 'Enter new secret to overwrite…' : '输入新密钥以覆盖…') : (f.placeholder || 'sk-...')}
                    oninput={(v) => (secretDrafts = { ...secretDrafts, [f.key]: v })}
                  />
                </div>
                <Button variant="primary" size="sm" onclick={() => saveSecretField(f.key)}>{isEn ? 'Save' : '加密保存'}</Button>
                {#if secretConfigured}
                  <Button variant="secondary" size="sm" onclick={() => clearSecretField(f.key)} title={isEn ? 'Clear secret' : '清除密钥'}><Icon name="Trash2" size={12} /></Button>
                {/if}
              </div>
            {:else if f.type === 'dir' || f.type === 'file'}
              <div class="v3-inline">
                <div class="v2-input sm">
                  <Input size="sm" value={String(val)} placeholder={f.placeholder || ''} oninput={(v) => onChangeSetting(pidLow, f.key, v)} />
                </div>
                <Button
                  variant="secondary"
                  size="sm"
                  onclick={() => postHost({ cmd: 'pick_plugin_path', plugin_id: pidLow, key: f.key, kind: f.type })}
                >
                  <Icon name="FolderOpen" size={12} />
                  {f.type === 'file' ? (isEn ? 'File…' : '选文件') : (isEn ? 'Dir…' : '选目录')}
                </Button>
              </div>
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

  {#if events.length > 0 || plugin.has_ui || (plugin.tray_actions_count || 0) > 0 || (plugin.file_actions_count || 0) > 0}
    <div class="v2-events">
      <Icon name="Zap" size={12} />
      <span>{isEn ? 'Capabilities:' : '扩展钩子：'}</span>
      {#if plugin.has_ui}<span class="t-pill">Hybrid UI</span>{/if}
      {#if (plugin.tray_actions_count || 0) > 0}<span class="t-pill">{isEn ? `${plugin.tray_actions_count} tray` : `${plugin.tray_actions_count} 项托盘`}</span>{/if}
      {#if (plugin.file_actions_count || 0) > 0}<span class="t-pill">{isEn ? `${plugin.file_actions_count} file ext` : `${plugin.file_actions_count} 项文件动作`}</span>{/if}
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
  .v3-section { font-size: 11px; font-weight: 600; color: var(--muted); text-transform: uppercase; letter-spacing: 0.03em; padding-top: 6px; }
  .v2-row { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding-top: 8px; border-top: 1px dashed var(--border); }
  .v2-meta { min-width: 0; flex: 1; }
  .v2-lbl-row { display: inline-flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .v2-lbl { font-size: 12.5px; font-weight: 500; color: var(--text); }
  .v2-desc { font-size: 11.5px; color: var(--muted); margin-top: 2px; }
  .v2-ctrl { flex-shrink: 0; }
  .v2-input { width: 250px; }
  .v2-input.sm { width: 196px; }
  .v3-inline { display: flex; align-items: center; gap: 6px; }
  .v3-ui-launch { display: flex; justify-content: flex-end; }
  .v2-events { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; padding-top: 8px; border-top: 1px dashed var(--border); font-size: 11.5px; color: var(--muted); }
  .t-pill.is-ok { color: var(--ok); border-color: rgba(16, 185, 129, 0.35); background: rgba(16, 185, 129, 0.1); }
</style>
