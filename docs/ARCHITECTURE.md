# Tomoyo 架构说明 (Architecture Overview)

![Tomoyo Wordmark](../assets/tomoyo-wordmark.png)

> **设计目标**：使用 Rust 与原生 Win32 API 构建开源桌面启动器，支持 Everything 文件搜索与多语言插件隔离运行，同时将闲置后台资源占用保持在较低水平。

---

## 一、设计背景与解决的问题

### 1.1 核心功能对标
| 维度 | 目标能力 | Tomoyo 实现策略 |
|---|---|---|
| **窗口呼出** | 全局快捷键（`Alt+Space`）唤起与隐藏 | 原生 Win32 窗口 + 纯 Rust SDF / ClearType GDI 绘制，无 Electron/浏览器常驻冷启动开销 |
| **混合检索** | 应用启动、拼音/首字母匹配与高频排序 | 全拼/首字母匹配 + 时间衰减 Frecency + DeskBox/自定义目录扫描 + 计算与窗口切换 |
| **文件搜索** | 快速检索本地文件 | 原生 Everything 1.4 / 1.5a `WM_COPYDATA` IPC 客户端，支持分类筛选下推与动作面板 |
| **上下文匹配** | 对剪贴板/输入中的时间戳、颜色、表达式、路径作出响应 | 内置轻量 `IntentDetector` 与 `Super Match` 粘贴胶囊处理 |
| **扩展能力** | 兼顾列表型扩展与富交互面板 | 分层扩展方案：内置 Rust Provider + STDIO JSON-RPC + WASM (`wasmi`) + 按需 WebView 宿主 |

### 1.2 常见启动器开销来源
1. **常驻进程内存偏高**：Electron 类应用后台通常常驻 Main + Renderer + GPU 等多个进程。
2. **封闭商业分发**：部分软件绑定账号体系或闭源市场。
3. **插件缺乏资源上限**：插件与主界面耦合时，异常扩展容易持续占用主进程内存。
4. **语言栈受限**：只支持 Node/JS 时，Rust / Python / Go / C# 等语言开发者较难直接编写扩展。

---

## 二、系统架构

### 2.1 模块分层

```mermaid
flowchart TB
    subgraph UI["表现层 · Native Shell (tomoyo-app)"]
        Hotkey["全局热键 & 系统托盘<br/>(Win32 Native)"]
        SearchBar["原生搜索框<br/>(Borderless EDIT · 中文 IME 支持)"]
        Renderer["纯 Rust SDF 抗锯齿 + ClearType GDI 渲染器<br/>(Win11 圆角 · 无 D3D/WARP · DPI 感知)"]
        IconCache["异步有界图标缓存<br/>(后台提取 HICON -> DrawIconEx · LRU<=96)"]
        Hibernator["休眠回收调度<br/>(隐藏超时释放 DIB/字体/HICON + WorkingSet Trim)"]
    end

    subgraph Core["核心调度引擎 (tomoyo-core)"]
        Router["QueryRouter<br/>(代际取消令牌 · 快慢两阶段路由 · 分组筛选)"]
        Matcher["Pinyin + Fuzzy 匹配器<br/>(全拼/首字母/子序列)"]
        Frecency["Frecency 排序引擎<br/>(时间衰减 · 有界持久化)"]
        Intent["Context Intent Detector<br/>(数学/时间戳/颜色/URL/JSON/路径)"]
        Telemetry["内存与句柄采样<br/>(WorkingSet/PrivateBytes/GDI/Handles)"]
    end

    subgraph Builtin["内置 Providers (tomoyo-providers)"]
        EV["Everything 原生 IPC<br/>(WM_COPYDATA · 1.4 & 1.5a)"]
        Apps["应用索引<br/>(桌面/DeskBox/开始菜单/UWP/Registry/自定义目录)"]
        Calc["计算/进制/时间戳/编码"]
        Clip["有界剪贴板历史<br/>(SQLite + 事件监听)"]
        Win["实时窗口切换器"]
        Sys["系统命令 & :stats 诊断"]
    end

    subgraph Ext["多运行时插件架构 (tomoyo-plugin-host)"]
        Host["PluginHost<br/>按需拉起 · 空闲回收 · Windows Job Object 限额"]
        RPC["Stdio JSON-RPC 2.0 + SDUI<br/>(Rust / Python / Go / Node / C#)"]
        WASM["wasmi 能力沙箱<br/>(燃料计量 · 线性内存上限 · 显式权限)"]
        WebView["按需独立 WebView 宿主<br/>(tomoyo-webview-host.exe · uTools API 兼容层)"]
    end

    Hotkey --> SearchBar
    SearchBar --> Router
    Router --> Renderer
    Renderer --> IconCache
    Hibernator -.->|释放 Surface / Trim| Renderer

    Router --> Matcher
    Router --> Frecency
    Router --> Intent
    Router --> Builtin
    Router --> Host
    Telemetry --> Renderer

    Host --> RPC
    Host -.-> WASM
    Host -.-> WebView
```

### 2.2 内存控制与防泄露设计

> [!NOTE]
> 在早期性能剖析中，`ID2D1DCRenderTarget` 会隐式加载 `d3d10warp.dll` 软件光栅化器，导致 Private Commit 增加约 51.4 MB、内核句柄增加 219 个。因此渲染路径统一采用 **纯 Rust SDF 抗锯齿 32-bit DIB + 原生 ClearType GDI + `DrawIconEx`** 方案。

1. **活跃态与休眠态切换**：
   - **活跃态（窗口显示）**：按需创建 32-bit 双缓冲 DIB 与 ClearType 字体，由后台线程异步提取 Shell 图标与预览内容，避免阻塞 UI 线程。
   - **休眠态（隐藏超时）**：释放双缓冲 DIB、GDI 字体、缓存 `HICON` 与图片预览，收缩 SQLite 页缓存，裁剪结果缓冲并通知工作线程及主线程执行 `mi_collect(true)` 归还 `mimalloc` 页，保持 0% 空闲 CPU 唤醒（消息阻塞等待 `GetMessageW`）。
2. **有界数据结构**：
   - 图标 LRU 缓存上限 96 项（`HICON` 通过 `Drop` 调用 `DestroyIcon`）。
   - 剪贴板缓存上限有界且单条文本与图片均限制尺寸。
   - 每次按键分配单调递增 `generation: u64`，旧查询结果直接丢弃。
3. **插件资源隔离**：
   - 外部插件按需启动，绑定 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 与 `JOB_OBJECT_LIMIT_PROCESS_MEMORY`（默认 128 MB 上限）。
   - 闲置超时后自动终止并清理，无活跃插件时回收线程通过 `Condvar` 挂起。
4. **运行时可观测性**：
   - 主界面输入 `:stats` 可实时查看 Private Bytes、Working Set、GDI/USER/内核句柄数量及活跃插件进程信息；`:gc` 可手动触发休眠整理。

---

## 三、插件通信模型 (`tomoyo-protocol` + `tomoyo-sdk`)

```mermaid
sequenceDiagram
    participant User as 用户输入
    participant Core as tomoyo-core
    participant Host as tomoyo-plugin-host
    participant Job as Windows Job Object
    participant Plugin as 外部插件进程 (Rust/Py/Go/JS)

    User->>Core: 输入 "hi world"
    Core->>Host: 匹配关键词 "hi" (tomoyo/query)
    alt 插件未启动 (Lazy Spawn)
        Host->>Job: 创建 Job Object (128MB Cap + KillOnClose)
        Host->>Plugin: 启动子进程 (Stdio JSON-RPC 2.0)
        Host->>Plugin: tomoyo/handshake
        Plugin-->>Host: capabilities & protocol v1
    end
    Host->>Plugin: tomoyo/query { raw, args, clipboard, limit }
    Plugin-->>Host: ResultItem[] (含 SDUI 预览 & Actions)
    Host-->>Core: 合并排序并渲染
    User->>Core: 按 Enter 执行 Action
    Host->>Plugin: tomoyo/execute { action_id, payload }
    Plugin-->>Host: ExecuteResponse (CopyText / OpenUrl / Toast / ViewModel)
    Note over Host,Plugin: 空闲超时后 Idle Reaper 发送 tomoyo/shutdown 并回收进程
```

- **跨语言支持**：只需读写 `stdin/stdout` JSON 行文本即可编写插件，可用 Python/Node/Go 或 `tomoyo-sdk`（Rust）实现。
- **声明式结果（SDUI）**：列表插件通过结构化 `ResultItem` 与 `PreviewSpec` 即可呈现，无需单独启动浏览器进程。
- **沙箱与按需 WebView 补充**：纯计算插件可选 `wasm`（`tomoyo-wasm-sdk` + `wasmi`），复杂交互界面则通过短生命周期 `tomoyo-webview-host.exe` 承载并在关闭后退出。

---

## 四、Cargo Workspace 模块划分

| Crate / 目录 | 职责 | 关键依赖与约束 |
|---|---|---|
| `crates/tomoyo-protocol` | 插件 Manifest、JSON-RPC 2.0、查询/筛选/Action/SDUI 类型 | 纯 Rust + `serde`，无 OS 依赖 |
| `crates/tomoyo-everything` | Everything 1.4 / 1.5a `WM_COPYDATA` IPC 客户端 | 无第三方 DLL 依赖，带查询超时保护 |
| `crates/tomoyo-core` | 配置、QueryRouter（两阶段检索与筛选）、Pinyin/Fuzzy、Frecency、Intent、WinHTTP/更新检查、遥测 | 不引入重型异步运行时，容器有界 |
| `crates/tomoyo-providers` | 内置 Providers：Everything、Apps、Calc、Clipboard、Process、WebSearch、Windows、System | 原生 Win32 API + `pinyin` |
| `crates/tomoyo-plugin-host` | 插件发现、按需加载、Job Object 隔离、JSON-RPC、`wasmi`、Idle Reaper | 进程与沙箱隔离 |
| `crates/tomoyo-sdk` | Rust STDIO 插件开发包 (`TomoyoPlugin` trait) | 依赖 `tomoyo-protocol` |
| `crates/tomoyo-wasm-sdk` | WebAssembly 插件开发包 | 依赖 `tomoyo-protocol` |
| `crates/tomoyo-webview-host` | 按需独立 WebView2 宿主（设置中心、插件清单视图、`window.utools` 兼容桥） | 独立短生命周期进程 |
| `crates/tomoyo-cli` | 插件脚手架、检查、SHA-256 计算与 uTools 导入工具 | 命令行工具 |
| `crates/tomoyo-app` | Windows 桌面主程序（Win32 + SDF/GDI + DPI + 托盘自愈 + CrashGuard + 休眠） | `windows` crate + `mimalloc` |
| `crates/tomoyo-plugin-ocr` | 按需 Windows WinRT 离线 OCR 子进程插件 (`tomoyo-plugin-ocr.exe`) | 原生 WinRT `OcrEngine`，0 常驻内存 |
| `crates/tomoyo-ui-shared` / `macos` / `linux` | 跨平台状态模型及 macOS / Linux 应用与文件后端适配层 | 无 OS 硬依赖 + 平台适配钩子 |
| `plugins/example-*` & `builtin-ocr` | Rust / Python / WASM / WebView / WinRT OCR 参考示例 | 用于插件接口与回归验证 |

---

## 五、版本演进记录

- **v0.1.0 – v0.3.0 — 基础架构与插件体系**
  - 完成 Workspace 分层、`tomoyo-everything` IPC、内置 Providers、Job Object 隔离的 STDIO 插件、WASM 沙箱 (`wasmi`)、独立 WebView 宿主 (`tomoyo-webview-host.exe`)、预览面板、SQLite 剪贴板历史、Windows OLE 文件拖出与 `tomoyo-cli`。
- **v0.4.0 — 设置中心、图片剪贴板与自动化测试**
  - 引入阿里巴巴普惠体 3.0 与中英文切换、WebView2 设置中心、GitHub Releases 检查更新、图片剪贴板历史、Super Match 粘贴上下文与 GitHub Actions CI/Release 流水线。
- **v0.5.0 – v0.5.1 — 模块化拆分、内存优化与安装包**
  - 建立 `CODING_STANDARDS.md` 与 `check_hygiene.ps1` 检查（单文件 ≤ 450 行），拆分过长源码文件为独立子模块。
  - 休眠阶段接入 `mimalloc` (`mi_collect`) 与查询期延迟计算，添加 Inno Setup 免 UAC 安装包与 `--self-test --strict-budget` 预算门禁。
- **v0.6.0 — 休眠唤醒响应、桌面整理索引与分组筛选**
  - 图标提取与预览解码全面后台线程化，解决唤醒瞬间卡顿，并在后台线程释放大缓冲区与休眠期同步调用 `mi_collect(true)` 保持私有内存稳定在 ~20 MB。
  - 修复搜索结果左键双击无响应问题（补充 `CS_DBLCLKS` 与 `WM_LBUTTONDBLCLK` 处理）。
  - 新增自绘主题弹出菜单，替换 Logo、结果行与托盘右键菜单为符合当前配色与字体的统一样式。
  - 增强应用扫描：自动识别 DeskBox 托管桌面目录、公共桌面、完整开始菜单与自定义便携目录，读取 `.lnk` 原生目标可执行文件名，修复 `App Paths` 解析。
  - 新增搜索分组筛选栏（`全部 / 应用 / 文件夹 / PDF / Excel / Word / PPT / 图片 / 压缩`），支持 `@tag` 内联过滤、`Ctrl+Tab` / `Ctrl+1..9` 快捷键与 Everything 下推筛选。
- **v0.6.1 – v1.0.0 — 异常退出治理、自更新闭环、效率功能完善与跨平台分发底座**
  - **稳定性与生命周期治理**：新增 `crash_guard.rs`（256 KB 滚动日志 + panic/Win32 SEH 捕获 `last_crash.txt`）、`TaskbarCreated` 托盘重建、单实例窗口唤醒、退出时 SQLite WAL checkpoint 与独立任务分离启动。
  - **v0.6.x 体验闭环**：WinHTTP 流式下载 `setup.exe` + `SHA256SUMS.txt` 校验并自动启动覆盖升级；`WM_DPICHANGED` 多屏 DPI 动态字体与窗口重缩放；全局热键冲突提醒；`FindFirstChangeNotificationW` 零轮询应用目录增量刷新；重定向 KnownFolder 桌面扫描。
  - **v0.7.0 安全生态**：`PermissionStore`（`permissions.json`）敏感权限首次弹窗审批与设置中心热撤权管理；纯 Rust PKZIP + `flate2` 解析器实现本地/远程社区 `.zip` 一键 SHA-256 校验安装。
  - **v0.8.0 效率工具与离线 OCR**：`SnippetsProvider` 动态模板片段库（`snippets.toml`）、Everything 最近搜索历史（`ev_history.json`）与收藏筛选器，以及基于系统 WinRT `OcrEngine` 的 0 常驻内存离线 OCR 插件（`tomoyo-plugin-ocr.exe`）。
  - **v1.0.0 分发与跨平台**：新增 Winget / Scoop / MSIX 分发清单与 `tomoyo-ui-shared` / `tomoyo-ui-macos` / `tomoyo-ui-linux` 适配层（Windows 10/11 x64 全套实机验证通过，macOS/Linux 适配层纳入 CI 跨平台编译测试）。
- **v1.0.1 — Everything 1.4 协议修复、设置中心受控组件重构与 UI 字体对比度优化**
  - **搜索结果完整性修复**：修复 `tomoyo-everything` 三个底层 IPC 问题（`HWND_MESSAGE` 改为隐藏顶层接收窗口、`SMTO_ABORTIFHUNG` 改为 `SMTO_NORMAL`、`EVERYTHING_IPC_LIST2` 头部由 28 字节修正为实际 20 字节），并在 `EverythingProvider` 引入快速索引排序、`startwith:` 二次合并与路径权重重排（桌面/DeskBox 提权、隐藏点目录与缓存目录降权）。
  - **设置中心单实例与受控输入**：设置中心新增命名互斥体单实例唤起；移除全部裸 `<input type="number">` 与路径 `<textarea>`，替换为分段深度选择、原生 Win32 `IFileOpenDialog` 目录浏览、范围滑块、Everything 排除标签组与带协议校验的搜索模板配置；Rust 后端 `TomoyoConfig::sanitize()` 在加载与保存时统一执行边界钳制与高危目录/协议拦截。
  - **界面清晰度与设计规范**：修复私有字体因 GDI 伪粗体（`FW_BOLD`/`FW_SEMIBOLD`/`FW_MEDIUM`）导致的小字模糊问题，统一使用 `FW_NORMAL` 原生字形，引入三级文字对比度与柔和选中态标签，并新增 [`docs/UI_DESIGN_STANDARDS.md`](UI_DESIGN_STANDARDS.md)。
- **v1.1.0 — 查询词绑定记忆、命中字符高亮、多音字与跨源去重**
  - **检索引擎升级**：`FrecencyStore` 新增有界查询词绑定记忆（`query -> item`，上限 400 词 × 8 项，14 天半衰期）；`IndexedText` 支持多音字（`pinyin` `heteronym`）与空格多词跨字段交集匹配；`QueryRouter` 合并 `Apps` 与 `Everything` 时按规范化文件系统路径去重，并在全局零结果时提供默认网页搜索与 `f ` 全盘检索回退项。
  - **可解释高亮与快捷操作**：新增 `highlight_title_ranges` 与 `draw_text_highlighted_gdi`（严格 `FW_NORMAL` 零伪粗体），实时高亮标题中的文本/拼音/多音字/首字母命中区间；筛选栏标签展示分类实时命中数；空输入框按历史高频应用排序；新增 `Shift+Enter` 快捷次动作（文件/目录在此处打开终端，应用复制路径），并建立 `ranking_golden.rs` 排序回归测试集与长期规划 [`docs/ROADMAP.md`](ROADMAP.md)。
- **v1.2.0 — 统一多进程日志溯源与离线 OCR 根治重构**
  - **全链路日志与崩溃溯源**：在 `tomoyo-core` 抽离公共 `diagnostics` 模块（1 MB × 3 主进程轮转 `tomoyo.log`、512 KB × 2 宿主轮转 `webview.log`、单调 `trace_id`、`last_crash.txt` panic/SEH 快照与脱敏诊断报告生成）；`tomoyo-plugin-host` 接管全部子进程 `stderr`（12 行环形缓冲）写入主日志，并在插件启动/查询失败时向主列表返回附带 `trace_id` 的错误卡片；`tomoyo-webview-host` 接入独立日志、崩溃守护及前端 JS `error`/`unhandledrejection` 上报；修复设置中心滑块拖动触发全量热重载刷屏日志的问题；新增 `:logs` 指令与设置中心「诊断与日志」面板。
  - **OCR 死循环修复、精度优化与审校窗口**：修复 `QueryRouter` 粘贴图片胶囊拦截显式 `ocr` 关键词的路由缺陷；`tomoyo-plugin-ocr` 引入 WinRT `BitmapTransform` 自适应 2× Cubic 放大、中文空格清理与数字/IP/时间全角标点规范化，支持 `ocr en`/`zh`/`raw` 及免贴图识别最近剪贴板截图；新增按需启动的「OCR 审校与编辑窗口」（左原图缩放对照 + 右可编辑文本，关闭释放 100% 内存）及主列表单行直拷卡片。
- **v1.3.0 — 增强剪贴板管理中心（替代 CopyQ）与 Win32 WIC 全图预览**
  - **剪贴板存储与筛选引擎**：`ClipboardStore` 升级 SQLite v2 模式（`subtype`、`tag`、`note`、`source_app`、`file_paths` 与联合索引），支持文本语义分类（链接/色值/路径/代码/文本）、截图图片与复制文件（`CF_HDROP`）、`@pin/@img/@text/@link/@file/@code/@color/#标签` 过滤、拼音多词匹配、单条编辑/删除、清空非置顶与 JSON 备份导出/导入。
  - **专属热键、隐私保护与工作台**：新增独立剪贴板全局快捷键（默认 `Alt+V`）与 `cb` 模式快捷键（`Shift+Enter` 单行纯文本贴回 / 图片一键 OCR、`Ctrl+E` 工作台、`Ctrl+D` 删除、`Ctrl+Shift+P` 置顶）；新增 Windows 机密剪贴板格式自动拦截与来源应用隐私屏蔽名单（设置中心落盘「下拉搜索框」组件）；新增按需单实例三栏「剪贴板工作台」（`__tomoyo_clipboard_manager__`，关闭即释放 100% WebView 内存）以及 `tomoyo-core::wic_thumb` Win32 WIC 全格式图片缩略图解码。
