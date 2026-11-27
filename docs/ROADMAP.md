# Tomoyo 长期演进蓝图与分阶段替代计划 (Long-Term Roadmap)

> **终极定位（North Star）**：Tomoyo 的终极目标是成为陪伴开发者与效率用户一生的**「跨操作系统、零账号、本地优先的个人计算中枢（Personal Computing Shell）」**——只要桌面计算范式尚未发生颠覆式改变，它就是用户快捷键肌肉记忆、高频指令、代码片段、工作流与本地数字资产不依赖任何商业厂商、可零成本跨系统长期持有的个人计算外壳。
> **两大长期准入原则**：任何新能力进入 Tomoyo 前必须通过两道自检——① *「这个能力我 5 年后还会每天用吗？」* ② *「这个能力能否放在按需隔离插件里，而不是塞进微内核？」*
> **基线与核心目标**：`v3.2.1` + `v3.3.0 Milestone 1` 已完成按需短命 WebView2 宿主分离、远程桌面穿透、磁吸防闪烁截图、双机分层配置、原生端口查杀、按需分类侧栏、纯 Rust Fluent 标签弹窗、离线自然语言文件筛选与 `?` / `/` 命令中心；`v3.3.0 Milestone 2` 已持续落地 OS 原生 UI 字体栈与一键Preset、Linux RGB 次像素抗锯齿、8 向缩放与 KWin 6.8 Wayland 位置记忆、2px 光标精修、Linux 可拖动 Fluent 滚动条、Wayland/X11 `PRIMARY` 零剪贴板取词与 `.xlsx` 纯 Rust 表格预览。`tomoyo` 始终坚守极简、低占用、毫秒唤起的微内核原则，高频重型工具一律作为按需隔离插件提供。
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
│  v3.3.0 · [M1 已完成 ✅ · M2 持续推进中] 原生字体栈与一键Preset · KWin6.8    │
│           Wayland位置记忆 · 2px光标精修 · Linux可拖动滚动条 · PRIMARY零剪贴  │
│           板取词 · .xlsx首表预览 · 按需侧栏 · 自然语言筛选 · ? / 指令中心    │
├──────────────────────────────────────────────────────────────────────────────┤
│  v3.2.1 · [已完成 ✅] 远程桌面键鼠穿透 · 插件读线程零滞留 · 磁吸防闪烁截图   │
│           双机分层配置 · 原生 TCP 端口查杀 · JSON / CSV 结构化预览           │
├──────────────────────────────────────────────────────────────────────────────┤
│  v3.2.0 · [已完成 ✅] 上下文感知剪贴板卡 · 旗舰插件 2.0 (顺序粘贴/重截/老板键 │
│           翻译风格预设) · 零权限纯 WASM 开发者工具箱 · 社区 WASM 模板仓库    │
├──────────────────────────────────────────────────────────────────────────────┤
│  v3.1.0 · [已完成 ✅] 零剪贴板 UIA 超级面板 · 核心翻译彻底去重 · 插件文本动作│
│           高 DPI 清晰图标 · 可拖动滚动条 · 12 层深扫热监听 · 剪贴板防竞态加固│
├──────────────────────────────────────────────────────────────────────────────┤
│  v3.0.0 · [已完成 ✅] Core/Suite 物理拆包 · WASM ABI v2 · 签名/溯源 · macOS  │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.8.0 · [已完成 ✅] 社区 CLI 打包/分片/体检 · WASM RichCard · 清单同步流水线│
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.7.5 · [已完成 ✅] 录屏 2.0（独立热键/双音混音/点击波纹）· Inline Esc 精修 │
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.7.0 · [已完成 ✅] 万级插件中心 2.0（分片索引 / ETag 缓存 / 详情抽屉/升级）│
├──────────────────────────────────────────────────────────────────────────────┤
│  v2.6.2 · [已完成 ✅] Windows Clippy 修复 · 450 行热点拆解 · settings 瘦身   │
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

### 阶段二十：`v2.6.2 – v2.7.5` — 工程稳态加固、万级多平台插件中心 2.1、解耦 Phase 1 与录屏 2.0（已完成 ✅）
> 微内核彻底解耦架构蓝图见 [`docs/DECOUPLING_BLUEPRINT.md`](DECOUPLING_BLUEPRINT.md)。
1. **Windows Clippy 修复与热点减压（`v2.6.2`）**：
   - 收敛 `popup_menu` 签名至 `PopupMenuStyle`；拆解 `main.rs`、`window/mod.rs`、`win_host.rs`、`config/mod.rs` 全部 450 行边缘热点；移除前端未使用的 Tailwind v4 全局生成器并启用 `es2022` / `legalComments: 'none'`，一次性释放 `14.6 KB` `settings.html` 体积裕量。
2. **面向成千上万款插件的「插件中心 2.1」（`v2.7.0`）**：
   - 落地 `schema_version: 2` 分片注册清单（`registry/index.json` + `registry/shards/{official,examples}.json`）、`platforms` 多架构产物映射（`windows-x86_64` / `linux-x86_64` / `macos-*` / `any`）与 `min_tomoyo_version`、HTTP `ETag` / `If-None-Match` 304 与本地磁盘分片缓存。
   - `tomoyo-webview-host` 在 Rust 侧实现毫秒级过滤/分类/排序/分页与惰性 `FetchPluginDetail` 详情抽屉，内置 SemVer 升级徽章与原子暂存更新（`StoreTab` + `PluginsTab`），`settings.html` 始终控制在 `≤ 320 KB`。
3. **首批彻底零特权拆离（Quiet.md / 备忘录 / 示例插件）**：
   - 将 Quiet.md 下载校验器从 `tomoyo-core`、`tomoyo-protocol`、`tomoyo-providers`、`tomoyo-webview-host` 与设置前端全量剥离，回归 `plugins/official-md-editor` 插件自包含执行；设置中心已安装列表不再硬编码外部插件占位条目；安装脚本默认不再强制铺放 `example-*` 示例插件（通过 `-WithExamples`/`--with-examples` 按需开启）。
4. **录屏 2.0 与 `inline` 贴靠交互精修（`v2.7.5`）**：
   - 全链路新增独立一级全局录屏热键槽位（`CaptureConfig.record_hotkey`，含 Windows/Linux 热键注册、热键治理中心与设置中心）；
   - 新增 `RecAudio::Both`（「双音」：Windows 双 WASAPI 实时软削波混音 + Linux PulseAudio `amix=inputs=2`）、Windows 鼠标点击波纹高亮与录屏保存后自动复制文件（`CF_HDROP`）至剪贴板；
   - `inline` 贴靠模式下按 `Esc` 优先平滑关闭内嵌面板并将焦点归还搜索输入框。

### 阶段二十一：`v2.8.0` — 社区生态拓展、WASM 富卡片与自动化清单流水线（已完成 ✅）
1. **WASM 声明式富卡片原语（`PreviewSpec::RichCard`）**：
   - 协议层与 Windows SDF/GDI、Linux Pango 原生预览管线支持结构化富卡片（标题/副标题/彩色徽章/键值对表格/页脚），参考插件 `plugins/example-wasm` 率先升级 `RichCard` 输出（JWT 解码与字节换算）。
2. **`tomoyo-cli` 社区打包、分片构建与离线体检工具链**：
   - `tomoyo-cli verify` / `pack` 强制校验 PNG 魔数与 `≤ 64 KB` 图标红线并生成带 `platforms` 的 Protocol v2 条目；新增 `tomoyo-cli registry build-shards` 自动汇总重建分片 SHA-256 与 `tomoyo-cli doctor` 离线/镜像体检；设置中心插件页同步接入「一键体检（`CheckPluginHealth`）」。
3. **发布清单自动化同步流水线（`scripts/sync-release-manifests.mjs`）**：
   - 接入 `.github/workflows/release.yml`，发布构建自动产出版本号与 `SHA-256` 严格对齐的 Scoop、Winget 与 Arch `PKGBUILD` 清单。

### 阶段二十二：`v3.0.0` — Core/Suite 物理拆包、WASM ABI v2、签名溯源与 macOS 能力对齐（已完成 ✅）
1. **WASM Runtime ABI v2（`wasm.rs` + `wasm_kv.rs` + `tomoyo-wasm-sdk`）**：
   - 新增受限主机原语 `host_now_ms` 与按插件 ID 严格目录隔离的有界持久化键值存储 `host_kv_get` / `host_kv_set`（`<data_dir>/plugin-kv/<plugin_id>.json`，单值与总容量硬限），配套跨插件隔离单元测试。
2. **Core vs Suite 物理拆包与多渠道可选组件安装**：
   - `install-local.ps1`（`-Profile Core|Suite` / `-CoreOnly` / `-WithExamples`）、`install-local.sh`（`--profile=core|suite` / `--core-only` / `--with-examples`）与 Inno Setup `tomoyo.iss` `[Types]`/`[Components]` 正式支持极简 Core 微内核与完整 Suite 双分发形态；Release 工作流同步产出 `*-core-portable.zip` 与 `*-core.tar.gz`。
3. **可验证供应链与可选 Windows Authenticode 签名钩子（[`docs/RELEASE_SIGNING.md`](RELEASE_SIGNING.md)）**：
   - 全平台发布包接入 `SHA256SUMS` 与 GitHub `actions/attest-build-provenance@v4` SLSA 溯源证明，并在 Windows x64 Release 流水线预置基于仓库密钥（`WINDOWS_CERT_PFX_BASE64` / `WINDOWS_CERT_PASSWORD`）的条件式 `signtool.exe` SHA-256 + RFC 3161 时间戳自动签名。
4. **`tomoyo-ui-macos` 平台能力对齐**：
   - 补齐 Spotlight `mdfind` 参数构建与输出解析、系统剪贴板读取、Finder 定位与 `LaunchAgent` plist 生成器及跨平台单元测试，为 macOS 原生外壳落地夯实接口底座。

### 阶段二十三：`v3.0.x` — Protocol v3 通用底座、重型官方模块全量外迁与 Starter Packs（已完成 ✅）
> 完整蓝图与验收记录见 [`docs/DECOUPLING_BLUEPRINT.md`](DECOUPLING_BLUEPRINT.md) Phase 1–4：
1. **Protocol v3 通用插件底座（Phase 2）**：落地 Hybrid 混合运行时（`window.tomoyo.callPlugin` ↔ `tomoyo/call`）、声明式 `[[settings]]` v3（`secret` / `hotkey` / `dir` / `file` / `section` / `visible_if`）与通用加密密钥库（`plugin:<id>:<key>`）、`[[tray_actions]]`、`[[file_actions]]`、`Permission::SelectionRead` 与 `tomoyo-cli dev --devtools`；
2. **四大重型官方模块 100% 零特权外迁（Phase 3a–3d）**：离线 OCR（`dev.tomoyo.ocr`）、多源划词翻译（`dev.tomoyo.translate`）、原生截图贴图录屏（`dev.tomoyo.capture`）与剪贴板历史工作台（`dev.tomoyo.clipboard`）全部迁出主内核与 `tomoyo-webview-host`，移除核心 `rusqlite`、内置业务 Provider/Tab/HTML 及静态剪贴板监听（按订阅动态启停），`tomoyo` 减重至 `5.86 MB`、`tomoyo-webview-host` 减重 `-35%` 至 `3.40 MB`（仅内嵌 `settings.html`）；
3. **Starter Packs 与多平台独立插件分发（Phase 4）**：插件市场新增四组按需装机套件（纯净微内核 / 高频效率 / 写作识字 / 官方全家桶），CI/Release/Inno Setup/Arch `PKGBUILD` 全量同步 6 款官方插件及每款插件独立的 Linux/Windows ZIP 产物与 `SHA256SUMS`。

### 阶段二十四：`v3.1.0` — 零剪贴板 UIA 超级面板、核心彻底去重、高 DPI 图标与交互/稳态全链路打磨（已完成 ✅）
1. **原生零 WebView 超级面板与零剪贴板 UIA 划词（`super_panel/` + `uia_probe.rs`）**：
   - 极简单列光标浮窗（支持右键长按 / 中键长按 / 中键单击三选一统一配置），采用严格平衡的 **Swallow-and-Replay** `WH_MOUSE_LL` 钩子（消除 `win32k` 按键失同步与任务栏假死）及三重冗余点击外部关闭；
   - **优先 Win32 UI Automation（`IUIAutomationTextPattern2` / `TextPattern`）无痕取词**（`< 3 ms` 零剪贴板触碰、零 `Ctrl+C` 注入），内置 **终端 `SIGINT` 保护名单**（防止终端无选区时发送 `Ctrl+C` 误杀正在运行的命令），仅在非终端文件管理器/旧控件时回退至防竞态无损剪贴板探测；同步赋能 `Alt+T`（`Permission::SelectionRead`）；
   - **核心翻译引擎彻底去重**：删除 `tomoyo-core/src/translate/engines.rs`（444 行）与冗余 `TranslateStore`，新增 `PluginHost::resolve_stdio_oneshot()` 无锁快照 + `tomoyo-plugin-translate --quick-translate` 单次委托，超级面板内联翻译与翻译工作台 100% 共享插件设置/DPAPI 密钥且永不阻塞 `PluginHost`；
   - **官方插件接入 Protocol v3 `[[text_actions]]`**：`dev.tomoyo.notes` 新增「一键存为备忘（`save_selection_note`）」，`dev.tomoyo.clipboard` 新增「置顶存入剪贴板（`pin_selection_text`）」。
2. **启动器质感、深层索引热监听与剪贴板防竞态加固**：
   - **高清 Shell 图标与可拖动滚动条**：`icon_extract.rs` 接入 `SHGetImageList`（`SHIL_EXTRALARGE` / `SHIL_JUMBO`）+ 2×2 Lanczos 抗锯齿下采样消除图标模糊锯齿；`scrollbar.rs` 提供 Fluent 自绘侧边滚动条（支持鼠标拖动拇指、点击轨道翻页与悬停加宽）；
   - **1–12 层 BFS 深层自定义目录索引 + 热变更自动重同步**：`custom_dirs.rs` 支持最多 12 层深层目录与文档索引（自动过滤构建噪声目录），`AppsProvider::reload()` 动态同步 `AppDirWatcher` 监听集合，新增/移除自定义目录免重启即刻生效；
   - **OS 剪贴板防竞态加固（`clipboard_ctrl.rs` + `clipboard_win.rs`）**：120 ms `WM_CLIPBOARDUPDATE` 防抖合并突发写入、单次 `< 0.3 ms` 原子锁快照（DIB 解码移出锁外）、有效属主 `HWND` 及 30 ms 序列号静默窗口，消除 `CLIPBRD_E_CANT_OPEN`；
   - **工程减压**：清理 `frontend/src/lib/i18n.ts` 中 55 个历史废弃双语键（`settings.html` 降至 `288.3 KB / 300 KB`），拆解 `TranslatorApp.svelte`、`tomoyo-plugin-ocr/src/main.rs`、`store_index.rs`、`official-md-editor/src/main.rs`、`official-notes/src/main.rs`、`launch_ctrl.rs` 全部贴顶热点。

### 阶段二十五：`v3.2.0` — 上下文感知、旗舰插件 2.0 与纯 WASM 开发者生态基座（已完成 ✅）
1. **启动器零轮询上下文感知剪贴板卡（`window/context_card.rs`）**：
   - 空输入唤起（`All` 视图）时对 90 秒内新鲜剪贴板自动呈现至多 1 张上下文直达卡：URL 浏览器直达、JSON 一键美化/预览、色值桌面贴图/大小写切换、短文本多源翻译或存备忘、剪贴板位图一键贴图/离线 OCR，严格尊重 `disabled_plugins` 与零后台内存开销。
2. **三大旗舰插件 2.0 纵深进化（零核心膨胀）**：
   - **`dev.tomoyo.clipboard` 顺序粘贴队列（Paste Stack · `paste_stack.rs`）**：支持 `cb :stack` 管理最多 32 项 FIFO 队列、任意历史记录 Tab 入队、一键装入最近 5 条记录及可选全局弹出快捷键 `paste_stack_next`（顺序弹出并复制下一项，自动汇报剩余条数）；
   - **`dev.tomoyo.capture` 重截上次选区 & 多贴图老板键（`region_prefs.rs` + `pin_window.rs`）**：持久化上次有效选区坐标支持 `--repeat-region` 无弹窗一键复截，新增跨进程 `RegisterWindowMessageW` 广播 `--toggle-pins` 一键隐藏/恢复全部悬浮贴图，并开放插件热键与托盘动作入口；
   - **`dev.tomoyo.translate` 领域风格预设（`style_prompt.rs` + `TranslatorApp.svelte`）**：翻译胶囊新增 `标准 / 学术 / 代码 / 商务 / 润色` 五档 Prompt 预设，为 OpenAI 兼容模型与本地 Ollama 自动注入对应领域系统提示词并在结果卡显示风格徽标。
3. **官方零权限纯 WASM 开发者工具箱与社区模板基座**：
   - 新增 **`plugins/official-devtools-wasm`（`dev.tomoyo.devtools`）**：零环境权限（`permissions = []`）、16 MB 线性内存硬顶、仅 ~262 KB 跨平台 `plugin.wasm`，内置 Unix 时间戳/UTC+8/ISO-8601 互转（Howard Hinnant 纯整数历法）、UUID v4/256-bit 密钥生成（接入 ABI v2 `host::kv_get`/`kv_set` 持久递增盐）、Base64/URL 双向编解码与 Query 拆解、JWT 离线过期解析、5 段 Cron 中文释义及超级面板 `[[text_actions]]` 一键直达；
   - 新增 **`templates/plugin-wasm-template/`**：开箱即用的 10 分钟社区 WASM 插件模板仓库（含 `Cargo.toml`、`tomoyo-plugin.toml`、`src/lib.rs`、`README.md` 与一键打包/发布 SHA-256 的 GitHub Actions 工作流）。

### 阶段二十六：`v3.2.1` — 远程桌面穿透、插件读线程内存治理、截图防闪烁磁吸与双机/端口/预览增强（已完成 ✅）
1. **远程桌面快捷键与鼠标触发全景打通（`3d17a24` + `a42dafd`）**：
   - 移除 `hotkey_avoid.rs` 中将 `QUNS_BUSY` 误判为全屏独占的逻辑（保留真实 D3D 全屏与演示模式检测 + 3s 节流避让日志）；
   - 将 `hotkey_override.rs`（`OVERRIDE_EXTRA_INFO`）与 `super_panel/trigger.rs`（`REPLAY_EXTRA_INFO`）的注入过滤精确限定为 Tomoyo 自身发送的 `dwExtraInfo`，放行 RustDesk/ToDesk/向日葵等远程控制服务端转发的鼠标键盘事件，并在 Windows 默认排除列表加入 `rustdesk.exe`；
   - 同步补齐双端托盘「停用/恢复全局快捷键」快捷开关、Linux `KGlobalAccel` 陈旧槽位清理与双端自更新自动重启加固。
2. **插件 STDIO 读线程零跨线程堆滞留与 COM/Shell 内存治理（`3d17a24`）**：
   - 将 `tomoyo-plugin-host/src/process.rs` 中阻塞读线程的 `serde_json::from_str` 移回调用方主线程并回收复用 `<= 8 KB` 行缓冲，消除后台读线程 mimalloc 堆页滞留（休眠 Private Commit 由 `28.56 MB` 降至 `23.6–24.8 MB`，100 轮 `Handles +0`）；
   - `icon_extract.rs` 将重型 `IShellItemImageFactory` 限定在可执行体/快捷方式，文档类直走轻量 `SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES` 并统一休眠期 `CoFreeUnusedLibrariesEx(0, None)` 释放。
3. **截图智能选区防闪烁与磁吸动画升级（`8df3ada`）**：
   - 新增跨平台 `crates/tomoyo-capture/src/snap_motion.rs`：引入 **820 px/s / 42 px 单步高速门控 + 58 ms 驻留确认**、**105 ms 小区块回退整窗延迟防抖**、**24 ms 指数磁吸平滑插值与零后台唤醒停表**及 **`Alt` 键即时禁用吸附**；
   - `overlay_draw.rs` + `overlay_toolbar.rs` 落地柔和悬停暗罩分层（82% 均一底罩 + 94% 柔光预选框）、高速移动十字线淡化与放大镜自动避让角标。
4. **双机分层配置、原生端口查杀与轻量结构化预览（本轮交付）**：
   - **双机配置分层隔离（`config/local_override.rs`）**：启用同步目录后自动拆分本机专属字段（`apps.extra_dirs`、`capture.save_dir`、`hotkey_excluded_apps`、窗口位置）至 `%LOCALAPPDATA%\Tomoyo\config.local.toml`，避免工作机/家用机互相覆盖路径；
   - **原生零子进程 TCP 端口查杀（`providers/src/ports.rs` + `port` 指令）**：Windows 基于 `GetExtendedTcpTable`、Linux 基于 `/proc/net/tcp*` 瞬时检索监听端口与归属进程并支持回车结束占用进程；
   - **结构化表格与 JSON 预览（`tomoyo-core/src/preview.rs`）**：支持 `<= 8 KB` `.json` 自动美化格式化与 `.csv`/`.tsv` 对齐表格预览。

---

## 五、中长期演进蓝图 (`v3.3 → v4.0` · 四阶段路线图)

1. **Stage 1 (`v3.3.0`) · 双端拉齐与日常零摩擦底座（Linux 与 Windows 同等打磨）**：
   - **[已完成 ✅ · Milestone 1] 零记忆负担交互与自然语言筛选**：
     - **搜索与标签直觉化**：按需左侧分类侧栏（多结果才展开 + 零跳变工作区锁定）、设置中心无边框自绘标题栏（浅色/深色/午夜蓝 1px 零白边同步）与搜索框直搜已装插件一键跳对应设置页；
     - **纯 Rust Fluent 标签弹窗**：去除生涩 `Everything 表达式` 术语，升级为 `今日修改 / 近7天 / 大文件 / 仅文件夹 / 下载目录` 可点击自然语言条件芯片 + 实时中文/英文生效规则译文预览 + 主题适配占位提示；
     - **100% 离线自然语言文件筛选引擎**：在 `f ` / `ev ` 及文件分类视图下支持 `f 今天 pdf`、`f 大文件`、`f 近7天 合同`、`f 下载目录` 等日常短语零延迟直译 Everything 语法，并补齐友好引导卡；
     - **`?` / `/` 零门槛命令与快捷键发现中心**：无需死记前缀，输入 `?` / `/` 或 `帮助` 一键浏览并填入全部内置效率能力与快捷键速查表。
   - **[推进中 🔜 · Milestone 2] Linux 主端拉齐、首装引导与多屏精修**：
     - **已落地 ✅**：OS 原生 UI 字体栈 + Linux Cairo RGB 次像素抗锯齿 + Fontconfig 枚举 + `:font`/设置页一键Preset（移除 16.3 MB 内置 TTF）；GTK3 搜索框 2px 主题色光标与 `fcitx5` preedit 兼容占位符；8 向鼠标缩放 + KWin 6.8 Wayland 位置记忆（`Object.assign`）；Linux 可拖动 Fluent 滚动条；Wayland/X11 `PRIMARY` 零剪贴板优先取词；`.xlsx` 纯 Rust zip+xml 首表对齐表格预览；
     - **待完成 ⏳**：Linux 轻量原生零 WebView 超级面板、新用户 30 秒可跳过引导卡、混合 DPI 多屏跨屏贴图坐标加固、跨机 Argon2id + AES-256-GCM 凭据加密迁移包。
2. **Stage 2 (`v3.4.0`) · 个人长期数字资产层（Snippets / Flows 2.0 / Context）**：
   - **可读纯文本片段资产升级（`snippets.toml` 2.0）**：在原生模糊/前缀检索基础上增加快速录入/管理入口与更多动态变量，保持纯文本、Git/dotfiles 友好，永不绑定私有格式；
   - **可组合轻量工作流 Flows 2.0**：支持「划词 / 剪贴板 / 截图 / 选中文件 → 格式转换 / 翻译 / 归档 → 直接粘贴或执行」可视化串联与单步干跑调试；
   - **前沿窗口与剪贴板上下文感知**：识别前台活动应用提供定制动作（IDE 打开终端、浏览器取 URL），剪贴板新增 Token/密码类敏感文本定时自动脱敏清理开关；
   - **官方零权限纯 WASM 工具矩阵与商店静默检测**：推出 `dev.tomoyo.regex`（正则实时高亮与捕获组 `RichCard` 测试）与 `dev.tomoyo.winlayout`（零常驻键盘窗口分屏/跨屏瞬移），插件商店接入后台低频 `ETag` 304 更新提示与一键批量升级。
3. **Stage 3 (`v3.5.0`) · 本地优先的克制 AI 与 MCP 桥接（严守休眠零常驻）**：
   - **按需隔离进程 MCP Client / Agent Action Bridge**：通过独立按需子进程连接本地 Ollama、用户自备 OpenAI 兼容端点或本地 MCP 工具，将桌面上下文（划词 / 截图 / 剪贴板 / 文件）路由给合适引擎并将结果直接写回或粘贴，主内核严守零 AI 依赖与 `< 25 MB` 休眠红线；
   - **高频场景深度整合**：划词/剪贴板一键总结、报错堆栈诊断、翻译润色与「用人话生成复杂筛选/正则」，以及可选本地备忘录/高频文档增量语义检索插件（不侵入核心 Everything 毫秒索引）。
4. **Stage 4 (`v4.0.0`) · 稳定协议冻结、可信分发与三平台肌肉记忆统一**：
   - **协议冻结与开发者生态独立**：冻结长期向后兼容的 Protocol v3 与 WASM ABI，将 `tomoyo-protocol` / `tomoyo-sdk` / `tomoyo-wasm-sdk` 发布至 `crates.io`；
   - **可信代码签名与官方包源**：对接开源代码签名（如 SignPath.io）打通已预置的 `signtool.exe` Release 自动签名消除 Windows SmartScreen 拦截，正式上架 **Winget**、**Scoop** 与 **Arch AUR**；
   - **三平台闭环与 ARM64**：基于 `tomoyo-ui-macos` 推进 macOS 原生 `NSPanel` 外壳与 Windows/Linux ARM64 实测矩阵，实现同一套快捷键、片段与工作流在 Windows / Linux / macOS 间无缝延续。

---

## 负面清单（Anti-Roadmap · 永久架构红线）
1. **永不做账号登录、强制云端绑定与任何遥测上报**；
2. **永不引入常驻 WebView2 / Electron / Chromium / `tokio` / `d3d10warp`**，休眠态始终锁死 `WS ≤ 2.0 MB` 与 100 轮 `+0` 句柄泄露；
3. **永不把低频重型应用塞进核心进程**，扩展能力一律由按需隔离外部插件承接；
4. **永不靠堆砌冗长开关掩盖交互缺陷**，始终以直觉操控与聪明默认值为先；
5. **永不将用户个人配置、片段、工作流或备忘锁死在私有闭源格式中**，所有个人资产均坚持可读纯文本与开放格式，支持 Git / Syncthing 自由迁移与十年后无障碍解析。

---

## 四、每阶段固定交付节奏

每个里程碑严格执行以下五步闭环，缺一不可：
1. **需求边界与接口设计**：明确数据模型、进程归属与可卸载原工具的量化验收项；
2. **规范与单测门禁**：`node scripts/check_hygiene.mjs`（Windows 可选 `pwsh -File scripts/check_hygiene.ps1`）+ `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace`；
3. **性能与资源预算门禁**：`tomoyo --self-test --strict-budget`（Linux Private ≤ 45.0 MB / Windows WS ≤ 2.0 MB & Private ≤ 28.0 MB，100 轮压测零句柄泄漏）；
4. **实机交互验收**：在 Linux KDE Plasma Wayland/X11 与 Windows 桌面实测核心路径、边界输入与日志溯源；
5. **版本发布与本机升级**：GitHub Actions 双平台（Linux + Windows）CI + Release 全绿，生成 `SHA256SUMS` 并完成 `.tar.gz` / `.desktop` / Windows 安装包升级验收。
