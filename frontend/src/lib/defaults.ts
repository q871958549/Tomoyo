import { OFFICIAL_REPO } from './i18n';
import type { CaptureToolHotkeys, SettingsInitPayload, TomoyoConfig } from './types';

export const PLUGIN_CLIPBOARD = 'dev.tomoyo.clipboard';
export const PLUGIN_TRANSLATE = 'dev.tomoyo.translate';
export const PLUGIN_CAPTURE = 'dev.tomoyo.capture';
export const PLUGIN_OCR = 'dev.tomoyo.ocr';
export const PLUGIN_NOTES = 'dev.tomoyo.notes';
export const PLUGIN_MD_EDITOR = 'dev.tomoyo.markdown-editor';

export interface OfficialPluginMeta {
  id: string;
  category: string;
  zhName: string;
  enName: string;
  zhDesc: string;
  enDesc: string;
  triggers: string;
}

export const OFFICIAL_PLUGINS: OfficialPluginMeta[] = [
  {
    id: PLUGIN_CLIPBOARD,
    category: 'productivity',
    zhName: '剪贴板管理中心',
    enName: 'Clipboard Center',
    zhDesc: '历史采集、图片缓存、隐私屏蔽与工作台；关闭即卸载系统剪贴板监听与热键',
    enDesc: 'History, image cache, privacy filter & workbench; unhooks OS listener & hotkey when off',
    triggers: 'cb / clip',
  },
  {
    id: PLUGIN_CAPTURE,
    category: 'media',
    zhName: '原生截图与贴图',
    enName: 'Native Capture & Pin',
    zhDesc: '区域截图、平滑标注、长截图实时预览、取色器与贴图（按需独立进程）',
    enDesc: 'Region capture, smooth annotations, live long-capture & pins (ephemeral process)',
    triggers: 'cap / pin',
  },
];

export function isPluginEnabled(cfg: Pick<TomoyoConfig, 'disabled_plugins'>, id: string): boolean {
  const list = cfg.disabled_plugins || [];
  const target = id.toLowerCase();
  return !list.some((x) => x.toLowerCase() === target);
}

export function defaultCaptureToolHotkeys(): CaptureToolHotkeys {
  return {
    rect: 'R',
    ellipse: 'E',
    arrow: 'A',
    pen: 'B',
    highlighter: 'H',
    mosaic: 'M',
    step: 'N',
    text: 'X',
    scroll: 'L',
    pin: 'P',
    ocr: 'O',
    ocr_copy: 'Shift+C',
    translate: 'T',
    record: 'V',
  };
}

export function makeDefaultConfig(): TomoyoConfig {
  return {
    language: 'zh',
    hotkey: 'Alt+Space',
    override_conflict_hotkeys: true,
    suspend_hotkeys_in_fullscreen: true,
    hotkey_excluded_apps: ['raylink.exe', 'mstsc.exe', 'parsec.exe', 'moonlight.exe', 'todesk.exe', 'sunloginclient.exe', 'anydesk.exe'],
    theme: 'light',
    hibernate_delay_secs: 15,
    auto_preview: true,
    show_filter_bar: true,
    filter_bar_layout: 'wrap',
    hidden_builtin_filters: [],
    custom_filters: [],
    typography: {
      font_family: '',
      title_weight: 500,
      subtitle_weight: 400,
      title_size: 16,
    },
    disabled_plugins: [],
    window: { width_dip: 720, max_visible_rows: 8, preview_width_dip: 310, custom_x: null, custom_y: null },
    apps: { extra_dirs: [], auto_detect_desktop_organizers: true, extra_scan_depth: 2 },
    everything: { max_global_results: 6, max_dedicated_results: 30, extra_filter: '' },
    clipboard: {
      persist_sqlite: true,
      max_persisted_items: 500,
      hotkey: 'Alt+V',
      capture_enabled: true,
      capture_images: true,
      excluded_apps: [],
    },
    translate: {
      hotkey: 'Alt+T',
      target_lang: 'auto',
      engines: ['google', 'mymemory'],
      openai_base_url: 'https://api.openai.com/v1',
      openai_model: 'gpt-4o-mini',
      ollama_url: 'http://127.0.0.1:11434',
      ollama_model: 'qwen2.5:7b',
    },
    capture: {
      hotkey: 'Alt+S',
      pin_hotkey: 'F3',
      ocr_hotkey: '',
      record_hotkey: '',
      save_dir: '',
      pin_opacity: 100,
      copy_hex_lowercase: false,
      record_format: 'mp4',
      record_fps: 30,
      record_audio: 'none',
      record_cursor: true,
      tool_hotkeys: defaultCaptureToolHotkeys(),
    },
    update: { auto_check: true, check_interval_hours: 24, repo: OFFICIAL_REPO, last_checked_unix: 0 },
    plugin_hotkeys: {},
    plugin_settings: {},
    plugin_display_modes: {},
    web_searches: [],
  };
}

export function hydrateSettingsInit(): SettingsInitPayload {
  const raw = (window.__TOMOYO_SETTINGS_INIT__ ?? {}) as Partial<SettingsInitPayload>;
  const def = makeDefaultConfig();
  const rc = (raw.config ?? {}) as Partial<TomoyoConfig>;
  return {
    version: raw.version || '3.0.0',
    platform: raw.platform || 'linux',
    autostart: !!raw.autostart,
    plugins: Array.isArray(raw.plugins) ? raw.plugins : [],
    flows: Array.isArray(raw.flows) ? raw.flows : [],
    flows_path: raw.flows_path || '',
    config_path: raw.config_path || '',
    sync_dir: raw.sync_dir || '',
    plugins_dir: raw.plugins_dir || '',
    logs_dir: raw.logs_dir || '',
    running_apps: Array.isArray(raw.running_apps) ? raw.running_apps : [],
    secrets_status: {
      deepl: !!raw.secrets_status?.deepl,
      openai: !!raw.secrets_status?.openai,
      plugins: raw.secrets_status?.plugins && typeof raw.secrets_status.plugins === 'object' ? raw.secrets_status.plugins : {},
    },
    hotkey_health: raw.hotkey_health || {},
    cached_update: raw.cached_update ?? null,
    official_icons: typeof raw.official_icons === 'object' && raw.official_icons ? raw.official_icons : {},
    system_fonts: Array.isArray(raw.system_fonts) ? raw.system_fonts : [],
    initial_route: typeof raw.initial_route === 'string' ? raw.initial_route : '',
    config: {
      ...def,
      ...rc,
      filter_bar_layout: rc.filter_bar_layout === 'single' ? 'single' : 'wrap',
      typography: { ...def.typography!, ...(rc.typography || {}) },
      window: { ...def.window, ...(rc.window || {}) },
      apps: { ...def.apps, ...(rc.apps || {}) },
      everything: { ...def.everything, ...(rc.everything || {}) },
      clipboard: { ...def.clipboard, ...(rc.clipboard || {}) },
      translate: { ...def.translate, ...(rc.translate || {}) },
      capture: {
        ...def.capture,
        ...(rc.capture || {}),
        tool_hotkeys: {
          ...def.capture.tool_hotkeys,
          ...(rc.capture?.tool_hotkeys || {}),
        },
      },
      update: { ...def.update, ...(rc.update || {}) },
      hidden_builtin_filters: Array.isArray(rc.hidden_builtin_filters) ? rc.hidden_builtin_filters : [],
      custom_filters: Array.isArray(rc.custom_filters) ? rc.custom_filters : [],
      disabled_plugins: Array.isArray(rc.disabled_plugins) ? rc.disabled_plugins : [],
      suspend_hotkeys_in_fullscreen: rc.suspend_hotkeys_in_fullscreen ?? true,
      hotkey_excluded_apps: Array.isArray(rc.hotkey_excluded_apps) ? rc.hotkey_excluded_apps : def.hotkey_excluded_apps,
      plugin_hotkeys: rc.plugin_hotkeys && typeof rc.plugin_hotkeys === 'object' ? rc.plugin_hotkeys : {},
      plugin_settings: rc.plugin_settings && typeof rc.plugin_settings === 'object' ? rc.plugin_settings : {},
      web_searches: Array.isArray(rc.web_searches) ? rc.web_searches : def.web_searches,
    },
  };
}

export function localizeRuntime(kind: string, isEn: boolean): string {
  const k = (kind || '').toLowerCase();
  if (isEn) {
    if (k === 'stdio_jsonrpc' || k === 'stdio') return 'Native STDIO';
    if (k === 'wasm') return 'WASM Sandbox';
    if (k === 'webview') return 'On-Demand WebView';
    return kind;
  }
  if (k === 'stdio_jsonrpc' || k === 'stdio') return '原生进程';
  if (k === 'wasm') return 'WASM 沙箱';
  if (k === 'webview') return '按需界面';
  return kind;
}

export function localizePermission(perm: string, isEn: boolean): string {
  if (isEn) return perm;
  const map: Record<string, string> = {
    clipboard: '剪贴板访问',
    network: '网络请求',
    fs: '文件读写',
    filesystem: '文件读写',
    filesystem_read: '读取本地文件',
    filesystem_write: '写入本地文件',
    shell: '执行系统命令',
    shell_exec: '执行系统命令',
    system_info: '系统基础信息',
    selection_read: '无痕划词读取',
    process: '进程管理',
    notification: '系统通知',
  };
  return map[perm.toLowerCase()] ?? perm;
}
