export interface HotkeySlotStatus {
  spec: string;
  active: string;
  ok: boolean;
  disabled: boolean;
  reason: string;
}

export interface HotkeyHealthReport {
  main?: HotkeySlotStatus;
  clipboard?: HotkeySlotStatus;
  translate?: HotkeySlotStatus;
  capture?: HotkeySlotStatus;
  updated_at_unix?: number;
}

export interface WebSearchEngine {
  keyword: string;
  name: string;
  url: string;
}

export interface TomoyoConfig {
  language: string;
  hotkey: string;
  theme: string;
  hibernate_delay_secs: number;
  auto_preview: boolean;
  show_filter_bar: boolean;
  window: {
    width_dip: number;
    max_visible_rows: number;
    preview_width_dip: number;
    custom_x: number | null;
    custom_y: number | null;
  };
  apps: {
    extra_dirs: string[];
    auto_detect_desktop_organizers: boolean;
    extra_scan_depth: number;
  };
  everything: {
    max_global_results: number;
    max_dedicated_results: number;
    extra_filter: string;
    favorites?: Array<{ tag: string; label: string; expr: string }>;
  };
  clipboard: {
    persist_sqlite: boolean;
    max_persisted_items: number;
    hotkey: string;
    capture_enabled: boolean;
    capture_images: boolean;
    excluded_apps: string[];
  };
  translate: {
    hotkey: string;
    target_lang: string;
    engines: string[];
    openai_base_url: string;
    openai_model: string;
    ollama_url: string;
    ollama_model: string;
  };
  capture: {
    hotkey: string;
    save_dir: string;
    pin_opacity: number;
    copy_hex_lowercase: boolean;
  };
  update: {
    auto_check: boolean;
    check_interval_hours: number;
    repo: string;
    last_checked_unix: number;
    skipped_version?: string | null;
  };
  plugin_registries?: string[];
  web_searches: WebSearchEngine[];
}

export interface PluginEntry {
  id: string;
  name: string;
  runtime: string;
  keywords: string[];
  max_memory_mb: number;
  declared_permissions: string[];
  granted_permissions: string[];
  bundled_trusted?: boolean;
}

export interface ReleaseInfo {
  version: string;
  name: string;
  html_url: string;
  download_url?: string;
  notes?: string;
  source?: string;
}

export interface UpdateStatus {
  state: 'up_to_date' | 'available' | 'error';
  current?: string;
  latest?: string;
  note?: string;
  message?: string;
  checked_at_unix?: number;
  release?: ReleaseInfo;
}

export interface SettingsInitPayload {
  version: string;
  config: TomoyoConfig;
  autostart: boolean;
  plugins: PluginEntry[];
  config_path: string;
  plugins_dir: string;
  logs_dir: string;
  running_apps: string[];
  secrets_status: { deepl: boolean; openai: boolean };
  hotkey_health?: HotkeyHealthReport;
  cached_update?: UpdateStatus | null;
}

export interface SelectOption {
  value: string;
  label: string;
  hint?: string;
}

export interface ContextMenuItem {
  id: string;
  label: string;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
  separator?: boolean;
}
