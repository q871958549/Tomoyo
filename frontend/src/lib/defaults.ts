import { OFFICIAL_REPO } from './i18n';
import type { CaptureToolHotkeys, SettingsInitPayload, TomoyoConfig } from './types';

export const PLUGIN_CLIPBOARD = 'dev.tomoyo.clipboard';
export const PLUGIN_TRANSLATE = 'dev.tomoyo.translate';
export const PLUGIN_CAPTURE = 'dev.tomoyo.capture';
export const PLUGIN_OCR = 'dev.tomoyo.ocr';
export const PLUGIN_NOTES = 'dev.tomoyo.notes';

export interface OfficialPluginMeta {
  id: string;
  zhName: string;
  enName: string;
  zhDesc: string;
  enDesc: string;
  triggers: string;
}

export const OFFICIAL_PLUGINS: OfficialPluginMeta[] = [
  {
    id: PLUGIN_CLIPBOARD,
    zhName: '剪贴板管理中心',
    enName: 'Clipboard Center',
    zhDesc: '历史采集、图片缓存、隐私屏蔽与工作台；关闭即卸载系统剪贴板监听与热键',
    enDesc: 'History, image cache, privacy filter & workbench; unhooks OS listener & hotkey when off',
    triggers: 'cb / clip',
  },
  {
    id: PLUGIN_TRANSLATE,
    zhName: '多源划词翻译',
    enName: 'Multi-Source Translator',
    zhDesc: '划词即译、中英词典音标与多引擎并行对比；关闭即释放热键与入口',
    enDesc: 'Selection translate, dictionary & parallel engines; releases hotkey & commands when off',
    triggers: 'tr / fy',
  },
  {
    id: PLUGIN_CAPTURE,
    zhName: '原生截图与贴图',
    enName: 'Native Capture & Pin',
    zhDesc: '区域截图、平滑标注、长截图实时预览、取色器与贴图（按需独立进程）',
    enDesc: 'Region capture, smooth annotations, live long-capture & pins (ephemeral process)',
    triggers: 'cap / pin',
  },
  {
    id: PLUGIN_OCR,
    zhName: '离线识图 OCR',
    enName: 'Offline WinRT OCR',
    zhDesc: '本地 WinRT 离线识字与审校面板（独立 STDIO 沙箱插件，零云端上传）',
    enDesc: 'Local Windows WinRT OCR & proofreading panel (isolated STDIO plugin)',
    triggers: 'ocr',
  },
  {
    id: PLUGIN_NOTES,
    zhName: '本地 Markdown 备忘录',
    enName: 'Local Markdown Notes',
    zhDesc: '纯外部独立插件：本地 .md 文件全文/标签检索、一键新建与剪贴板转存（0 常驻内存）',
    enDesc: 'Pure standalone STDIO plugin: local .md full-text/tag search & quick capture (0 idle RAM)',
    triggers: 'note / memo',
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
    translate: 'T',
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
    hidden_builtin_filters: [],
    custom_filters: [],
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
      save_dir: '',
      pin_opacity: 100,
      copy_hex_lowercase: false,
      tool_hotkeys: defaultCaptureToolHotkeys(),
    },
    update: { auto_check: true, check_interval_hours: 24, repo: OFFICIAL_REPO, last_checked_unix: 0 },
    web_searches: [],
  };
}

export function hydrateSettingsInit(): SettingsInitPayload {
  const raw = (window.__TOMOYO_SETTINGS_INIT__ ?? {}) as Partial<SettingsInitPayload>;
  const def = makeDefaultConfig();
  const rc = (raw.config ?? {}) as Partial<TomoyoConfig>;
  return {
    version: raw.version || '1.9.1',
    autostart: !!raw.autostart,
    plugins: Array.isArray(raw.plugins) ? raw.plugins : [],
    config_path: raw.config_path || '',
    sync_dir: raw.sync_dir || '',
    plugins_dir: raw.plugins_dir || '',
    logs_dir: raw.logs_dir || '',
    running_apps: Array.isArray(raw.running_apps) ? raw.running_apps : [],
    secrets_status: { deepl: !!raw.secrets_status?.deepl, openai: !!raw.secrets_status?.openai },
    hotkey_health: raw.hotkey_health || {},
    cached_update: raw.cached_update ?? null,
    config: {
      ...def,
      ...rc,
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
    shell: '执行系统命令',
    process: '进程管理',
    notification: '系统通知',
  };
  return map[perm.toLowerCase()] ?? perm;
}
