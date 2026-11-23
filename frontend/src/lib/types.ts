export interface HotkeySlotStatus {
  spec: string;
  active: string;
  ok: boolean;
  disabled: boolean;
  overridden?: boolean;
  reason: string;
}

export interface HotkeyHealthReport {
  main?: HotkeySlotStatus;
  clipboard?: HotkeySlotStatus;
  translate?: HotkeySlotStatus;
  capture?: HotkeySlotStatus;
  pin?: HotkeySlotStatus;
  ocr?: HotkeySlotStatus;
  updated_at_unix?: number;
}

export interface SuspectProcess {
  process_name: string;
  pid: number;
  exe_path: string;
  reason: string;
  confidence: 'high' | 'medium' | string;
  remediation?: string;
  can_terminate?: boolean;
}

export interface HotkeyAuditItem {
  spec: string;
  status: 'free' | 'tomoyo' | 'overridden' | 'occupied_external' | 'conflict';
  tomoyo_slot?: string | null;
  suspects: SuspectProcess[];
}

export interface HotkeyAuditReport {
  items: HotkeyAuditItem[];
  recommended_free: Record<string, string[]>;
  total_probed: number;
  external_occupied: number;
  conflicts: number;
  overridden?: number;
}

export interface LiveProbeResult {
  spec: string;
  found: boolean;
  is_free?: boolean;
  confidence?: 'confirmed' | 'likely' | 'none' | string;
  pid: number;
  process_name: string;
  window_title: string;
  exe_path: string;
  detail: string;
}

export interface WebSearchEngine {
  keyword: string;
  name: string;
  url: string;
}

export interface CustomFilterTab {
  id: string;
  label: string;
  tag: string;
  extensions: string[];
  everything_expr: string;
  enabled: boolean;
}

export interface TypographyConfig {
  font_family: string;
  title_weight: number;
  subtitle_weight: number;
  title_size: number;
}

export interface CaptureToolHotkeys {
  rect: string;
  ellipse: string;
  arrow: string;
  pen: string;
  highlighter: string;
  mosaic: string;
  step: string;
  text: string;
  scroll: string;
  pin: string;
  ocr: string;
  ocr_copy: string;
  translate: string;
  record: string;
}

export interface TomoyoConfig {
  language: string;
  hotkey: string;
  override_conflict_hotkeys?: boolean;
  suspend_hotkeys_in_fullscreen?: boolean;
  hotkey_excluded_apps?: string[];
  theme: string;
  hibernate_delay_secs: number;
  auto_preview: boolean;
  show_filter_bar: boolean;
  filter_bar_layout?: 'wrap' | 'single' | string;
  hidden_builtin_filters?: string[];
  custom_filters?: CustomFilterTab[];
  typography?: TypographyConfig;
  disabled_plugins?: string[];
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
    pin_hotkey: string;
    ocr_hotkey: string;
    save_dir: string;
    pin_opacity: number;
    copy_hex_lowercase: boolean;
    record_format?: string;
    record_fps?: number;
    record_audio?: string;
    record_cursor?: boolean;
    tool_hotkeys: CaptureToolHotkeys;
  };
  update: {
    auto_check: boolean;
    check_interval_hours: number;
    repo: string;
    last_checked_unix: number;
    skipped_version?: string | null;
  };
  plugin_registries?: string[];
  plugin_hotkeys?: Record<string, string>;
  plugin_settings?: Record<string, Record<string, string | number | boolean>>;
  plugin_display_modes?: Record<string, 'auto' | 'inline' | 'window' | string>;
  web_searches: WebSearchEngine[];
}

export interface PluginHotkeyDecl {
  id: string;
  label: string;
  default_hotkey?: string;
  trigger_query?: string;
}

export interface PluginSettingOption {
  value: string;
  label: string;
}

export interface PluginSettingField {
  key: string;
  label: string;
  description?: string;
  type: 'text' | 'bool' | 'number' | 'select';
  placeholder?: string;
  min?: number;
  max?: number;
  step?: number;
  options?: PluginSettingOption[];
  default?: string | number | boolean;
}

export interface PluginEntry {
  id: string;
  name: string;
  version?: string;
  description?: string;
  author?: string;
  license?: string;
  homepage?: string;
  category?: string;
  display_mode?: string;
  icon_url?: string;
  runtime: string;
  keywords: string[];
  max_memory_mb: number;
  declared_permissions: string[];
  granted_permissions: string[];
  bundled_trusted?: boolean;
  declared_hotkeys?: PluginHotkeyDecl[];
  declared_events?: string[];
  settings_schema?: PluginSettingField[];
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

export interface WorkflowStep {
  kind: string;
  value?: string;
}

export interface WorkflowDef {
  id: string;
  name: string;
  keyword: string;
  hotkey?: string;
  description: string;
  enabled: boolean;
  steps: WorkflowStep[];
}

export interface SettingsInitPayload {
  version: string;
  platform?: string;
  config: TomoyoConfig;
  autostart: boolean;
  plugins: PluginEntry[];
  flows?: WorkflowDef[];
  flows_path?: string;
  config_path: string;
  sync_dir?: string;
  plugins_dir: string;
  logs_dir: string;
  running_apps: string[];
  secrets_status: { deepl: boolean; openai: boolean };
  hotkey_health?: HotkeyHealthReport;
  cached_update?: UpdateStatus | null;
  system_fonts?: string[];
  official_icons?: Record<string, string>;
  quiet_md_path?: string;
  initial_route?: string;
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
