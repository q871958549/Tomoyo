export interface SettingSearchEntry {
  id: string;
  zh: string;
  en: string;
  kw: string;
  tab: 'core' | 'search' | 'hotkeys' | 'flows' | 'plugins' | 'system';
  sub?: string;
  pluginId?: string;
}

export const SETTINGS_SEARCH_INDEX: SettingSearchEntry[] = [
  { id: 'lang', zh: '界面语言 · 启动 · 休眠与同步', en: 'Language, Startup, Hibernate & Sync', kw: '语言 启动 休眠 同步 backup language autostart sync', tab: 'core', sub: 'general' },
  { id: 'super_panel', zh: '超级面板（长按右键 / 鼠标中键快捷翻译）', en: 'Super Panel (Mouse Hold & Instant Translate)', kw: '超级面板 super panel 鼠标 中键 右键 长按 划词 翻译 middle right hold', tab: 'core', sub: 'general' },
  { id: 'theme', zh: '主题配色 · 字体排版 · 窗口尺寸', en: 'Theme, Typography & Window Size', kw: '主题 浅色 深色 午夜 字体 字重 粗细 大小 排版 宽度 行数 预览 theme font weight size typography width', tab: 'core', sub: 'appearance' },
  { id: 'apps', zh: '应用目录扫描 · Everything 检索深度', en: 'App Folders, Depth & Everything Rules', kw: '目录 扫描 深度 deskbox everything 排除 app', tab: 'search', sub: 'indexing' },
  { id: 'filters', zh: '自定义搜索分组筛选标签 (@pdf / @code)', en: 'Custom Search Filter Tabs (@pdf / @code)', kw: '筛选 标签 分组 扩展 filter tab custom', tab: 'search', sub: 'indexing' },
  { id: 'web', zh: '快捷网页搜索前缀 (g / gh / bd)', en: 'Quick Web Search Prefixes', kw: '网页 搜索 前缀 url google github baidu web', tab: 'search', sub: 'web' },
  { id: 'hotkeys', zh: '全局热键冲突检测与抓现行修复', en: 'Global Hotkey Conflicts & Culprit Probe', kw: '热键 冲突 接管 避让 全屏 抓现行 hotkey doctor', tab: 'hotkeys' },
  { id: 'flows', zh: '轻量工作流管道 (cleanpaste / shottr / aisum)', en: 'Declarative Action Flows (flows.toml)', kw: '工作流 管道 ai ollama flow cleanpaste shottr aisum', tab: 'flows', sub: 'flows' },
  { id: 'snippets', zh: '快捷短语与代码片段 (snippets.toml / snip)', en: 'Quick Snippets & Templates (snippets.toml)', kw: '短语 模板 片段 snip snippets date uuid', tab: 'flows', sub: 'snippets' },
  { id: 'clip', zh: '剪贴板插件（热键 / 历史上限 / 隐私黑名单）', en: 'Clipboard Plugin (Hotkey, SQLite & Blocklist)', kw: '剪贴板 复制 历史 隐私 黑名单 图片 clipboard alt+v', tab: 'plugins', sub: 'installed', pluginId: 'dev.tomoyo.clipboard' },
  { id: 'tr', zh: '多源翻译插件（DeepL / OpenAI / Ollama 密钥）', en: 'Translator Plugin (Engines & API Keys)', kw: '翻译 划词 密钥 deepl openai ollama translate alt+t', tab: 'plugins', sub: 'installed', pluginId: 'dev.tomoyo.translate' },
  { id: 'cap', zh: '截图与贴图插件（热键 / 透明度 / 小工具键）', en: 'Screenshot & Pin Plugin (Opacity & Canvas Keys)', kw: '截图 贴图 透明度 取色 长截图 小工具 快捷键 capture pin f3 f1', tab: 'plugins', sub: 'installed', pluginId: 'dev.tomoyo.capture' },
  { id: 'ocr', zh: '离线 OCR 插件（语言 / 热键 / 空格清洗）', en: 'Offline WinRT OCR Plugin Settings', kw: 'ocr 识字 离线 识别 语言', tab: 'plugins', sub: 'installed', pluginId: 'dev.tomoyo.ocr' },
  { id: 'notes', zh: '本地 Markdown 备忘录插件（目录与热键）', en: 'Local Markdown Notes Plugin Settings', kw: '备忘录 笔记 markdown note memo', tab: 'plugins', sub: 'installed', pluginId: 'dev.tomoyo.notes' },
  { id: 'winlayout', zh: '窗口分屏布局与上下文 (WinLayout)', en: 'Window Tiling Layouts & Context (WinLayout)', kw: '窗口 布局 分屏 居中 置顶 winlayout layout left right center', tab: 'plugins', sub: 'installed', pluginId: 'dev.tomoyo.winlayout' },
  { id: 'store', zh: '开源插件市场（校验下载外部插件）', en: 'Plugin Store & Registry Catalog', kw: '插件 商店 市场 清单 下载 store install', tab: 'plugins', sub: 'store' },
  { id: 'updates', zh: '检查更新与一键校验升级', en: 'Check Updates & One-Click Upgrade', kw: '更新 版本 升级 github sha256 update', tab: 'system', sub: 'updates' },
  { id: 'logs', zh: '诊断与日志（主进程 / WebView / 升级日志）', en: 'Diagnostics, Crash & Update Logs', kw: '日志 崩溃 升级 报错 诊断 log crash update.log', tab: 'system', sub: 'logs' },
];

export function searchSettingsEntries(query: string): SettingSearchEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const terms = q.split(/\s+/).filter(Boolean);
  return SETTINGS_SEARCH_INDEX.filter((item) => {
    const hay = `${item.zh} ${item.en} ${item.kw}`.toLowerCase();
    return terms.every((t) => hay.includes(t));
  }).slice(0, 6);
}
