# Tomoyo 长期演进蓝图与分阶段替代计划 (Long-Term Roadmap)

> **定位与基线**：`v2.0.0` 正式推出 **Tomoyo Open Plugin Protocol v2**（100% 向下兼容 v1 插件），首次赋予外部隔离插件「声明式全局热键 (`[[hotkeys]]`)」「权限门控内核事件订阅 (`events` + `tomoyo/event`)」与「声明式强类型设置 Schema (`[[settings]]`)」三大一等公民能力：外部插件声明的热键由 `tomoyo.exe` 微内核统一治理并自动继承 Win32 冲突高优先级接管与全屏/远程桌面热键避让；设置中心「插件管理」自动为任意 Protocol v2 插件生成可折叠的参数与热键抽屉；两大官方独立按需插件 `dev.tomoyo.notes`（本地 Markdown 备忘录）与 `dev.tomoyo.ocr`（离线 WinRT OCR）已率先升级 Protocol v2 并通过实机验证。  
> **产品目标**：`tomoyo.exe` 始终坚守极简、低占用、毫秒唤起的 Windows 原生启动器核心；剪贴板、划词翻译、截图贴图、离线 OCR、本地备忘录等高频效率工具一律以**按需启停/安装的模块化官方插件**形态提供（本机实测已顺利替代并卸载 Pot 与 PixPin）。  
> **执行纪律**：每个版本坚持小步快跑与实机验收，严守内存/句柄硬红线与全仓单文件 ≤ 450 行、零警告、前端编译产物运行时门禁。

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
│  v2.1.0 · 声明式轻量工作流 (`flow`) + `tomoyo-cli dev` + 社区清单自动校验     │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.0.0 · [已完成] 插件协议 v2（声明式热键/事件订阅/设置 Schema + 旗舰插件） │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.9.1 · [已完成] 全屏/远程桌面热键避让 + 免账号本地同步目录 + 文件动作流    │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.9.0 · [已完成] 剪贴板冲突自动无损唤起 + 三阶段差分热键治理 + 纯插件备忘录 │
│          零核心膨胀本地 Markdown 插件 · Office/ZIP/PDF 预览 · 脱敏换机备份   │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.8.0 · [已完成] 微内核插件化开关 + 可扩展搜索标签 + 零折角平滑弯箭头      │
│          官方模块按需启停 · 长截图白底校准与实时预览条 · 点外部确定性收起    │
├──────────────────────────────────────────────────────────────────────────────┤
│  v1.7.x · [已完成] 极简直控标注（8手柄/现代渐粗箭头/等宽椭圆）+ 热键强行接管 │
│          零样式按钮膨胀 · 固定侧栏自动屏蔽拼接 · 双击关贴图 · 底部一键定位   │
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

### 阶段七：`v1.7.0` – `v1.7.2` — 极简直控截图标注、数学级抗锯齿曲线与热键冲突闭环治理（已完成）
**目标**：将 `tomoyo-capture.exe` 打磨至真正「极简但好用」可替代 PixPin / Snipaste，并彻底打通热键冲突定位与强行接管闭环，同时保持纯 Rust 零 GPU 膨胀与 0 MB 常驻内存。

1. **极简直控矢量标注引擎与精确解析 SDF 图形** — ✅ 已完成：任意工具状态下悬停已有标注即亮起控制手柄并切换对应调整/移动光标（无需切回选择工具即可随时拖动/8 手柄缩放任意矩形、椭圆、马赛克或箭头）；统一采用**现代渐粗流线型箭头**（尾细头宽 + 无缝 4 顶点凹口箭翼，修复短拖断言崩溃，拖动中段橙色控制点自由弯折、双击拉直）；椭圆采用梯度归一化精确欧氏距离 SDF（宽扁椭圆线宽处处均匀不截断）；保留矩形/椭圆单开关「填充」、平滑画笔/荧光笔、马赛克/模糊、序号标号、中文 IME 批注与鼠标滚轮实时调节大小；
2. **智能固定栏屏蔽纵/横向长截图（`stitch.rs` + `scroll_capture.rs`）** — ✅ 已完成：自动学习帧间动态滚动区域（屏蔽固定左侧导航栏、顶部标题栏、底部状态栏与滚动条）、底部高纹理模板锚定 + 全重叠区间像素验证（杜绝卡片重复拼接）、侧栏纯色背景无缝延展、平滑滚动静止等待与 `WDA_EXCLUDEFROMCAPTURE` HUD 防入镜；
3. **贴图双击关闭与 12 项小工具快捷键配置** — ✅ 已完成：修复贴图窗因 `HTCAPTION` 触发 `WM_NCLBUTTONDBLCLK` 导致双击无法关闭的问题（双击/中键/`Esc` 均直接关闭，超大长截图自动适配屏幕 82% 视野）；支持 12 项截图小工具快捷键自定义配置；
4. **热键冲突可点击定位、实时刷新与高优先级强行接管** — ✅ 已完成：设置中心底部冲突栏升级为可点击按钮（直显冲突功能与键位并一键跳至热键治理页）+ 侧栏红点计数；修正 PixPin 误归因并在窗口回切激活与扫描完成后实时刷新 `hotkey_status.json`；「抓现行」新增 `GetProcessTimes` 后台 CPU 唤醒监测；新增按需可选项「高优先级强行接管冲突热键」（仅对注册失败的冲突键挂载极轻量 `WH_KEYBOARD_LL`，零额外线程，关闭或无冲突时零挂载）。

---

### 阶段八：`v1.8.0` — 坚守微内核初衷（官方模块插件化）、可扩展搜索标签与丝滑交互精修（已完成）
**目标**：确立「纯启动器微内核 + 官方功能按需插件化」长期边界，同步彻底解决紧弯箭头多段折线感、浅色界面长截图与实时预览缺失、点击外部偶发不收起与搜索栏标签不可扩展等一线痛点。

1. **官方功能插件化启停与微内核坚守** — ✅ 已完成：
   - 设置中心「插件管理」提供 4 大官方功能模块（`dev.tomoyo.clipboard` 剪贴板、`dev.tomoyo.translate` 划词翻译、`dev.tomoyo.capture` 截图与贴图、`dev.tomoyo.ocr` 离线 OCR）及所有已安装外部插件的一键启停开关（持久化至 `disabled_plugins`）；
   - 关闭任意模块后立即动态卸载对应系统监听（`RemoveClipboardFormatListener`）、释放全局热键、取消 `cb`/`tr`/`fy`/`cap`/`pin` 指令劫持、隐藏托盘入口与对应设置侧栏页签，并过滤热键探针中的 `System (PID 4)` 等内核系统进程误报。
2. **用户可扩展搜索结果分类标签（`CustomFilterTab`）** — ✅ 已完成：
   - 支持在设置中心「应用、文件与标签」中隐藏任意不需要的内置标签（`全部` 常驻），一键添加常用预设（`代码`/`视频`/`音频`/`Markdown`/`安装包`），或按扩展名（如 `psd;fig`）与 Everything 语法（如 `dm:today`）自由新增最多 12 个自定义标签；
   - 主搜索栏动态渲染、实时计数，并原生支持点击切换、`Ctrl+Tab` 循环、`Ctrl+1..9` 直达及 `@标签名` 行内过滤与 Everything 下推。
3. **零折角解析三次方程贝塞尔弯折箭头（`closest_quad_bezier`）** — ✅ 已完成：
   - 彻底抛弃 20 段折线近似，在 `annotate_render.rs` 中引入 Inigo Quilez 解析 Cardano 三次方程求解逐像素到二次贝塞尔曲线精确距离与弧长参数 $t$，180° 急弯亦达到无限分辨率丝滑曲线、零折角零接缝。
4. **白底高适应长截图与侧边实时可视化预览条（`scroll_preview.rs`）** — ✅ 已完成：
   - 重校准 `stitch.rs` 适应 95% 留白浅色桌面界面（相对 SAD 比例门限 + 稀疏行变化判定 + 单测验证），新增停靠于选区旁侧的 `216px` 实时预览浮窗（`WDA_EXCLUDEFROMCAPTURE` + `WS_EX_NOACTIVATE` 不抢滚动焦点），2×2 抗锯齿缩略图随滚动实时增长并展示进度条与已拼接帧数/总尺寸。
5. **100% 确定性点击外部自动收起搜索栏** — ✅ 已完成：
   - 消除 `suppress_next_deactivate` 粘滞泄露，换用作用域 `in_modal_overlay` 标记；`show_launcher` 新增 `AttachThreadInput` 突破 Windows 前台锁；补齐 `WM_ACTIVATEAPP(0)` 与仅在窗口显示时运行（隐藏即停、0 后台 CPU）的 `160ms` 前台归属守卫定时器。

---

### 阶段九：`v1.9.0` — 剪贴板冲突唤起根治、基线差分热键治理、纯外部备忘录插件与办公预览（已完成）
**目标**：彻底根治被外部软件静默占用的剪贴板热键唤起失败顽疾，将热键治理升级为零误报科学探测工具，并以纯独立外部插件交付本地 Markdown 备忘录，补齐办公/压缩预览与脱敏换机备份。

1. **剪贴板冲突唤起根治（`hotkey_override.rs` + `window/mod.rs` + `clipboard_ctrl.rs`）** — ✅ 已完成：
   - 冲突自动高优先级接管默认开启（无冲突时零钩子零开销，仅对 `RegisterHotKey` 失败的键挂载零线程 `WH_KEYBOARD_LL`）；
   - 接入 `LLKHF_ALTDOWN` 事件内修饰键位、`#MenuMaskKey (0xE8)` 阻止裸 `Alt` 释放激活原窗口菜单栏抢焦点、`kb.time` 防重入死锁、三阶段 Win32 前台锁突破、`420ms` 激活宽限期与单次查询无闪烁 `show_launcher_with_query(Some("cb "))`（同时支持 `cb` / `clip` 前缀）。
2. **三阶段基线校准差分热键治理中心（`hotkey_doctor.rs` + `hotkey_signatures.rs` + `HotkeysTab.svelte`）** — ✅ 已完成：
   - 探针先采样 `300ms` 静息基线 CPU（自动剔除 Wallpaper Engine、IDE 服务等持续活跃进程误报）再执行触发窗口、新弹窗与剪贴板序列号监测，并对后台瞬峰追加 `220ms` 静默回落复核；
   - 扩展至 34+ 常见桌面软件签名与 7 类本地配置解析（含 Clash Verge / Everything / RayLink），新增 KPI 摘要栏、任意自定义快捷键探测框与冲突行直达「一键接管」按钮。
3. **纯独立外部插件 · 本地 Markdown 备忘录（`plugins/official-notes`，`dev.tomoyo.notes`）** — ✅ 已完成：
   - 独立 `0.32 MB` Rust STDIO 进程（0 MB 闲置常驻、不膨胀主进程），收录至 `registry/index.json`（SHA-256 校验），支持 `note`/`memo`/`bw` 全文与 `#标签` 检索、`note + 标题 :: 内容` 一键新建、`note clip` 剪贴板转存与外部编辑器直达。
4. **核心有界文档预览与脱敏换机迁移（`preview_docs.rs` + `backup.rs`）** — ✅ 已完成：
   - 零新依赖实现 `.zip`（仅读尾部中央目录）、`.docx`、`.xlsx`、`.pptx`（单条目防炸弹 `≤ 96 KB` 解压）与 `.pdf`（版本/页数/元信息）有界预览；
   - 设置中心「常规设置」新增脱敏单文件 JSON 备份导出与校验恢复（自动剥离本机 DPAPI API 密钥并经 `sanitize()` 校验）。

---

### 阶段十：`v1.9.1` — 实机零打扰稳态精修（热键全屏避让 + 本地同步目录 + 文件二级动作流 · 已完成）
**目标**：围绕真实高频桌面场景打磨无摩擦日常体验，零新增重依赖、零后台轮询线程。

1. **全屏游戏 / 远程桌面热键智能避让（`hotkey_avoid.rs`）**：
   - 仅在按键触发瞬间执行 `< 0.05 ms` 零轮询检查：检测全屏独占/无边框全屏或前台命中避让名单（默认覆盖 `raylink.exe` / `mstsc.exe` / `parsec.exe` / `moonlight.exe` / `todesk.exe` / `sunloginclient.exe` / `anydesk.exe`）时自动放行按键，不抢游戏与远程桌面热键；
   - 设置中心「热键治理」提供「全屏自动避让」开关与下拉搜索式避让进程名单。
2. **本地优先免账号同步目录（`sync_dir`）**：
   - 在设置中心「常规设置」选择任意本地同步盘目录（OneDrive / 坚果云 / iCloud / Git），托管 `config.toml`、`snippets.toml` 与 `notes/` 备忘录；本机 DPAPI `secrets.json`、剪贴板库与运行日志严格隔离留在 `%LOCALAPPDATA%\Tomoyo`，不上传敏感密钥。
3. **上下文文件二级动作流（File Action Flow）**：
   - Everything 搜到图片（`.png/.jpg/.webp/.bmp/.gif`）→ `Tab` 菜单直出「贴图到桌面 (`tomoyo-capture --pin`)」「离线 OCR 识字」「复制 Base64」；
   - 搜到 `.zip` 压缩包 → `Tab` 直出「安全解压到同名目录（纯 Rust 防 Zip-Slip）」；
   - 搜到文本/代码/Markdown/JSON → `Tab` 直出「复制全文（≤ 128 KB）」「计算并复制 SHA-256」。

---

### 阶段十一：`v2.0.0` — 插件协议 v2 与声明式微内核扩展体系（已完成 ✅）
1. **`tomoyo-protocol` v2 + `tomoyo-sdk`（100% 向下兼容 v1 插件）**：
   - `tomoyo-plugin.toml` 支持声明 `[[hotkeys]]`、`events = ["clipboard_text" | "clipboard_image" | "hotkey_triggered"]` 与强类型 `[[settings]]`（`text` / `bool` / `number` / `select`）；
   - `QueryContext::plugin_settings` 自动合并清单默认值与用户 `config.toml` 实时覆写，`tomoyo/event` 仅在启用且满足权限（如 `Permission::Clipboard`）时非阻塞投递，零订阅时零开销；
   - `tomoyo-plugin-host` 握手兼容 `protocol_version 1..=2`，确保全部现有 Python/Rust v1 插件零改动运行。
2. **微内核统一热键治理与设置中心自动渲染**：
   - `tomoyo.exe` 统一注册与热重载最多 16 组插件全局热键（ID `100..116`），底层冲突接管槽位扩至 12 路并原生继承全屏/远程桌面避让；
   - 设置中心新增 `PluginConfigDrawer.svelte`，自动基于插件 `settings_schema` 与 `declared_hotkeys` 渲染受控组件抽屉并实时热生效；
   - 官方独立按需插件 `dev.tomoyo.notes`（本地 Markdown 备忘录）与 `dev.tomoyo.ocr`（离线 WinRT OCR）率先完成 Protocol v2 升级。

### 阶段十二：`v2.1.0` — 声明式轻量管道、深度拆包与插件 DX
- 声明式动作流 `flow`（零脚本解释器，如 `截屏 → OCR → 翻译 → 复制`）；
- `tomoyo-cli dev` 热重载与社区插件 PR 自动校验；
- 持续按需精简核心体积并推进跨平台真机窗口壳验证。

---

## 负面清单（Anti-Roadmap · 永久架构红线）
1. **永不做账号登录、强制云端绑定与任何遥测上报**；
2. **永不引入常驻 WebView2 / Electron / Chromium / `tokio` / `d3d10warp`**，休眠态始终锁死 `WS ≤ 2.0 MB` 与 100 轮 `+0` 句柄泄露；
3. **永不把低频重型应用塞进核心进程**，扩展能力一律由按需隔离外部插件承接；
4. **永不靠堆砌冗长开关掩盖交互缺陷**，始终以直觉操控与聪明默认值为先。

---

## 四、每阶段固定交付节奏

每个里程碑严格执行以下五步闭环，缺一不可：
1. **需求边界与接口设计**：明确数据模型、进程归属与可卸载原工具的量化验收项；
2. **规范与单测门禁**：`pwsh -File scripts/check_hygiene.ps1` + `RUSTFLAGS="-D warnings" cargo test --workspace`；
3. **性能与资源预算门禁**：`tomoyo.exe --self-test --strict-budget`（WS ≤ 2.0 MB、Private ≤ 28.0 MB、100 轮压测零句柄增长）；
4. **实机交互验收**：在 `WinSta0\Default` 桌面实测核心路径、边界输入与日志溯源；
5. **版本发布与本机升级**：GitHub Actions CI + Release 全绿，SHA-256 校验安装包并完成本机静默覆盖升级与启动确认。
