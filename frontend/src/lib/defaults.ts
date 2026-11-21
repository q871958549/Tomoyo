import { OFFICIAL_REPO } from './i18n';
import type { SettingsInitPayload, TomoyoConfig } from './types';

export function makeDefaultConfig(): TomoyoConfig {
  return {
    language: 'zh',
    hotkey: 'Alt+Space',
    theme: 'light',
    hibernate_delay_secs: 15,
    auto_preview: true,
    show_filter_bar: true,
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
    capture: { hotkey: 'Alt+S', save_dir: '', pin_opacity: 100, copy_hex_lowercase: false },
    update: { auto_check: true, check_interval_hours: 24, repo: OFFICIAL_REPO, last_checked_unix: 0 },
    web_searches: [],
  };
}

export function hydrateSettingsInit(): SettingsInitPayload {
  const raw = (window.__TOMOYO_SETTINGS_INIT__ ?? {}) as Partial<SettingsInitPayload>;
  const def = makeDefaultConfig();
  const rc = (raw.config ?? {}) as Partial<TomoyoConfig>;
  return {
    version: raw.version || '1.6.1',
    autostart: !!raw.autostart,
    plugins: Array.isArray(raw.plugins) ? raw.plugins : [],
    config_path: raw.config_path || '',
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
      capture: { ...def.capture, ...(rc.capture || {}) },
      update: { ...def.update, ...(rc.update || {}) },
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
