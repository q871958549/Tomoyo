# Tomoyo 长期演进蓝图与分阶段替代计划 (Long-Term Roadmap)

> **定位与基线**：`v2.2.3` 深度打磨日常高频交互体验：彻底根治搜索框按 `Esc` / `Enter` / `Tab` / `Alt+1..8` 触发 Windows 单行 `EDIT` 系统提示音的顽疾并升级为静默两段式 `Esc`（有词先清词、无词再关窗）；将「全局一键贴图」升级为独立一级全局热键槽位（默认 `F3`，支持 `F1–F12` 单键录入、冲突接管与全屏避让），并在设置中心清晰区分全局热键与画布内小工具键；同时重构 `--pin-clipboard` 为**全格式万能贴图引擎**（支持 `PNG`/`CF_DIB` 透明图混合、复制图片文件/路径列表、`#HEX`/`rgb()` 色值卡及跟随主题的 ClearType 文本/代码便签卡，支持 `Ctrl+C` 还原原文与 `L` 直达翻译）。  
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
│  v3.0.0 · 远期：物理拆包 Core/Suite 双版本 + WASM wasi-p2 + 公益代码签名     │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.8.0 · 规划：社区脚本一键打包发布流水线 + WASM 声明式富卡片扩展           │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.7.0 · 规划：万级插件中心 2.0（分片索引 / Rust 侧检索分类分页 / 详情抽屉） │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.6.1 · [已完成 ✅] Windows 原生录屏 · 卸载防重 · Everything UIPI/截图修复 │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.6.0 · [已完成 ✅] 备忘录与 Quiet.md 分离 · 插件专属 Logo · 内嵌/独立双模态   │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.5.1 · [已完成 ✅] 补充 UI 设计规范 §1.3/§2.8 并重构分类栏列表与 FontSelect│
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.5.0 · [已完成 ✅] 分类栏多行/+N溢出支持 32 标签 + 结果字体/字重精细可配置 │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.4.1 · [已完成 ✅] Everything 降噪熔断 + 设置无缝过渡 + 级联尝试右键菜单  │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.4.0 · [已完成 ✅] 贴图越界自由拖拽 + 长截图动态去黑区 + 设置 12→6 栏插件化  │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.3.1 · [已完成 ✅] Windows 一键校验升级 Job Object 逃逸与架构精准匹配修复 │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.3.0 · [Linux 主端落地 ✅] CachyOS/Arch KDE Wayland/X11 全套原生支持     │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.2.6 · [已完成 ✅] 热键治理 v2 + 双路高精度离线 OCR + 截图 Shift+C 取字  │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.2.5 · [已完成] 根治热键接管刷屏 WARN/钩子抖动与 WebView2 早关句柄竞态 │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.2.4 · [已完成] 统一设置中心标题栏/任务栏/侧边栏/关于页与系统托盘应用 Logo│
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.2.3 · [已完成] 静默两段式 Esc + 全局 F3 一键万能贴图（图片/文件/色值/文本）│
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.2.2 · [已完成] 双击启动 0ms 瞬时反馈 + 第二梯队拆解 + 双向 IPC 1:1 契约│
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.2.1 · [已完成] 7道防膨胀门禁 + Clippy全绿 + HTML Gzip瘦身 + 热点拆解    │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.2.0 · [已完成] 工作流全局热键 + 本地 AI 管道 + ARM64 原生构建 + SLSA溯源 │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.1.0 · [已完成] 声明式轻量工作流 (`flow`) + `tomoyo-cli dev` + 清单校验   │
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

### 阶段十二：`v2.1.0` — 声明式轻量工作流 (`flow`) 与开放插件 DX（已完成 ✅）
1. **声明式轻量动作管道（`flows.rs` + `FlowsProvider` + `flow_exec.rs` + `FlowsTab.svelte`）**：
   - 零脚本运行时、纯 Rust 有界多步动作管道（上限 32 条工作流 × 12 步，涵盖剪贴板/截图/OCR/翻译/本地备忘/文本清洗/URL 编码/模板变量/打开网页/预填指令 19 类受控原语），存储于 `<sync_root>/flows.toml` 并自动随本地同步目录与脱敏备份迁移；
   - 默认预置 5 条高频开箱工作流（`shottr` 截图识字翻译、`cleanpaste` 剪贴板去换行直贴、`cliptr` 剪贴板翻译、`clipnote` 剪贴板转存 Markdown 备忘、`clipweb` 网页速查），设置中心「轻量工作流」提供全受控可视化编排与即时热重载。
2. **插件开发者体验与 CI 自动化防漂移（`tomoyo-cli dev` + `registry check`）**：
   - `tomoyo-cli dev <plugin-dir>` 自动监听变更、校验 Protocol v2 清单与实时查询耗时，并通过 Win32 消息热通知运行中的 `tomoyo.exe` 重载插件；
   - `tomoyo-cli registry check` 自动核验清单 Schema、ID/版本一致性与全量 SHA-256，并已接入 GitHub Actions CI 流水线。

### 阶段十三：`v2.2.0` — 稳态沉淀、工作流全局热键、本地 AI 管道与 ARM64 / SLSA 信任底座（已完成 ✅）
1. **工作流全局热键静默直达（`WorkflowDef::hotkey` + `FLOW_HOTKEY_BASE = 200`）**：
   - 任意工作流可在设置中心「轻量工作流」内一键录制绑定全局热键，由 `tomoyo.exe` 统一治理并原生继承全屏/远程桌面避让与冲突接管；按下即直接作用于当前前台窗口（例如 `Alt+Shift+V` 原地清洗剪贴板断行并直贴，无需闪烁启动器）；
2. **零常驻 BYOM/BYOK 本地 AI 管道步骤（`flows_llm.rs` + 异步 `WM_APP_FLOW_DONE`）**：
   - 新增 `llm_transform` 原语与内置工作流 `aisum`（剪贴板本地 AI 提炼要点），后台短生命周期线程调用本地 Ollama 或 DPAPI 加密 OpenAI 兼容接口，不阻塞 UI、用完即退 0 MB 常驻；
3. **混合 DPI 多屏精修与 `sync_dir` 备忘录迁移**：
   - `layout.rs` 首帧基于目标显示器 `GetDpiForMonitor(MDT_EFFECTIVE_DPI)` 缩放并自动钳位在目标 `rcWork` 内，杜绝跨屏 DPI 错位与预览栏出界；`sync.rs` 切换同步目录时自动无损迁移 `notes/`；
4. **Windows ARM64 原生支持、SLSA 溯源与分发规范（[`docs/DISTRIBUTION.md`](DISTRIBUTION.md)）**：
   - CI 新增 `windows-11-arm` 原生编译测试，Release 同步发布 `windows-arm64-portable.zip` + 全量 `actions/attest-build-provenance@v2` SLSA 构建证明，并修复 Winget 发布清单正则保留 `ManifestVersion: 1.6.0`。

### 阶段十三（加固）：`v2.2.1` — 前后端结构减压、7 道防膨胀门禁与 Clippy 零警告工程加固（已完成 ✅）
1. **7 道自动化防膨胀门禁（`scripts/check_hygiene.ps1` + CI）**：
   - 新增 `380 / 450` 双水位提醒、前端单文件产物体积红线与重型 npm 黑名单、**Rust `HostMessage` ↔ TypeScript `postHost` 双向 IPC 契约全量校验**、以及 `-CheckBinaries` Release 二进制体积天花板；更新 [`docs/CODING_STANDARDS.md`](CODING_STANDARDS.md) 全面对齐 Svelte 5 + Tailwind v4 + TS 工程规范；
2. **全工作区 `cargo clippy --workspace --all-targets -- -D warnings` 零警告**：
   - 接入 GitHub Actions CI，清理全仓 17 个 Crate 全部静态告警，对 `HostMessage`、`UpdateStatus`、`LiveRuntime` 大变体执行 `Box` 缩栈，并将 `PopupState::closed` 升级为 `Cell<bool>` 消除潜在别名隐患；
3. **构建期 Gzip 压缩内嵌 HTML & Top-3 热点文件减压**：
   - `tomoyo-webview-host/build.rs` 使用 `flate2` 最高压缩比将 4 份单文件 HTML 压为 `.html.gz`（`600.7 KB → 199.7 KB`，压缩率 `-66.8%`），按需 `< 2 ms` 解压，`tomoyo-webview-host.exe` 体积从 `4.74 MB` 降至 `4.36 MB`；
   - 拆解最紧绷的 3 个 440+ 行热点：`scroll_capture.rs` (`450 → 330` 行 + `scroll_hud.rs`)、`annotate_render.rs` (`445 → 258` 行 + `annotate_arrow.rs`)、`TranslatorApp.svelte` (`442 → 378` 行 + `TranslatorDrawer.svelte`)；
4. **前端自动化测试矩阵扩容**：
   - 新增 `workbenches.test.ts`（翻译抽屉/生词本、剪贴板筛选/置顶/单行复制、OCR 清洗/翻译、FlowsTab 新建工作流）与 `ipc_contract.test.ts`（自动化 IPC 契约与热键/配置容错测试），3 套件 9 用例零警告全绿。

### 阶段十三（体验精修）：`v2.2.2` — 双击启动 0ms 瞬时反馈根治、第二梯队减压与双向 IPC 契约（已完成 ✅）
1. **搜索结果双击应用 0ms 瞬时反馈与防丢失根治（`launch_ctrl.rs` + `wndproc_mouse.rs` + `sdf_pass.rs` / `gdi_pass.rs`）**：
   - 彻底消除主线程同步 `ShellExecuteW` 阻塞 `300ms–2.5s` 导致的界面假死与无反馈黑盒：双击或回车触发启动类动作时 `< 2 ms` 同步刷新行内主题色「启动中…」徽章与底部状态栏，随后在独立 COM-STA 短生命周期线程执行 `AllowSetForegroundWindow(ASFW_ANY)` + `ShellExecuteExW`；
   - 成功时行内徽章翻绿显示「✓ 已启动」并保留清晰确认节拍（若目标应用抢先弹窗则立即让位隐藏），失败时**绝不隐窗**并标红直显具体原因（如文件不存在 / UAC 取消 / 权限不足）；
   - 补齐同行双击防抖容错（克服高 DPI 屏 4px `SM_CXDOUBLECLK` 限制）并将 OLE 拖拽门槛升级至 `MK_LBUTTON` + `12 × dpi_scale`，杜绝手抖吞双击。
2. **第二梯队 420+ 行热点拆解 & 双向 IPC 1:1 契约门禁**：
   - `wndproc.rs` (`437 → 302` 行 + `wndproc_mouse.rs`)、`bridge.rs` (`423 → 268` 行 + `bridge_ops.rs`)、`overlay.rs` (`423 → 277` 行 + `overlay_actions.rs`) 全员降至 310 行以下；
   - `check_hygiene.ps1` 与 `ipc_contract.test.ts` 升级为双向对称校验（46 项上行指令 + 26 项下行事件 1:1 零漏配）。
3. **工作台公共样式去重、剪贴板全键盘流、带参工作流反馈与 Node 24 CI 升级**：
   - 抽离 `frontend/src/styles/workbench.css`，为剪贴板工作台新增 `Ctrl+F` 聚焦、`↑↓` 切换与 `Enter` 快捷复制；工作流列表实时高亮行内文本参数提示并新增单元测试；CI/Release Actions 全面升级至 `checkout@v7` / `setup-node@v7` / `attest-build-provenance@v4` / `action-gh-release@v3`。

### 阶段十三（交互精修）：`v2.2.3` — 静默两段式 `Esc` 与全局 `F3` 万能贴图体系（已完成 ✅）
1. **彻底消除搜索框系统提示音并升级两段式 `Esc`（`input_edit.rs` + `input.rs`）**：
   - 在 `edit_subclass_proc` 中显式吞掉单行 `EDIT` 控件因 `Esc`/`Enter`/`Tab`/`Ctrl+<key>` 触发的 `WM_CHAR` 控制字符以及 `Alt+1..8` 的 `WM_SYSCHAR`（同步支持 `Ctrl+A` 无声全选），从根源彻底消除 Windows `MessageBeep` 提示音；
   - `Esc` 状态机升级为清晰渐进回退：动作菜单 → **先清空当前搜索内容（保留焦点继续检索）** → 清除粘贴胶囊 → 重置分组筛选 → 空框再次按 `Esc` 静默收起。
2. **一级「全局一键贴图」热键槽位（默认 `F3`）与设置中心防误导引导**：
   - 在 `CaptureConfig`、`HotkeyHealthReport`、`tomoyo-app`（`HOTKEY_PIN_ID = 5`）、`hotkey_doctor.rs` 与 Svelte 5 设置中心全链路接入独立全局贴图热键（支持 `F1–F12` 单键、冲突强接管、全屏避让与热键治理中心一键换键）；
   - 设置中心「截图与贴图」在全局截图下方新增醒目的「全局一键贴图快捷键」录入框（推荐 `F3`），并明晰底部小工具区仅作用于截图画布内，若用户在小工具区按下 `F1–F12` 将实时弹出温和引导提示。
3. **全格式万能剪贴板贴图引擎（`pin_clipboard.rs` + `pin_card.rs` + `pin_window.rs`）**：
   - 支持连续快速 `Ctrl+C` → `F3` 剪贴板重试锁与透明 `PNG` 白底抗黑化合成，按优先级智能贴图：剪贴板位图/PNG → `CF_HDROP` 复制图片文件/路径清单 → CSS 色值卡（`#RGB`/`#RRGGBB`/`rgb()`）→ 跟随当前主题的 ClearType 文本/代码便签卡；
   - 贴图窗就近弹出在当前鼠标所在显示器工作区内（多屏高 DPI 准确钳位）；文本/色值贴图按 `Ctrl+C` 还原复制原始纯文本、按 `L` 直接交由多源翻译窗，右键菜单同步提供「复制原始文本 / 复制为图片 / 保存 PNG / 翻译原文」，实测 0 GDI/USER 句柄泄露。

### 阶段十三（品牌视觉统一）：`v2.2.4` — 全窗口与设置中心应用 Logo 视觉统一（已完成 ✅）
1. **根治 Win32 内嵌图标资源 ID 错位（`win_theme.rs` + `win_host.rs` + `tray.rs`）**：
   - 修复此前 `std::ptr::dangling::<u16>()` 指向资源 ID `2`（而 `winresource` 实际嵌入 `RT_GROUP_ICON` 资源 ID `1`）导致设置中心/翻译/剪贴板/OCR 窗口标题栏与系统托盘无法加载真实 `tomoyo.ico` 的隐蔽缺陷，统一改用 `std::ptr::without_provenance::<u16>(1)`；
   - 升级 `tomoyo-webview-host` 至 `WNDCLASSEXW` + DPI 匹配双尺寸 `LoadImageW`（`SM_CXSMICON` 标题栏小图标 + `SM_CXICON` 任务栏大图标），并在 `--self-test` 增加真实图标句柄非空防回归断言。
2. **设置中心侧边栏与关于页品牌 Logo 统一（`SettingsApp.svelte` + `UpdatesTab.svelte` + `tomoyo-logo.png`）**：
   - 基于 `assets/tomoyo-icon-transparent.png` 生成 `96×96` 高清透明应用 Logo（与 `tomoyo.ico` 像素一致），替换设置中心导航栏上方旧字母方块 `T` 并同步展示于关于页头图，单文件 `settings.html` 保持在 `293.9 KB / 320 KB` 预算内。

### 阶段十三（稳定性与日志清净）：`v2.2.5` — 根治热键接管刷屏 WARN、钩子抖动与 WebView2 早关竞态（已完成 ✅）
1. **日志精准分级（`input.rs` + `plugin_hotkeys.rs`）**：
   - 移除 `register_optional_hotkey()` 内盲目打印的 `WARN`，仅在热键确实不可用（未启用接管或底层接管钩子挂载失败）时输出 `WARN`；已被高优先级钩子自动接管生效的槽位记为 `INFO`，不再误污染诊断面板告警计数。
2. **消灭焦点切换与非热键保存引发的钩子拆装风暴（`hotkey.rs` + `win_host.rs` + `selftest.rs`）**：
   - 新增 `HotkeyHealthReport::has_unresolved_conflicts()`：设置中心 `WM_ACTIVATE` 仅在存在真实未解决冲突（`!ok`）时才通知主进程重探热键；全部热键均已生效（含已接管）时零拆装、零热键真空期；
   - `reload_config_internal` 引入 `hotkeys_config_changed` 与 `avoidance_config_changed` 脏位比对：修改外观、窗口尺寸、目录等非热键配置完全不触碰现有热键与钩子，修改避让名单时仅原子刷新 `sync_hotkey_avoidance`。
3. **根治 WebView2 窗口早关引发的 `HRESULT(0x80070578)` 假 ERROR（`win_host.rs`）**：
   - 在 `CreateCoreWebView2Controller` 调用前后及回调入口校验 `IsWindow(Some(hwnd))`；若用户于初始化前提前关窗则静默释放并退出，彻底消除无效窗口句柄 `ERROR`。

### 阶段十四：`v2.3.0 – v2.3.1` — Linux KDE Wayland/X11 主端落地与 Windows 自更新修复（已完成 ✅）
1. **Linux CachyOS / Arch KDE 原生全栈落地（`v2.3.0`）**：
   - 主壳、`tomoyo-webview-host`（WebKitGTK 4.1）、`tomoyo-capture`（Spectacle/`grim`/X11）、`tomoyo-plugin-ocr`（Tesseract 5）、`org.kde.kglobalaccel` 热键、`fd` + `plocate` 检索与 Arch `PKGBUILD` 全链路验证通过。
2. **Windows 一键校验升级修复（`v2.3.1`）**：
   - `JobGuard::assign_webview_child` 开启 `JOB_OBJECT_LIMIT_BREAKAWAY_OK`，搭配 [installer/win.rs](../crates/tomoyo-core/src/updater/installer/win.rs) `CREATE_BREAKAWAY_FROM_JOB` 与 `CIM Win32_Process.Create` 双重脱离，防止设置窗口关闭回收 Job 时误杀安装脚本；补齐跨架构资产匹配、7 类子进程退出清理与 `%LOCALAPPDATA%\Tomoyo\logs\update.log` 落盘。

### 阶段十五：`v2.4.0` — 贴图/长截图/更新器交互打磨与设置中心微内核收敛（已完成 ✅）
1. **高频工具交互手感精修（`pin_window.rs` + `scroll_preview.rs` + `UpdatesTab.svelte`）**：
   - **贴图窗口自管拖拽与光标锚定缩放**：告别系统 `HTCAPTION` 顶部与边缘强制阻挡，改为 `SetCapture` 自管拖拽，支持跨屏幕四边越界自由停靠（全虚拟桌面仅保留 `44 px` 最小抓手边际），滚轮按鼠标所指像素为中心缩放并增加外框对比描边。
   - **长截图内容自适应预览**：`LivePreviewStrip` 改为预分配最大 `DIB` + 按 `StitchCanvas` 实际比例动态长高，移除初始大片空白黑框，达到屏幕上限后再平滑切换滚动深度追踪。
   - **检查更新平滑回执与升级日志入面板**：手动检查保持至少 `480 ms` 旋转动画过渡并展示 Toast / 状态脉冲回执；诊断面板新增 `update.log` 与 `inno_setup.log` 查看源。
2. **设置中心 12 → 6 栏微内核收口与「真·插件内配置」（`SettingsApp.svelte` + `PluginsTab.svelte`）**：
   - 左侧导航精简为 **常规与外观 / 搜索与网页 / 热键治理 / 轻量工作流 / 插件中心 / 诊断与更新** 共 6 项，并新增顶部设置快捷搜索直达栏。
   - 剪贴板、多源翻译、截图与贴图全面下沉为「插件中心」模块内原生配置抽屉（与 OCR、备忘录及 Protocol v2 插件体验一致），同时支持从剪贴板工作台、翻译窗与 OCR 审校窗顶部一键直达对应插件设置；后续演进聚焦 PixPin 贴图队列与多选区记忆、CopyQ 顺序粘贴、Pot 流式翻译与统一内置插件清单。

### 阶段十六：`v2.4.1` — Everything 日志降噪、设置过渡打磨与级联尝试右键菜单（已完成 ✅）
1. **Everything IPC 超时降噪与诊断归档（`ipc.rs` + `everything.rs` + `diagnostics.rs`）**：
   - `tomoyo-everything` 精准区分 `ERROR_ACCESS_DENIED (UIPI)`、`ERROR_INVALID_WINDOW_HANDLE` 与真实超时；全局搜索智能跳过纯算术表达式（如 `5564*0.05`）与超短纯 ASCII 词（`<3` 字符且仍保留 `f `/`ev ` 专用前缀全匹配）；全局慢速防抖提升至 `85 ms` 并加入连续超时短路冷却与日志降噪，避免快速连续输入触发串口雪崩 `WARN`。
   - 诊断与日志面板新增「归档清理」一键动作（安全归档旧版 `last_crash.txt` 并截断旧日志重置计数）。
2. **搜索框图标打开设置中心平滑过渡（`win_theme.rs` + `win_host.rs` + `selftest.rs`）**：
   - 顶栏 `T` 徽章支持悬停微光与手型指针反馈，左键点击即可平滑唤出设置中心；WebView2 宿主窗口预设与主题一致的 `hbrBackground` 画刷和 `SetDefaultBackgroundColor`，隐藏创建并由 Svelte 首帧 `window_ready`（或 `360 ms` 保底）后呈现，主搜索框通过 `WS_EX_LAYERED` 微步淡出交接，消除白底空窗闪烁。
3. **搜索输入框右键菜单重构（`input_edit.rs` + `popup_menu/` + `window/menus.rs`）**：
   - `edit_subclass_proc` 接管原生 `EDIT` 右键（`WM_CONTEXTMENU`），展示主题同步的高清级联弹出菜单。
   - **一级/二级层次规划**：划分 `编辑与输入`、`切换搜索模式`、`外观主题`、`界面语言`、`视图与窗口`、`诊断与维护` 等二级子菜单；**尝试不自动关闭**：切换主题、语言、预览开关、筛选标签栏、搜索模式或编辑动作均即时生效并同步更新窗口与菜单自身外观，仅当点击菜单外部、按 `Esc` 或选择「设置中心 / 退出」等终结动作时才收起。

### 阶段十七：`v2.5.0` — 分类栏扩展简化与结果排版现代化可配置（已完成 ✅）
1. **搜索框分类栏多行流式 / `+N ▾` 溢出架构与 `32` 标签承载（`filter_bar.rs` + `filter.rs` + `layout.rs` + `menus.rs`）**：
   - 自定义分类标签上限从 `12` 提升至 `32`，主窗口分类栏支持**多行流式换行（最多 3 行）**与**单行溢出折叠 `+N ▾`**两种排版；超出可见区域或激活隐藏标签时自动生成主题弹出菜单一键直选并高亮，彻底解决旧版单行截断丢失标签的问题。
2. **设置中心统一交互式标签画布（`FilterTabsCard.svelte`）**：
   - 合并原「实时预览 / 内置开关 / 预设 / 自定义列表」四重冗余区域为单一紧凑卡片：点击内置芯片直接显隐，点击自定义芯片就地编辑并支持关键字筛选（≥6 项自动出现）及排版模式切换，大幅降低界面复杂度与垂直空间占用。
3. **搜索结果字体样式现代化与字体/粗细/字号可配置（`typography.rs` + `fonts.rs` + `gdi.rs` + `sdf_pass.rs` + `render_pango.rs` + `AppearanceTab.svelte`）**：
   - 新增 `TypographyConfig`（字体族、标题字重 `300–700`、副标题字重 `300–600`、标题字号 `14–18px`）；枚举本机已安装字体（`EnumFontFamiliesExW`）并与内置阿里巴巴普惠体 3.0 协同，自适应调节结果行高（`54/58 DIP`），重绘结果图标垂直居中与 `Enter`/`Alt+N` 按键胶囊微标，外观页提供实时样式预览对比。

### 阶段十八：`v2.5.1` — 设计规范 §1.3/§2.8 补充与设置中心分类栏/字体选择器合规重构（已完成 ✅）
1. **补充设计规范（`docs/UI_DESIGN_STANDARDS.md`）**：新增 **§1.3 设计原子强约束**（严禁未声明全局类与胶囊内嵌套裸 `<button>` 形成“框套框”）、新增 **§2.8 可扩展分类标签组与结构化规则列表规范**、完善 **§1.2 可配置高级排版约定**。
2. **设置中心规范修复与极简分层（`Select.svelte` + `AppearanceTab.svelte` + `FilterTabsCard.svelte`）**：
   - 为 `Select.svelte` 增加 `searchable` 实时检索过滤，替换 `AppearanceTab.svelte` 的原生 `<select>`，修复 `.tab-head` 与 `.t-row` 标准容器排版。
   - 重构 `FilterTabsCard.svelte` 为清晰三段式行结构（排版模式 `Segmented` + 中性内置分类开关 + `38 px` 紧凑自定义标签列表/`Select` 预设菜单/标准 `<Input />` 内联编辑器），彻底消除多层边框堆砌与胶囊混杂。

### 阶段十九：`v2.6.0` — 备忘录与 Markdown 编辑器解耦、插件专属 Logo 与双呈态模式（已完成 ✅）
1. **备忘录（Quick Notes）与 Markdown 编辑器（`Quiet.md`）职责分离**：
   - **备忘录（`plugins/official-notes`，`dev.tomoyo.notes`）**：专注本地快速记录、即时检索、#标签筛选、剪贴板转存与右侧轻量预览；支持一键交付 **Quiet.md** 精编（`preferred_editor` 可选 `auto [Quiet.md]` / `system` / `custom`）。
   - **Markdown 编辑器（`plugins/official-md-editor`，`dev.tomoyo.markdown-editor`）**：官方集成开源桌面编辑器 [`s1dashu/quiet-md`](https://github.com/s1dashu/quiet-md)（MIT，Rust + GPUI）；零 `tomoyo.exe` 体积膨胀——自动探测本机 `quiet-md.exe`（或 Linux `quiet-md`）直传文件路径编辑，未安装时支持从官方 Release 按 `SHA256SUMS` 校验下载部署至 `%LOCALAPPDATA%\Tomoyo\tools\quiet-md\`，并为全仓与 Everything `.md` 结果补齐快速打开入口。
2. **插件专属 Logo 全链路体系（`IconSpec::ImageFile` + Manifest `icon` + Host `WM_SETICON`）**：
   - 协议层（`PluginManifest`）新增 `icon`、`category`、`display_mode`、`tags`，`IconSpec` 新增 `ImageFile(String)`（基于零额外依赖 `WIC / png` 异步解码缓存），覆盖搜索结果行（Windows SDF + Linux Pango）、**WebView 宿主独立窗口标题栏与任务栏图标（动态构建 ARGB `HICON` / GTK icon，不再千篇一律显示 Tomoyo Logo）**、以及设置中心插件/市场图标徽章。
   - 为全套内置模块（剪贴板 / 多源翻译 / 截图贴图 / OCR / 备忘录 / Markdown 编辑器）与官方参考插件设计独立、高辨识度的现代矢量图标。
3. **插件双呈态展示：`inline` 搜索结果内嵌展示 vs `window` 独立窗口分离**：
   - 插件可在 `tomoyo-plugin.toml` 声明默认 `display_mode`（`auto` / `inline` / `window`），用户亦可针对任意插件在设置中心「插件中心」与搜索结果动作菜单中自由覆盖切换。
   - `inline` 模式将 WebView 宿主按无边框从属面精准贴靠在搜索框右侧预览区域内，主窗体聚焦保护自动识别子宿主（点击不误关），跟随缩放/关闭即时销毁释放全部 WebView 内存，并支持顶部一键「分离为独立窗口（Pop Out）」热切换至带专属 Logo 标题栏的 `WS_OVERLAPPEDWINDOW`。

### 阶段十九·补丁：`v2.6.1` — Windows 原生区域录屏、双卸载项治理与 Everything UIPI/截图自捕修复（已完成 ✅）
1. **Windows 端原生区域录屏（`win_record*.rs` + `overlay*.rs` + `SettingsApp.svelte`）**：
   - 免装 FFmpeg 开箱即录：集成 Windows 原生 **Media Foundation (`IMFSinkWriter`)** H.264/AAC MP4 编码器、**WASAPI** 系统声音 Loopback / 麦克风采集、纯 Rust **Median-Cut 256 色 + 脏矩形 LZW GIF89a** 编码器，与 `WDA_EXCLUDEFROMCAPTURE` 防穿帮录制边框及悬浮控制条（暂停/继续/完成/`CF_HDROP` 复制文件/播放/定位目录），全量打通截图工具栏、快捷键 `V`、搜索前缀 `rec`/`lp` 与设置中心。
2. **彻底修复双卸载注册表条目（`tomoyo.iss` + `install-local.ps1` + `uninstall-local.ps1` + `updater/installer/win.rs`）**：
   - 建立 Inno Setup（`{8F4E29A1-...}_is1`）与本地脚本安装（`Uninstall\Tomoyo`）双向互斥清理机制，加固 `Cargo.toml` 版本提取正则，保证卸载列表始终仅展示唯一 Tomoyo 条目。
3. **修复高权限下 Everything 搜索 100% 超时与截图截不到搜索框**：
   - 在 `tomoyo-everything` 接收窗显式调用 `ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, None)`，彻底解除高权限 `tomoyo.exe` 无法接收非提权 `Everything.exe` 回复导致的 UIPI 静默丢包（修复 `.md` 等全盘文件搜不到问题并提升 `.md`/`.txt` 相关性权重）。
   - 全局截图/OCR 热键改为不预先隐藏搜索框并在 `enumerate_snap_windows` 中放行 `TomoyoLauncherWindowClass`，直接支持一键吸附截取 Tomoyo 搜索框界面。

### 阶段二十：`v2.7.0` — 面向成千上万款插件的「插件中心 2.0」可扩展架构（架构奠基 → 深化）
1. **分片注册清单协议（Registry Schema v2）**：
   - 支持 `schema_version: 2` 轻量主清单 + 按分类分片（`shards` + ETag 304 + `%LOCALAPPDATA%\Tomoyo\cache\store\` 本地缓存），向下兼容全部 v1 清单；内置分类（`efficiency / docs / dev / ai / media / system`）、评分/下载量元信息、校验徽章与按需图标。
2. **Rust 侧检索引擎与分页 IPC 协议（防前端 DOM / 内存膨胀）**：
   - 严禁在设置初始化 `__TOMOYO_SETTINGS_INIT__` 中全量注入市场列表；改为由 `tomoyo-webview-host` 在 Rust 侧构建常驻内存检索引擎（关键词 / 分类 / 运行时 / 排序 / 装机状态过滤），通过 `QueryStore` / `StorePage` 分页协议每次仅向 Svelte 交付一页（如 `24` 条）与按页按需的轻量缩略图标，`10,000+` 插件下 DOM 节点始终 `< 100` 且检索耗时 `< 5 ms`，`settings.html` 仍严守 `≤ 320 KB`。
3. **设置中心双栏检索与插件详情交互（`PluginsTab` + `StoreTab`）**：
   - 已安装插件列表与插件市场均支持即时关键字搜索、分类芯片筛选、呈现模式快捷切换，以及惰性拉取的插件详情抽屉（权限透明列表、支持触发词、作者与主页）。

### 阶段二十一：`v2.8.0` — 社区生态拓展与打包分发流水线
1. **`tomoyo-cli pack` 标准化社区插件包生成器**：校验 `icon.png`、`display_mode`、Protocol v2 Schema 与确定性 `SHA-256` 清单生成；
2. **WASM 声明式卡片输出原语**：允许零权限 WASM 沙箱插件输出结构化 Markdown/表格/快捷动作卡片在搜索结果右侧直接内嵌呈现；
3. **插件一键体检与多源仓库镜像健康检测**：自动检测过期清单、不可达镜像与插件版本升级提示。

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
2. **规范与单测门禁**：`node scripts/check_hygiene.mjs`（Windows 可选 `pwsh -File scripts/check_hygiene.ps1`）+ `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace`；
3. **性能与资源预算门禁**：`tomoyo --self-test --strict-budget`（Linux Private ≤ 45.0 MB / Windows WS ≤ 2.0 MB & Private ≤ 28.0 MB，100 轮压测零句柄泄漏）；
4. **实机交互验收**：在 Linux KDE Plasma Wayland/X11 与 Windows 桌面实测核心路径、边界输入与日志溯源；
5. **版本发布与本机升级**：GitHub Actions 双平台（Linux + Windows）CI + Release 全绿，生成 `SHA256SUMS` 并完成 `.tar.gz` / `.desktop` / Windows 安装包升级验收。
