<p align="center">
  <img src="assets/tomoyo-wordmark.png" alt="Tomoyo — Open Rust Launcher" width="720" />
</p>

<p align="center">
  <strong>开源、低内存占用的 Windows Rust 桌面启动器</strong><br/>
  <em>Open-Source, Low-Memory Rust Command Launcher for Windows</em>
</p>

<p align="center">
  <a href="https://github.com/q871958549/Tomoyo/actions/workflows/ci.yml"><img src="https://github.com/q871958549/Tomoyo/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg" alt="License" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D6.svg" alt="Windows" />
  <img src="https://img.shields.io/badge/version-v1.6.0-7c6af7.svg" alt="Version" />
</p>

---

## 项目简介 (Overview)

**Tomoyo** 使用 Rust 与原生 Win32 API 编写，侧重简洁实用的桌面检索体验与可控的资源占用：

1. **开源与独立运行（MIT / Apache-2.0 双协议）**：无需账号登录，不依赖商业后台服务，支持本地与自定义 JSON 插件清单。
2. **资源控制与休眠回收**：
   - 使用 Rust 2×2 超采样 SDF 抗锯齿 32-bit DIB 配合 ClearType GDI 进行界面绘制，不依赖 Electron、常驻 WebView2 或 `d3d10warp.dll`。
   - **自动休眠清理**：窗口隐藏超时后释放离屏 DIB、字体、Shell `HICON` 缓存与图片缩略图，收缩 SQLite 页缓存（`PRAGMA shrink_memory`），回收空闲插件，并调用 `mimalloc` (`mi_collect`) 与 `SetProcessWorkingSetSizeEx` 整理工作集。
   - **插件隔离**：STDIO 插件绑定 Windows Job Object（限制内存并随主进程退出清理）；WASM 插件运行于 `wasmi` 燃料计量沙箱；WebView 设置中心与插件运行在独立短生命周期进程 `tomoyo-webview-host.exe`，关闭窗口即退出释放内存。
3. **Everything 文件检索集成**：
   - 内置纯 Rust 实现的 **Everything 1.4 / 1.5a** `WM_COPYDATA` IPC 客户端，无需额外 DLL。
   - 支持异步有界右侧预览（文本 8 KB 截断、目录列表与 Win32 WIC 全格式图片缩略图）及原生 Windows OLE 文件拖拽。
4. **界面交互与多语言支持**：
   - 内置阿里巴巴普惠体 3.0 字体，支持简体中文与英文切换，提供浅色（Light）、深色（Dark）、午夜（Midnight）三种配色方案，并同步 DWM 标题栏样式与自定义弹出菜单。
   - 支持拖拽窗口位置、八向边缘缩放与预览分割线调整，配置自动保存并支持热重载。
5. **多运行时扩展与 uTools 导入支持**：
   - **内置 Provider**：本地应用（含 UWP/MSIX、DeskBox 等桌面整理目录、自定义便携目录、拼音与首字母匹配）、Everything、剪贴板历史（文本 + 图片）、粘贴胶囊上下文动作、计算与格式转换、进程管理、网页搜索、窗口切换及 GitHub Releases 检查更新。
   - **STDIO 插件 (`stdio_jsonrpc`)**：通过标准输入输出 JSON-RPC 2.0 通信，支持 Rust (`tomoyo-sdk`)、Python、Go、Node.js 等语言编写。
   - **WASM 插件 (`wasm`)**：通过 `tomoyo-wasm-sdk` 编译，由 `wasmi` 提供燃料计数与细粒度能力限制。
   - **WebView 插件 (`webview`)**：由按需拉起的 `tomoyo-webview-host.exe` 承载，提供 `window.tomoyo` 与基础 `window.utools` API 兼容层（含 `setSubInput`、`db`/`dbStorage`、`hideMainWindowPasteText`、`Ctrl+Shift+T` 置顶等）以及 `tomoyo-cli import-utools` 导入工具。

---

## 实测数据与自检阈值 (`v1.6.0` Release)

| 指标 | 实测记录 | 自检阈值 (`--strict-budget`) |
|---|---|---|
| `tomoyo.exe` 主程序体积 | **5.48 MB** (`5,614.5 KB`) | — |
| `tomoyo-webview-host.exe` Svelte 5 极简现代设置、剪贴板、多源翻译、OCR 与插件宿主 | **4.25 MB**（关闭即退出） | 0 MB 常驻 |
| `tomoyo-capture.exe` 原生按需截图、AA SDF 矢量工具栏、6 种标注、取色与贴图 | **0.95 MB**（关闭即退出） | 0 MB 常驻 |
| `tomoyo-plugin-ocr.exe` 离线 WinRT OCR 插件 | **0.36 MB**（空闲 15 秒自动回收） | 0 MB 常驻 |
| 休眠态 Working Set 内存占用 (`--self-test`) | **1.49–1.52 MB** | ≤ 2.0 MB |
| 休眠态 Private Commit（含 345 应用/桌面条目索引、SQLite 与字体） | **21.39 MB** | ≤ 28.0 MB |
| 100 轮查询 / 绘制 / 预览 / 休眠压测 Delta | **GDI `+0` · USER `+0` · Handles `+0` · Private `-0.03 MB`** | 句柄 0 增长 / 内存 ≤ +0.50 MB |
| 源码规范与前端编译产物门禁（[编码规范](docs/CODING_STANDARDS.md) / [UI 规范](docs/UI_DESIGN_STANDARDS.md)） | **全仓 180 个源码文件均 ≤ 450 行 · `svelte-check`/`Vitest`/4 份 Svelte 5 单文件产物 `happy-dom` 门禁全绿** | ≤ 450 行/文件 · 0 JS 异常 |

---

## 下载安装与构建 (Install & Build)

- **安装包与便携版**：前往 [GitHub Releases](https://github.com/q871958549/Tomoyo/releases) 下载 `Tomoyo-vX.Y.Z-windows-x64-setup.exe`（默认安装至 `%LOCALAPPDATA%\Programs\Tomoyo`，无需管理员权限）或 `portable.zip`。
- **从源码构建与验证**：

```powershell
# 编译 Svelte 5 极简现代前端（仓库已内置编译产物，仅修改前端源码时需要）
cd frontend && npm ci && npm run build && cd ..

# 克隆仓库并编译 Release
git clone https://github.com/q871958549/Tomoyo.git
cd Tomoyo
cargo build --release --workspace

# 安装到本机用户目录
pwsh -File scripts\install-local.ps1

# 运行规范检查与严格内存/句柄预算自检
pwsh -File scripts\check_hygiene.ps1
.\target\release\tomoyo.exe --self-test --strict-budget
.\target\release\tomoyo-webview-host.exe --self-test
.\target\release\tomoyo-capture.exe --self-test
```

---

## 内置指令与快捷键一览

| 前缀 / 快捷键 | 功能说明 |
|---|---|
| *(直接输入)* | 混合检索：本地应用、DeskBox 等桌面整理目录、自定义便携目录与 UWP/MSIX（支持拼音/多音字/多词交集/命中高亮与查询词记忆）、计算器、格式识别、窗口列表、Everything 置顶结果与无本地命中时的网页/全盘回退 |
| `Ctrl+Tab` / `Ctrl+1..9` / `@pdf` | 分组筛选栏（含分类实时计数）：按 `全部 / 应用 / 文件夹 / PDF / Excel / Word / PPT / 图片 / 压缩` 过滤结果并下推至 Everything |
| `Ctrl+V` *(粘贴截图/文件)* | 粘贴上下文识别：对图片提供看图/Base64/定位目录，对文件提供路径复制/SHA-256 校验/打开终端，按 `Backspace` 清除 |
| `f <词>` / `ev <词>` | Everything 文件搜索（空参数列出收藏筛选器 `@today/@large/@code` 与最近搜索历史）、异步预览及拖拽导出 |
| `snip <词>` / `:reload` | 代码片段与动态模板扩展（支持 `{{date}}`/`{{datetime}}`/`{{unix}}`/`{{uuid}}`/`{{clipboard}}`） |
| `ocr` / `ocr en` / `Ctrl+V` 贴图 | 按需拉起原生 WinRT 离线 OCR（自适应 2× 放大与标点清理、支持免贴图直识最近截图、原图对照审校编辑窗、一键翻译与单行复制，15s 自动释放） |
| `Alt+T` / `tr <文本>` / `fy <文本>` | 多引擎划词/输入/OCR 联动翻译（原生 WinHTTP 并行查询 Google、MyMemory+有道音标词典、DeepL、OpenAI 兼容接口与本地 Ollama，Windows DPAPI 加密保存 API 密钥，无损恢复剪贴板且不污染历史，内置生词本与历史抽屉） |
| `Alt+S` / `cap` / `pin` / `jtocr` / `jtfy` | 原生按需截图、AA SDF 矢量图标胶囊工具栏、3 档线宽 + 6 色板、6 种标注（矩形/椭圆/箭头/画笔/马赛克/序号 + Undo/Redo）、`11×11` 像素放大镜（`C` 复制、`Shift` 切换 HEX/RGB）、无边框置顶贴图与一键 OCR/翻译，关闭即释放 100% 内存 |
| `Alt+V` / `cb <词>` | 增强剪贴板管理中心（文本/图片/文件、语义分类、来源应用、`@pin/@img/@link/@file/@code/#标签` 与拼音检索、`Shift+Enter` 单行纯文本贴回或图片一键 OCR、`Ctrl+E` 工作台、`Ctrl+D` 删除、`Ctrl+Shift+P` 置顶，自动屏蔽密码管理器机密剪贴板） |
| `kill <词>` / `ps <词>` | 进程列表查看与终止进程（按内存占用降序排列） |
| `g` / `gh` / `crates` / `docs` / `bd` / `zh` / `mdn` | 可在设置中心自定义的快捷网页搜索前缀 |
| `w <词>` | 已打开窗口列表筛选与切换 |
| `:update` | 检查 GitHub Releases 更新、查看更新说明并跳转下载 |
| `:settings` / `Ctrl+,` | 打开设置中心（快捷键录入、自定义应用目录、分组栏开关、主题与语言、插件管理、诊断日志、更新检查） |
| `:logs` / `:stats` / `:plugins` / `:gc` | 查看运行/WebView/崩溃日志与诊断报告、进程内存/句柄/插件状态，或手动执行休眠清理 |
| `:theme dark\|light\|midnight` / `:lang zh\|en` | 切换配色主题或界面语言 |
| `hi` / `py` / `wasm` / `jwt` / `unit` / `json` | 内置参考插件示例（Rust STDIO / Python STDIO / WASM / WebView） |
| `双击左键` / `Ctrl+Enter` / `Shift+Enter` / `Ctrl+Shift+Enter` / `Ctrl+P` / `Tab` / `Alt+1..8` | 双击打开 / 定位目录 / 终端或复制路径 / 以管理员身份运行 / 预览面板 / 动作菜单 / 快捷键直达 |

---

## 仓库架构 (Workspace Layout)

- [`crates/tomoyo-protocol`](crates/tomoyo-protocol) — 插件 Manifest、JSON-RPC、筛选枚举、注册表 Schema 与 UI 原语定义（无 OS 依赖）
- [`crates/tomoyo-everything`](crates/tomoyo-everything) — Everything 1.4 / 1.5a `WM_COPYDATA` IPC 客户端
- [`crates/tomoyo-core`](crates/tomoyo-core) — 配置读写、QueryRouter、WinHTTP 客户端、更新检查、有界文件/PNG 缩略图预览、拼音/模糊匹配、Frecency、i18n 与内存句柄采样
- [`crates/tomoyo-providers`](crates/tomoyo-providers) — 内置数据源：Apps（Desktop/DeskBox/StartMenu/UWP/自定义目录）、Everything、Calc、Clipboard、Process、WebSearch、WindowsSwitcher、System/`:update`/`:stats`
- [`crates/tomoyo-plugin-host`](crates/tomoyo-plugin-host) — STDIO (Job Object) + WASM (`wasmi`) + WebView 插件宿主管理
- [`crates/tomoyo-sdk`](crates/tomoyo-sdk) & [`crates/tomoyo-wasm-sdk`](crates/tomoyo-wasm-sdk) — Rust 原生与 WebAssembly 插件开发 SDK
- [`crates/tomoyo-webview-host`](crates/tomoyo-webview-host) & [`frontend/`](frontend) — 按需短生命周期 WebView2 宿主与 Svelte 5 + Tailwind CSS v4 + TypeScript 极简现代前端（单文件 IIFE 编译嵌入，承载设置中心、剪贴板工作台、多源翻译、OCR 审校与 `window.utools` 兼容层）
- [`crates/tomoyo-cli`](crates/tomoyo-cli) — 插件脚手架、验证工具、SHA-256 校验、uTools 导入与插件安装 CLI
- [`crates/tomoyo-app`](crates/tomoyo-app) — Windows 桌面启动器主程序（SDF + GDI 渲染、异步图标/预览加载、自定义弹出菜单、DPI 重缩放、`TaskbarCreated` 托盘自愈与崩溃滚动日志守护）
- [`crates/tomoyo-capture`](crates/tomoyo-capture) — 按需原生截图、6 种标注、取色放大镜与桌面贴图子进程（`tomoyo-capture.exe`，0 常驻内存）
- [`crates/tomoyo-plugin-ocr`](crates/tomoyo-plugin-ocr) — 基于 Windows 10/11 原生 WinRT `OcrEngine` 的按需离线 OCR 子进程插件（0 常驻内存）
- [`crates/tomoyo-ui-shared`](crates/tomoyo-ui-shared)、[`crates/tomoyo-ui-macos`](crates/tomoyo-ui-macos)、[`crates/tomoyo-ui-linux`](crates/tomoyo-ui-linux) — 跨平台 UI 状态模型与 macOS (`.app` / Spotlight) / Linux (XDG `.desktop` / `fd`) 适配底座
- [`packaging/`](packaging) — Inno Setup 安装包、Winget / Scoop 清单与 MSIX 打包脚本
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)、[`docs/CODING_STANDARDS.md`](docs/CODING_STANDARDS.md)、[`docs/UI_DESIGN_STANDARDS.md`](docs/UI_DESIGN_STANDARDS.md)、[`docs/ROADMAP.md`](docs/ROADMAP.md) & [`docs/PLUGIN_PROTOCOL.md`](docs/PLUGIN_PROTOCOL.md) — 架构、编码与 UI 组件规范、演进蓝图及插件通信协议文档
- [`plugins/`](plugins) & [`registry/index.json`](registry/index.json) — 5 个多运行时示例与内置 OCR 插件及插件索引清单
- [`.github/workflows/`](.github/workflows) — Windows 资源预算门禁 + Linux/macOS 跨平台编译 CI 与 Tag 触发发包流水线

---

## 贡献与开源协议

欢迎通过 Issue 或 PR 参与改进或提交插件。  
本项目采用 **MIT** 或 **Apache-2.0** 双许可协议（详见 [LICENSE-MIT](LICENSE-MIT) 与 [LICENSE-APACHE](LICENSE-APACHE)）。
