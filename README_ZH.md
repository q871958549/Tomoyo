<p align="center">
  <img src="assets/tomoyo-wordmark.png" alt="Tomoyo Launcher — Open Rust Launcher" width="720" />
</p>

<p align="center">
  <strong>开源、低内存占用的 Linux &amp; Windows 原生 Rust 桌面启动器</strong><br/>
  <em>Open-Source, Ultra-Low-Memory Native Rust Command Launcher for Linux (Primary) &amp; Windows</em>
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>简体中文</strong>
</p>

<p align="center">
  <a href="https://github.com/q871958549/Tomoyo/actions/workflows/ci.yml"><img src="https://github.com/q871958549/Tomoyo/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg" alt="License" /></a>
  <img src="https://img.shields.io/badge/platform-Linux%20(CachyOS%2FArch%20KDE%20Wayland%2FX11)%20%7C%20Windows%2010%2F11-1793D1.svg" alt="Linux & Windows" />
  <img src="https://img.shields.io/badge/version-v2.6.0-7c6af7.svg" alt="Version" />
</p>

---

## 项目简介 (Overview)

**Tomoyo Launcher**（仓库：`q871958549/Tomoyo`，与 Linux 内核强制访问控制安全模块 *TOMOYO Linux* 及其他同名项目无关）使用 Rust 配合系统原生轻量 C/OS 引擎构建（主用平台：**Linux — CachyOS / Arch Linux · KDE Plasma 6 Wayland/X11**，同时完整支持 **Windows 10/11 x64 & ARM64**），侧重简洁实用的桌面检索体验与严格可控的资源占用：

1. **开源与独立运行（MIT / Apache-2.0 双协议）**：无需账号登录，不依赖商业后台服务，支持本地与自定义 JSON 插件清单，配置、数据与日志默认遵循 XDG 规范（`~/.config/tomoyo`、`~/.local/share/tomoyo`）。
2. **资源控制与休眠回收**：
   - **纯 Rust SDF + 原生图形管线**：Linux 端采用 pure-Rust 2×2 超采样 SDF + Cairo / PangoCairo 硬件友好光栅并适配 Wayland 透明图层与 `AppIndicator3` 托盘，Windows 端使用 32-bit DIB + ClearType GDI，零 `tokio` / `reqwest` / `tauri` / `wry` / Electron / `d3d10warp`。
   - **自动休眠清理**：窗口隐藏超时后释放离屏缓冲、图标缓存与缩略图，收缩 SQLite 页缓存（`PRAGMA shrink_memory`），回收空闲插件，并调用 `mimalloc` (`mi_collect`) 配合 Linux `malloc_trim(0)` 或 Windows `SetProcessWorkingSetSizeEx` 整理工作集。
   - **插件隔离**：STDIO 插件在 Linux 上使用独立进程组 + `PR_SET_PDEATHSIG` 孤儿防护（Windows 上使用 Job Object）；WASM 插件运行于 `wasmi` 燃料计量沙箱；WebView 设置中心与工作台运行在独立短生命周期进程 `tomoyo-webview-host`（Linux 基于 WebKitGTK 4.1，Windows 基于 WebView2），关闭窗口即退出释放全部内存。
3. **快速应用与全盘文件检索集成**：
   - **Linux 原生栈**：扫描 XDG `.desktop` / Flatpak / Snap 应用并用 `inotify` 零轮询增量监听，整合 KWin `/WindowsRunner` 窗口切换、`fd` + `plocate` 极速全盘文件索引与 `org.freedesktop.FileManager1` 文件定位。
   - **Windows 原生栈**：内置纯 Rust **Everything 1.4 / 1.5a** `WM_COPYDATA` IPC 客户端与 DeskBox/UWP 应用扫描。
   - 支持异步有界右侧预览（文本 8 KB 截断、目录列表与 Linux `gdk-pixbuf` / Windows WIC 全格式图片缩略图）及原生文件拖拽。
4. **界面交互与系统集成**：
   - 内置阿里巴巴普惠体 3.0 字体，支持简体中文与英文切换，提供浅色（Light）、深色（Dark）、午夜（Midnight）三种配色方案。
   - Linux 下通过 `org.kde.kglobalaccel` D-Bus 注册 Wayland 全局热键并在 X11/XWayland 回退 `XGrabKey`，Windows 下通过 `RegisterHotKey` + 按需钩子无损接管。
   - 支持拖拽窗口位置、调整大小、配置热重载（`inotify` / `SIGUSR1`）及多语言桌面通知。
5. **多运行时扩展与 uTools 导入支持**：
   - **内置 Provider**：Apps（Linux `.desktop` / Windows 应用）、全盘搜索（`fd`+`plocate` / Everything）、剪贴板历史（GTK3 / Win32 + SQLite 文本/图片/文件）、粘贴胶囊、计算与格式转换、进程管理（`/proc`）、网页搜索、窗口切换、GitHub Releases 自更新（`.tar.gz` / Inno Setup / ZIP）。
   - **STDIO 插件 (`stdio_jsonrpc`)**：通过标准输入输出 JSON-RPC 2.0 通信，支持 Rust (`tomoyo-sdk`)、Python、Go、Node.js 等语言。
   - **WASM 插件 (`wasm`)**：通过 `tomoyo-wasm-sdk` 编译，由 `wasmi` 提供燃料计数与细粒度权限审批（Linux 使用 `kdialog`/`zenity` 弹窗，Windows 使用原生弹窗）。
   - **WebView 插件 (`webview`)**：由按需拉起的 `tomoyo-webview-host` 承载，通过 `window.tomoyo` 与基础 `window.utools` 兼容层提供完整能力。

---

## 实测数据与自检阈值 (`v2.3.0` Linux / `v2.2.6` Windows 基线)

| 指标 | Linux Release (`--strict-budget`) | Windows Release (`v2.2.6`) | 自检阈值 (`--strict-budget` / `check_hygiene.mjs`) |
|---|---|---|---|
| `tomoyo` 微内核主程序体积（含原生外壳、6 槽位热键、SQLite 与 `wasmi` 宿主） | **6.85 MB** | **5.80 MB** | ≤ 7.50 MB (Linux) / ≤ 6.50 MB (Win) |
| `tomoyo-webview-host` Svelte 5 设置中心 & 3 大工作台（Linux WebKitGTK 4.1 / Windows WebView2，Gzip 嵌入，关闭即退出） | **4.96 MB**（0 MB 常驻） | **4.41 MB**（0 MB 常驻） | ≤ 5.50 MB / 0 MB 常驻 |
| `tomoyo-capture` 极简矢量标注 + 长截图 + 贴图 + 一键 OCR（Spectacle/`grim`/X11 + GTK3/Cairo / Win32 GDI） | **1.46 MB**（0 MB 常驻） | **1.15 MB**（0 MB 常驻） | ≤ 1.50 MB / 0 MB 常驻 |
| `tomoyo-plugin-ocr` / `tomoyo-plugin-notes` 官方按需 STDIO 插件（Linux Tesseract 5 / Windows WinRT OCR / Markdown 备忘录） | **0.57 MB** / **0.51 MB** | **0.38 MB** / **0.33 MB** | ≤ 0.80 MB / 0 MB 常驻 |
| 休眠态 Private Commit (`--self-test` Release) | **22.35 MB**（70 本机桌面应用索引、SQLite 与字体） | **22.03 MB**（343 条目；Working Set 1.50–1.52 MB） | ≤ 45.0 MB (Linux) / ≤ 28.0 MB (Win) |
| 100 轮查询 / 休眠 / 清理压测 Delta | **GDI `+0` · USER `+0` · Handles `+0` · Private `+0.09 MB`** | **GDI `+0` · USER `+0` · Handles `+0` · Private `+0.08 MB`** | 句柄 ≤ +2 / 内存 ≤ +0.50 MB |
| 7 道防膨胀卫生门禁（[编码规范](docs/CODING_STANDARDS.md)） | **全仓 272 源码文件 100% ≤ 450 行 · 47 IPC 指令 + 27 事件 1:1 对齐 · Clippy 0 警告 · `happy-dom` 4 产物全绿** | **SLSA Provenance + ARM64 构建** | ≤ 450 行/文件 · 0 警告 · 0 JS 异常 |

---

## 下载安装与构建 (Install & Build)

### Linux（主推平台 · CachyOS / Arch / KDE Plasma）

- **Release 压缩包**：前往 [GitHub Releases](https://github.com/q871958549/Tomoyo/releases) 下载 `Tomoyo-vX.Y.Z-linux-x86_64.tar.gz`，解压后运行 `./scripts/install-local.sh`，或使用 [`packaging/linux/PKGBUILD`](packaging/linux/PKGBUILD) 在 Arch/CachyOS 上打包安装。
- **安装系统依赖（Arch / CachyOS）**：

```bash
sudo pacman -S --needed base-devel pkgconf rustup nodejs npm \
  gtk3 webkit2gtk-4.1 libayatana-appindicator curl libsecret \
  xorg-xproto libx11 libxext libxtst dbus pipewire ffmpeg tesseract tesseract-data-eng \
  fd plocate spectacle kdialog
```

*(Ubuntu/Debian 系参见 [`.github/workflows/ci.yml`](.github/workflows/ci.yml) 中的 `apt-get` 列表。)*

- **从源码构建、本机安装与验收**：

```bash
git clone https://github.com/q871958549/Tomoyo.git
cd Tomoyo

# 1. 构建 Svelte 5 WebView 前端（仅修改 frontend/ 时必须重编）
cd frontend && npm ci && npm run check && npm test && npm run build && cd ..

# 2. 编译全工作区 Release（Rust toolchain）
cargo build --release --workspace

# 3. 安装至 ~/.local/bin 与 ~/.local/share/tomoyo，并生成桌面菜单项
./scripts/install-local.sh

# 4. 运行跨平台 7 道卫生门禁与严格内存/句柄自检
node scripts/check_hygiene.mjs --check-binaries
./target/release/tomoyo --self-test --strict-budget
./target/release/tomoyo-webview-host --self-test
./target/release/tomoyo-capture --self-test
./target/release/tomoyo-plugin-ocr --self-test
./target/release/tomoyo-cli registry check --manifest-only
```

### Windows（10 / 11 x64 & ARM64）

- 前往 [GitHub Releases](https://github.com/q871958549/Tomoyo/releases) 下载 `Tomoyo-vX.Y.Z-windows-x64-setup.exe` 或 `portable.zip`，源码构建使用：

```powershell
cd frontend && npm ci && npm run build && cd ..
cargo build --release --workspace
pwsh -File scripts\install-local.ps1
pwsh -File scripts\check_hygiene.ps1 -CheckBinaries
.\target\release\tomoyo.exe --self-test --strict-budget
```

---

## 内置指令与快捷键一览

| 前缀 / 快捷键 | 功能说明 |
|---|---|
| *(直接输入)* | 混合检索：本地应用、DeskBox 等桌面整理目录、自定义便携目录与 UWP/MSIX（支持拼音/多音字/多词交集/命中高亮与查询词记忆）、计算器、格式识别、窗口列表、Everything 置顶结果与无本地命中时的网页/全盘回退 |
| `Ctrl+Tab` / `Ctrl+1..9` / `@pdf` / `@code` | 可扩展分组筛选栏（含分类实时计数）：支持隐藏内置标签，并在设置中心按扩展名或 Everything 表达式自由扩展最多 12 个自定义标签 |
| `Ctrl+V` *(粘贴截图/文件)* | 粘贴上下文识别：对图片提供看图/Base64/定位目录，对文件提供路径复制/SHA-256 校验/打开终端，按 `Backspace` 清除 |
| `f <词>` / `ev <词>` | Everything 文件搜索（空参数列出收藏筛选器 `@today/@large/@code` 与最近搜索历史）、异步预览及拖拽导出 |
| `snip <词>` / `:reload` | 代码片段与动态模板扩展（支持 `{{date}}`/`{{datetime}}`/`{{unix}}`/`{{uuid}}`/`{{clipboard}}`） |
| `flow` / `wf` / `cleanpaste` / `shottr` / `aisum` | 声明式轻量动作管道（零脚本解释器，支持为任意工作流绑定全局热键直达，配置于可同步的 `flows.toml`）：内置 6 条开箱管道（含截图识字翻译、剪贴板去换行直贴、剪贴板转存备忘、网页速查及按需调用本地 Ollama / DPAPI OpenAI 的零常驻 `llm_transform` AI 提炼要点） |
| `ocr` / `ocr en` / `Ctrl+V` 贴图 | 按需拉起离线 OCR（Linux 基于 Tesseract 5，Windows 基于 WinRT `OcrEngine`，自适应放大与标点清理、原图对照审校编辑窗、一键翻译与单行复制，15s 自动释放） |
| `Alt+T` / `tr <文本>` / `fy <文本>` | 多引擎划词/输入/OCR 联动翻译（Linux `libcurl` + `libsecret` / Windows WinHTTP + DPAPI 加密保存 API 密钥，并行查询 Google、MyMemory+有道音标词典、DeepL、OpenAI 兼容接口与本地 Ollama，内置生词本与历史抽屉） |
| `F1` / `Alt+S` / `F3` / `cap` / `pin` / `jtocr` / `jtfy` | 原生按需截图与万能贴图插件（可在「插件管理」按需启停）：`Alt+S`/`F1` 8 手柄矢量标注与白底长截图实时预览；全局 `F3` 一键将最近复制的图片、本地图片文件、路径列表、`#HEX`/`rgb()` 色值卡或文本/代码卡片贴图置顶，0 常驻内存 |
| `Esc` *(搜索框内)* | 零提示音两段式智能回退：有输入内容时先一键清空搜索词（保留焦点继续检索），搜索框为空时再按一次静默收起窗口；彻底消除单行编辑框系统蜂鸣声 |
| `Alt+V` / `Alt+C` / `cb` / `clip` | 增强剪贴板管理中心（文本/图片/文件、语义分类、来源应用、`@pin/@img/@link/@file/@code/#标签` 与拼音检索、`Shift+Enter` 单行纯文本贴回或图片一键 OCR、`Ctrl+E` 工作台；Linux KGlobalAccel/X11 与 Windows 热键冲突自动提示与接管） |
| `note` / `memo` / `bw` | 纯外部独立官方 Protocol v2 插件 **本地 Markdown 备忘录 (`dev.tomoyo.notes`)**（`0.33 MB`–`0.51 MB`、闲置 30 秒回收 0 MB 常驻）：支持在设置中心声明式配置全局热键与目录/上限/默认标签参数，支持全文/#标签检索、`note + 标题 :: 内容` 一键新建与 `note clip` 剪贴板转存 |
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
- [`crates/tomoyo-core`](crates/tomoyo-core) — 配置读写、QueryRouter、WinHTTP/libcurl 客户端、更新检查、有界文件/PNG 缩略图预览、拼音/模糊匹配、Frecency、i18n 与内存句柄采样
- [`crates/tomoyo-providers`](crates/tomoyo-providers) — 内置数据源：Apps（Desktop/DeskBox/StartMenu/UWP/自定义目录）、Everything、Calc、Clipboard、Process、WebSearch、WindowsSwitcher、System/`:update`/`:stats`
- [`crates/tomoyo-plugin-host`](crates/tomoyo-plugin-host) — STDIO (进程组 / Job Object) + WASM (`wasmi`) + WebView 插件宿主管理
- [`crates/tomoyo-sdk`](crates/tomoyo-sdk) & [`crates/tomoyo-wasm-sdk`](crates/tomoyo-wasm-sdk) — Rust 原生与 WebAssembly 插件开发 SDK
- [`crates/tomoyo-webview-host`](crates/tomoyo-webview-host) & [`frontend/`](frontend) — 按需短生命周期 WebView 宿主与 Svelte 5 + Tailwind CSS v4 + TypeScript 极简现代前端（单文件 IIFE 编译嵌入，承载设置中心、剪贴板工作台、多源翻译、OCR 审校与 `window.utools` 兼容层）
- [`crates/tomoyo-cli`](crates/tomoyo-cli) — 插件脚手架、验证工具、SHA-256 校验、uTools 导入与插件安装 CLI
- [`crates/tomoyo-app`](crates/tomoyo-app) — 桌面启动器主程序（Linux GTK3/Cairo/Pango/KGlobalAccel/AppIndicator + Windows SDF/GDI，异步图标/预览加载、崩溃日志守护与自动休眠）
- [`crates/tomoyo-capture`](crates/tomoyo-capture) — 按需原生截图、标注、取色放大镜与桌面贴图子进程（Linux Spectacle/`grim`/X11 + GTK3/Cairo，Windows GDI/DIB，0 常驻内存）
- [`crates/tomoyo-plugin-ocr`](crates/tomoyo-plugin-ocr) — 基于 Linux Tesseract 5 / Windows WinRT `OcrEngine` 的按需离线 OCR 子进程插件（0 常驻内存）
- [`crates/tomoyo-ui-shared`](crates/tomoyo-ui-shared)、[`crates/tomoyo-ui-linux`](crates/tomoyo-ui-linux)、[`crates/tomoyo-ui-macos`](crates/tomoyo-ui-macos) — 跨平台 UI 状态模型与 Linux (XDG `.desktop` / `fd`) / macOS (`.app` / Spotlight) 适配底座
- [`packaging/`](packaging) — Linux `.desktop` / `PKGBUILD` 与 Windows Inno Setup / Winget / Scoop / MSIX 打包清单
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)、[`docs/CODING_STANDARDS.md`](docs/CODING_STANDARDS.md)、[`docs/UI_DESIGN_STANDARDS.md`](docs/UI_DESIGN_STANDARDS.md)、[`docs/ROADMAP.md`](docs/ROADMAP.md) & [`docs/PLUGIN_PROTOCOL.md`](docs/PLUGIN_PROTOCOL.md) — 架构、编码与 UI 组件规范、演进蓝图及插件通信协议文档
- [`plugins/`](plugins) & [`registry/index.json`](registry/index.json) — 多运行时示例与内置 OCR/Notes 插件及插件索引清单
- [`.github/workflows/`](.github/workflows) — Linux 主门禁（GTK3/WebKitGTK 4.1）+ Windows x64/ARM64 资源预算门禁与 Tag 触发双平台发包流水线

---

## 贡献与开源协议

欢迎通过 Issue 或 PR 参与改进或提交插件。  
本项目采用 **MIT** 或 **Apache-2.0** 双许可协议（详见 [LICENSE-MIT](LICENSE-MIT) 与 [LICENSE-APACHE](LICENSE-APACHE)）。
