# Tomoyo 长期演进蓝图与分阶段替代计划 (Long-Term Roadmap)

> **定位与基线**：`v1.6.0` 已在完整覆盖主链路检索、全链路北京时间日志溯源、离线 WinRT OCR 审校、增强剪贴板工作台（替代 CopyQ）、多引擎翻译（替代 Pot）、零配额自动更新闭环与原生按需截屏贴图（替代 PixPin）的基础上，完成了 **极简现代（Minimalist Modern · Linear × Raycast）全栈 UI/UX 深度重构**：引入 **Svelte 5 + Tailwind CSS v4 + TypeScript + `@lucide/svelte` + Vite 单文件 IIFE** 工程化前端并保留 0 MB 常驻 WebView 架构、建立全部控件设计体系、落盘全局热键真实系统注册状态回执（根治组合热键被占无提示），并对 `tomoyo-capture.exe` 截屏工具栏与原生右键菜单实施抗锯齿 SDF 视觉升级。  
> **产品目标**：以 Windows 原生低占用启动器为核心，按垂直场景逐一打磨高频桌面工具能力，在各模块经过实机长期验证后，逐步收敛并替代日常分散常驻的 **CopyQ（剪贴板）**、**Pot（划词/截图翻译）**、**PixPin（截图/贴图）** 以及轻量本地备忘录，减少后台常驻进程与内存碎片。  
> **执行纪律**：每个版本只主攻**一个垂直能力柱**并完成实机验收，严格守住内存/句柄红线与前端类型检查/编译产物运行时门禁，避免同时摊开多条线导致功能半成品化。

---

## 一、架构约束与横向门禁（贯穿全部版本）

1. **进程隔离原则**：
   - `tomoyo.exe` 主进程只承担全局热键、原生检索、Everything IPC、剪贴板监听与轻量列表渲染，休眠态始终维持 **Working Set ≤ 2.0 MB、Private Commit ≤ 28.0 MB、100 轮压测 GDI/USER 句柄 `+0`**。
   - 重交互面板（设置中心、OCR 审校、翻译弹窗、备忘录编辑）统一托管在短生命周期 `tomoyo-webview-host.exe` 中，关闭即退出释放 100% WebView 内存。
   - 截图与桌面贴图（`tomoyo-capture.exe`）、离线 OCR（`tomoyo-plugin-ocr.exe`）等独立子系统以按需子进程运行，闲置时零常驻内存。
2. **可观测性、零阻断自更新与热键健康**：
   - 主进程（`tomoyo.log`）、WebView 宿主（`webview.log`）、插件子进程 `stderr` 与原生崩溃（`last_crash.txt`）统一归口至 `%LOCALAPPDATA%\Tomoyo\logs\`，带 `trace_id` 与北京时间耗时记录。
   - 自动更新严格执行 `ETag 304` + 无配额限制 GitHub Web 回退 + `SHA256SUMS.txt` 校验 + 安装自重启，杜绝 `HTTP 403` 与升级失联。
   - 所有全局热键（主唤起 / 剪贴板 / 翻译 / 截屏）均上报注册冲突状态并在首呼/设置中心提示，杜绝静默失效。
3. **代码、UI 与前端运行时四重门禁**：
   - 所有源码文件遵循 [`docs/CODING_STANDARDS.md`](CODING_STANDARDS.md)（单文件 ≤ 450 行、零禁用重依赖、零 Direct2D/WARP）。
   - 所有配置与交互控件遵循 [`docs/UI_DESIGN_STANDARDS.md`](UI_DESIGN_STANDARDS.md)（优先受控组件、双层校验、原生字体 `FW_NORMAL`）。
   - **WebView2 UI 运行时烟雾门禁**（`scripts/check_webview_ui.mjs`，已接入 `scripts/check_hygiene.ps1` 与 CI）：强制在真实 JS 引擎中组装并执行全部 WebView 页面脚本与交互点击，拦截 TDZ / `ReferenceError` / 事件监听缺失等前端静默瘫痪故障。

---

## 二、桌面工具整合全景蓝图

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│  v1.9.0 · 可组合轻量工作流 (`flow`) + 脱敏配置备份/一键换机迁移              │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.8.0 · 轻量备忘录 + 办公/ZIP 预览 + 可视化片段/收藏 + 可选高精度 OCR 插件  │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.7.0 · 专业级截图标注编辑器 + 纵/横向滚动长截图 + 小工具快捷键全量可配置   │
│          可编辑矢量对象/拖动/弯曲箭头/荧光笔/滚轮调大小 · 纯 Rust 图像拼接    │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.6.1 · [已完成] 全局热键秩序治理中心 (Hotkey Doctor) + F1 单键 + 中文清单  │
│          录制态临时 WH_KEYBOARD_LL 独占捕获 · 静态/抓现行真凶定位 · 中文插件  │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.6.0 · [已完成] 极简现代 UI/UX 全栈重构 + Svelte 5 前端工程化 + 热键闭环  │
│          Svelte 5 + Tailwind v4 · 10 类受控原语 · 实时热键冲突 · SDF 截图栏   │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.5.0 · [已完成] 替代 PixPin：按需原生截图、标注、取色与桌面贴图            │
│          独立进程 tomoyo-capture.exe + 设置中心运行时门禁 + 多热键冲突可观测   │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.4.x · [已完成] 替代 Pot：多引擎翻译 + 零 API 限额自更新 + 检索完整性治理  │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.3.0 · [已完成] 替代 CopyQ：增强剪贴板管理中心 + WIC 全图预览              │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.2.0 · [已完成] 全链路日志溯源基座 + OCR 根治与原图对照审校窗口            │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.1.0 · [已完成] 查询词绑定记忆 · 命中高亮 · 多音字/多词 · 跨源去重         │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## 三、分阶段垂直推进计划与验收标准

### 阶段一：`v1.1.0` — 检索引擎相关性与结果可解释性（已完成）
- 查询词绑定习惯记忆（`Query-Aware Frecency`）、标题命中区间高亮（`FW_NORMAL` 零伪粗体）、多音字与多词交集匹配、跨源规范化路径去重、分组筛选实时计数、零结果回退与 `ranking_golden.rs` 回归集。

---

### 阶段二：`v1.2.0` — 底层日志溯源基座与 OCR 深度修复（已完成）
**解决的核心问题**：在功能扩张前先把异常可定位能力打牢，并修复 OCR 粘贴路由死循环、小字识别率低与无法审校复制的体验短板。

1. **统一多进程日志与异常溯源（`tomoyo_core::diagnostics`）** — ✅ 已完成
   - 主进程升级为 1 MB × 3 轮转日志（`tomoyo.log` / `.1.log` / `.2.log`），记录插件生命周期、慢查询耗时（`≥ 800 ms`）、动作执行失败与单调递增 `trace_id`；
   - `PluginHost` 接管全部子进程 `stderr`（12 行环形缓冲 + 自动写入 `tomoyo.log`），插件启动/超时/崩溃时在结果列表返回可操作错误卡片（附 `trace_id` 与错误摘要）；
   - `tomoyo-webview-host` 接入 512 KB × 2 轮转日志（`webview.log`）、panic/SEH 崩溃捕获及前端 `window.onerror` / `unhandledrejection` 上报；
   - 修复设置滑块拖动触发全量热重载刷屏日志的问题（按需增量刷新）；
   - 新增 `:logs` 快捷指令与设置中心「诊断与日志」面板（支持切换主进程/WebView/崩溃快照、过滤 WARN/ERROR、一键复制脱敏诊断报告）。
2. **离线 OCR 路由修复、精度提升与审校交互（`tomoyo-plugin-ocr` + `__tomoyo_ocr_viewer__`）** — ✅ 已完成
   - 修复 `QueryRouter` 粘贴图片胶囊短路阻塞显式 `ocr` 关键词的死循环（新增单元测试回归）；
   - 引入 WinRT `BitmapTransform` 自适应 2× Cubic 放大（严格受 `OcrEngine::MaxImageDimension` 上限约束）、中文字符间多余空格清理以及数字/IP/时间全角标点规范化（`192 ． 168 ． 1 ． 7` → `192.168.1.7`），并支持 `ocr en` / `ocr zh` / `ocr raw` 与免 `Ctrl+V` 直接识别最近剪贴板截图；
   - 新增按需启动的「OCR 审校与编辑窗口」（左原图缩放对照 + 右可编辑文本区，支持复制全文、复制选中、合并单行、去空行、切换原始/清洗输出，按 `Esc` 关闭释放 100% 内存）及主列表单行直拷卡片。

---

### 阶段三：`v1.3.0` — 增强剪贴板管理中心（目标：替代 CopyQ，已完成）
**目标**：将现有基础 `cb` 历史升级为完整的生产力剪贴板工作台，满足日常高频检索、分类沉淀与隐私保护需求。

1. **核心能力建设** — ✅ 已完成：
   - **独立呼出与快捷操作**：新增剪贴板专属全局快捷键（默认 `Alt+V`，设置中心支持快捷键录入/预设/关闭），在 `cb` 列表支持 `Enter` 纯文本直贴、`Ctrl+Enter` 仅复制、`Shift+Enter` 合并单行纯文本贴回（图片为一键 OCR、文件为打开终端）、`Ctrl+E` 工作台、`Ctrl+D` 删除、`Ctrl+Shift+P` 置顶；
   - **多维筛选与分组**：SQLite v2 升级支持文本语义分类（`LINK` / `COLOR` / `PATH` / `CODE` / `TEXT`）、图片、复制文件（`CF_HDROP`）、来源应用记录（`source_app`）、自定义 `#标签` 与备注；支持 `cb @pin/@img/@text/@link/@file/@code/@color/#标签` 与拼音多词检索，以及 `cb @` / `cb :manage` / `cb :pause` / `cb :clear`；
   - **按需剪贴板工作台（`__tomoyo_clipboard_manager__`）**：独立按需三栏窗口（左类型/标签筛选 + 中搜索列表与图片缩略图 + 右文本/备注/标签编辑），支持清空非置顶、JSON 备份导出与导入、主窗口实时同步，关闭释放 100% WebView 内存；
   - **Win32 WIC 全格式图片预览**：`tomoyo-core::wic_thumb` 支持 `PNG / JPG / WebP / BMP / GIF / ICO / TIFF` 有界缩略图预览（休眠自动释放）；
   - **双层隐私保护**：自动识别 Windows 机密剪贴板标记（`ExcludeClipboardContentFromMonitorProcessing` / `CanIncludeInClipboardHistory == 0`），并在设置中心通过 **下拉搜索框（Searchable Combobox）** 管理隐私屏蔽 `.exe` 列表（默认涵盖 KeePass / KeePassXC / Bitwarden / 1Password / Enpass）。
2. **可卸载 CopyQ 的验收门槛** — ✅ 单测与门禁已验证：
   - 1000+ 条记录实测检索远低于 `15 ms`，休眠内存仍严守 `WS ≤ 2.0 MB / Private ≤ 28.0 MB`、100 轮压测零句柄增长。

---

### 阶段四：`v1.4.0` — 多引擎翻译与 OCR 联动（目标：替代 Pot，已完成）
**目标**：构建低延迟、无常驻进程开销的划词翻译、输入翻译与截图/OCR 翻译闭环，并同步完成检索完整性、右键管理员运行与北京时间日志治理。

1. **核心能力建设** — ✅ 已完成：
   - **三大触发入口**：① 全局划词翻译快捷键（默认 `Alt+T`，后台 `SendInput` 采样选中文本并无损恢复剪贴板原文本，且通过原子抑制标志避免污染剪贴板历史）；② 启动器内 `tr` / `fy` 快捷翻译（支持 `tr en/zh/ja/ko/fr/de <文本>` 及本地生词本/历史瞬时命中）；③ OCR 审校窗口与剪贴板工作台内「一键翻译」联动；
   - **多源并行 WinHTTP 翻译与词典**：基于原生 WinHTTP 并行查询 Google 翻译、MyMemory + 有道英汉音标词典（免密钥可用）、DeepL Free/Pro、OpenAI 兼容接口（OpenAI / DeepSeek / 通义等）及本地离线 Ollama；
   - **Windows DPAPI 密钥隔离**：API Key 经 Windows 原生 `CryptProtectData` 加密写入 `%LOCALAPPDATA%\Tomoyo\secrets.json`，不写入明文 `config.toml` 或日志，设置中心新增「翻译引擎」面板（热键录入、引擎开关、只写密码框与加密状态徽标）；
   - **按需双语对照浮窗（`__tomoyo_translator__`）**：支持 `Ctrl+Enter` 翻译、`Ctrl+Shift+T` 置顶、复制译文、朗读、生词本与翻译历史抽屉，关闭即释放 100% WebView 内存；
   - **体验补强与根因治理**：搜索结果应用与可执行文件右键及 `Ctrl+Shift+Enter` 支持「以管理员身份运行」（UAC `runas`）；全部日志时间统一为北京时间（`UTC+08:00`）并修复日志统计计数；根治应用/桌面搜索不全问题（词边界噪声过滤保活 `HiBit Uninstaller` 等工具、桌面/PecoFence/DeskBox 零过滤并覆盖顶层文件夹与 `.bat/.cmd/.url/.appref-ms/.msc`、Windows `Uninstall` 注册表兜底以及剔除散乱拼音子序列误匹配）。
2. **可卸载 Pot 的验收门槛** — ✅ 实机与单测已验证：
   - 实机 WinHTTP 实测 Google + MyMemory/有道词典双引擎并行返回译文、音标与中文释义，剪贴板现场完整还原且零历史污染；
   - 闲置时零翻译进程常驻，主进程内存与 100 轮句柄门禁全绿。

---

### 阶段五：`v1.5.0` — 原生低占用截图与桌面贴图（目标：替代 PixPin，已完成）
**目标**：以独立按需原生进程 `tomoyo-capture.exe` 提供专业级截屏、标注、取色与桌面贴图能力，同时完全不污染主进程内存，并同步根治设置中心前端初始化顺序缺陷与热键冲突可观测性。

1. **核心能力建设** — ✅ 已完成：
   - **前端运行时根治与自动门禁**：抽离 `settings_boot.js` 置顶初始化共享状态，根治 `translate_controls.js` 在 `settings.js` 之前引用 `INIT`/`state` 触发的 ES6 TDZ `ReferenceError` 导致设置中心点击瘫痪；新增零依赖自动化烟雾门禁 [`scripts/check_webview_ui.mjs`](../scripts/check_webview_ui.mjs) 接入 `check_hygiene.ps1` 与 CI，强制验证全部 4 个 WebView 页面启动、10 个设置标签点击切换与 IPC 通信零异常；
   - **多屏高 DPI 精准截屏与窗口吸附（`tomoyo-capture.exe`）**：Per-Monitor V2 虚拟桌面 `BitBlt` 帧冻结 + `DWMWA_EXTENDED_FRAME_BOUNDS` 智能窗口边界吸附 + `11×11` 像素级放大镜（按 `C` 一键复制 `#HEX` 色值，可选大小写）；
   - **纯 Rust 32-bit BGRA 标注引擎**：支持矩形（`1`/`R`）、椭圆（`2`/`E`）、箭头（`3`/`A`）、画笔（`4`/`B`）、马赛克（`5`/`M`）、序号标号（`6`/`N`）、颜色预设与 `Ctrl+Z` / `Ctrl+Y` 撤销/重做栈；
   - **桌面贴图（Pin to Desktop）与流程打通**：无边框置顶浮窗，支持鼠标拖拽、滚轮缩放（25%–400%）、`Ctrl+滚轮` 实时透明度（20%–100%）、双击/`Ctrl+C` 复制、`Ctrl+S` 保存 PNG 与右键菜单（复制/保存/OCR/翻译/置顶/还原/关闭）；截图动作条直连 `复制` / `保存` / `贴图 (P)` / `OCR 识字 (O)` / `截图翻译 (T)`；
   - **启动器、设置中心与多热键冲突联动**：新增全局截图热键（默认 `Alt+S`）、`CaptureProvider`（`cap`/`shot`/`jt`/`pin`/`jtocr`/`jtfy` 与中文/拼音命中）、托盘入口、设置中心「截图与贴图」面板，并升级全局热键注册器以统一汇报主/剪贴板/翻译/截屏任意冲突。
2. **可卸载 PixPin 的验收门槛** — ✅ 单测、门禁与实机自检已验证：
   - 独立进程 `tomoyo-capture.exe --self-test` 实机验证虚拟桌面捕获、6 种标注光栅化、PNG 往返及 GDI/USER `+0` 句柄泄露；无贴图时截屏子进程完全退出（0 MB 常驻）；
   - 主进程 `--self-test --strict-budget` 严守 `WS ≤ 2.0 MB / Private ≤ 28.0 MB` 与 100 轮零句柄增长。

---

### 阶段六：`v1.6.0` — 极简现代 UI/UX 全栈重构、Svelte 5 前端工程化与全局热键健康闭环（已完成）
**目标**：根治手写 DOM 拼接导致的前端样式失控与组合快捷键冲突静默失效，以极简现代风格（Linear × Raycast）统一全部 WebView 工作台与原生桌面悬浮 UI，同时严守 0 MB WebView 常驻与零重依赖红线。

1. **核心能力建设** — ✅ 已完成：
   - **全局热键真实健康闭环与键位扩展**：`tomoyo-core::config::hotkey` 扩展支持 OEM 标点键与导航键并持久化 `%LOCALAPPDATA%\Tomoyo\hotkey_status.json`；`tomoyo-app` 在启动与热重载时落盘全部 4 个全局热键的 Win32 `RegisterHotKey` 真实结果；`tomoyo-webview-host` 通过 `CheckHotkeyHealth` 异步回推状态，`HotkeyRecorder` 控件实时呈现绿/红注册徽标与冲突替代建议，彻底根治「改了组合键用不了却毫无反馈」的问题。
   - **Svelte 5 + Tailwind CSS v4 + TypeScript 工程化前端（`frontend/`）**：以双层渲染策略保留纯 Rust 原生主进程与按需 0 MB WebView 宿主，构建 10 套极简现代受控原语（`Input` 含密码显隐、`Select` 自定义浮层、`Combobox` 下拉查询、`PathPicker` 路径浏览框、`Button` 5 级矩阵防折行、`Switch`、`Segmented`、`Slider`、`HotkeyRecorder`、`ContextMenu`），完整重写设置中心（10 个独立页签组件）、多源翻译双语工作台、三栏剪贴板工作台与 OCR 原图对照审校窗，经 `Vite + vite-plugin-singlefile` 产出 4 份零 CORS 经典 IIFE 单文件 HTML 嵌入宿主；`svelte-check` + `Vitest` + `happy-dom` 编译产物门禁全接入 CI。
   - **原生截屏工具栏与右键菜单视觉统一**：`tomoyo-capture.exe` 引入纯 Rust 抗锯齿 SDF 图形库（`overlay_icons.rs`），重构为深色半透明圆角胶囊矢量图标工具栏、标注工具联动二级属性胶囊（3 档线宽点 + 6 色圆形色板）、悬浮 Tooltip 气泡、圆角尺寸徽标与精细化 `11×11` 放大镜 HUD（支持 `Shift` 切换 HEX/RGB 与复制）；`tomoyo-app` 原生弹出菜单统一 `32px` 极简无框悬停高亮与 `danger` / `disabled` 状态规范。
2. **验收门槛** — ✅ 实机、单测与门禁全绿：
   - `svelte-check` 0 errors/warnings、`Vitest` 全部通过、`scripts/check_hygiene.ps1` 全仓源码 ≤ 450 行与 4 份 Svelte 5 单文件产物 `happy-dom` 运行时验证全绿；
   - `tomoyo.exe --self-test --strict-budget`、`tomoyo-capture.exe --self-test` 及真实 Edge WebView2 `NavigateToString` 交互启动 0 JS 报错。

---

### 阶段六（补强）：`v1.6.1` — 全局热键秩序治理中心、`F1` 单键热键与插件清单中文化（已完成）
**目标**：根治设置中心录入快捷键反被自己或第三方软件截胡唤起的系统级顽疾，治理 Windows 桌面全局热键“冲突不知真凶”黑盒乱象，放行 `F1`–`F12`/`PrintScreen` 单键绑定，并将插件清单与运行时标签全面中文化。

1. **核心能力建设** — ✅ 已完成：
   - **录制态独占捕获（零常驻钩子）**：`HotkeyRecorder` 进入录制态时自动通知 `tomoyo.exe` 暂停自身热键并由 `tomoyo-webview-host.exe` 临时挂载 `WH_KEYBOARD_LL` 独占捕获（完成/失焦/`Esc` 立即卸载），早于内核 `RegisterHotKey` 吞掉按键，彻底杜绝录入时误唤起任何本机工具；
   - **全局热键治理中心（Hotkey Doctor）**：设置中心新增「热键治理」独立页，支持 22+ 组高频系统热键无副作用矩阵探测、活跃进程+本地配置静态归因、交互式「抓现行」真凶定位（捕获响应窗口 PID/标题/路径并直达所在目录）以及冲突热键一键切换至实测空闲键；
   - **功能键单键绑定与插件清单全中文化**：前后端放行 `F1`–`F12` 与 `PrintScreen` 单键绑定并加入截图预设；`registry/index.json`、5 个随包 `tomoyo-plugin.toml` 与 UI 运行时/权限标签全部中文化并刷新 SHA-256 校验摘要。

---

### 阶段七：`v1.7.0` — 专业级截图标注画布、纵/横向滚动长截图与工具快捷键可配置
**目标**：将 `tomoyo-capture.exe` 打磨至真正可彻底替代 PixPin / Snipaste，同时保持纯 Rust 零 GPU 膨胀与 0 MB 常驻内存。

1. **可编辑矢量标注引擎**：对象命中选中/整体拖动/控制点重调/`Delete` 删除、三点贝塞尔弯曲箭头（拖动中段转弯）、空心/填充/圆角矩形与椭圆、平滑抗锯齿画笔与半透明荧光笔、马赛克/模糊双模式、文字批注、`Shift` 正交约束，以及鼠标滚轮实时调节大小+数值 HUD；
2. **纵向 / 横向滚动长截图（纯 Rust 图像拼接）**：支持纵向/横向滚动双模式、手动滚动捕获与自动匀速滚轮推进、实时预览拼接条与多尺度 SAD 模板匹配无痕拼接（严格有界尺寸上限）；
3. **截图内小工具快捷键全量可配置**：`config.toml` 新增 `[capture.tool_hotkeys]` 并在设置中心「截图与贴图」提供可视化映射与一键还原默认，截图 Tooltip 实时同步最新自定义键位。

---

### 阶段八：`v1.8.0` — 本地轻量备忘录、办公文件预览与可选高精度 OCR
**目标**：补齐日常碎片记录与高频文件预览最后一公里，保持数据开放、本地可控。

1. **轻量本地备忘录（`note`）**：
   - 纯本地 Markdown 文件存储（目录可自定义，无私有格式锁定，方便 Git/网盘同步），启动器内 `note <关键词>` 即时全文/标题/标签检索；
   - 回车快速复制或打开按需 Markdown 编辑/预览窗（复用 `frontend/` 极简现代组件库），支持与剪贴板/OCR 结果一键转存为备忘录。
2. **办公与媒体文件预览补齐 + 设置中心收尾**：
   - 主窗口右侧预览扩展 Win32 WIC 全格式图片、`.zip` 文件列表、`.docx`/`.xlsx`/`.pptx` 文本摘要与 `.pdf` 基础信息（严格有界 I/O）；
   - 设置中心补齐 `snippets.toml`、Everything 收藏规则可视化配置及配置备份/导入（经 `sanitize()` 校验）。
3. **可选高精度离线 OCR 扩展包**：
   - 为复杂多列英文报表、公式与竖排排版提供按需下载的本地高精度 OCR 插件（如 PaddleOCR/ONNX 独立子进程，受 Job Object 内存上限与空闲回收约束），与系统零体积 WinRT OCR 互补。

---

### 阶段八：`v1.8.0` — 可组合轻量工作流与脱敏配置备份迁移
**目标**：将已沉淀的独立能力柱串联为可复用个人工作流，并实现零痛点换机迁移。

1. **轻量声明式工作流（`flow`）**：
   - 在 `config.toml` 支持线性动作链定义（如 `截屏 → OCR → 翻译 → 复制`、`剪贴板文本 → JSON 格式化 → 贴回`），零脚本解释器常驻开销。
2. **加密/脱敏配置备份与一键迁移**：
   - 设置中心支持导出 `config.toml`、`snippets.toml`、`frecency.json` 与翻译生词本为单文件备份包（显式剥离本机 DPAPI 绑定密钥），导入时统一经过 `TomoyoConfig::sanitize()` 校验防护。

---

## 四、每阶段固定交付节奏

每个里程碑严格执行以下五步闭环，缺一不可：
1. **需求边界与接口设计**：明确数据模型、进程归属与可卸载原工具的量化验收项；
2. **规范与单测门禁**：`pwsh -File scripts/check_hygiene.ps1` + `RUSTFLAGS="-D warnings" cargo test --workspace`；
3. **性能与资源预算门禁**：`tomoyo.exe --self-test --strict-budget`（WS ≤ 2.0 MB、Private ≤ 28.0 MB、100 轮压测零句柄增长）；
4. **实机交互验收**：在 `WinSta0\Default` 桌面实测核心路径、边界输入与日志溯源；
5. **版本发布与本机升级**：GitHub Actions CI + Release 全绿，SHA-256 校验安装包并完成本机静默覆盖升级与启动确认。
