<p align="center">
  <img src="assets/tomoyo-wordmark.png" alt="Tomoyo — Open Rust Launcher" width="720" />
</p>

<p align="center">
  <strong>100% 开源 · 超低内存 · 零后台泄露的 Rust 桌面效率启动器与开放扩展基座</strong><br/>
  <em>100% Open-Source, Ultra-Low-Memory Rust Command Launcher & Extensible Productivity Hub</em>
</p>

<p align="center">
  <a href="https://github.com/q871958549/Tomoyo/actions/workflows/ci.yml"><img src="https://github.com/q871958549/Tomoyo/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg" alt="License" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D6.svg" alt="Windows" />
  <img src="https://img.shields.io/badge/version-v0.4.0-7c6af7.svg" alt="Version" />
</p>

---

## 核心理念 (Core Philosophy)

**Tomoyo** 从零使用纯 Rust 与原生 Win32 架构打造，致力于彻底替代基于 Electron、长期驻留内存动辄 300–600 MB 的传统启动器（如 uTools）：

1. **100% 开源（MIT / Apache-2.0 双协议）** — 无账号登录墙、无闭源商业核心、无遥测数据上传、去中心化社区插件清单。
2. **极低内存与防泄露底线（Zero-Leak Architecture）**：
   - 纯 Rust + Win32 + 自研 **2×2 超采样 SDF 抗锯齿 32-bit DIB + ClearType GDI** 渲染管线（彻底摒弃 Electron、常驻 WebView2 与 `d3d10warp.dll` 内存膨胀）。
   - **自动休眠机制 (Auto-Hibernation)**：隐藏后自动销毁绘图表面、字体、Shell `HICON` 缓存、图片缩略图、收缩 SQLite 页缓存（`PRAGMA shrink_memory`）、回收闲置 STDIO/WASM 插件并执行操作系统工作集整理（`SetProcessWorkingSetSizeEx`）。
   - **系统级插件硬隔离**：STDIO 插件绑定 Windows Job Object（硬限内存 + 退出强杀）；WASM 插件运行于 `wasmi` 燃料计量与零默认权限能力沙箱；WebView 插件与图形化设置中心运行在独立短生命周期进程 `tomoyo-webview-host.exe` 中，关闭即彻底销毁 0 常驻内存。
3. **原生深度集成 Everything**：
   - 纯 Rust 实现 **Everything 1.4 & 1.5a** 原生 `WM_COPYDATA` IPC 客户端，**零第三方 DLL 依赖**。
   - 右侧有界实时预览（支持代码/文本 8 KB 截断、目录列表与 **PNG 图片原生缩略图**）+ **原生 Windows OLE 文件拖拽**（直接从结果列表拖入微信、VS Code 或资源管理器）。
4. **中文体验与现代美学**：
   - 内置官方免费 **阿里巴巴普惠体 3.0**（简体中文主语言 + 完整英文切换），现代白瓷感艺术图标，三套原生主题（现代白瓷光 / 深空黑曜石 / 深邃午夜蓝）并实时同步 DWM 标题栏与滚动条。
   - 自由拖拽窗口位置、八向边缘调宽调高、拖拽预览分割线，配置自动保存与热重载。
5. **面向未来的三层扩展基座 + uTools 平滑迁移**：
   - **Tier 1**：原生 Rust 内置引擎（应用含 UWP/MSIX 与拼音首字母、Everything、文本+截图 SQLite 剪贴板、Super Match 粘贴胶囊、计算与意图识别、进程管理、网页搜索、窗口切换、GitHub 自动检查更新）。
   - **Tier 2A (`stdio_jsonrpc`)**：支持 **Rust (`tomoyo-sdk`)**、**Python**、**Go**、**Node/TS**、**C#** 等任意语言 50 行代码编写插件。
   - **Tier 2B (`wasm`)**：`tomoyo-wasm-sdk` 燃料计量能力沙箱插件，零默认系统权限。
   - **Tier 3 (`webview`)**：按需短生命周期 `tomoyo-webview-host.exe`，内置 `window.tomoyo` 与 **`window.utools` 兼容桥**（含 `setSubInput`、`db`/`dbStorage`、`hideMainWindowPasteText`、`Ctrl+Shift+T` 窗口置顶）及 `tomoyo-cli import-utools` 一键导入工具。

---

## 实测性能与反泄露压测 (`v0.4.0` Release)

| 指标 | 实测结果 |
|---|---|
| `tomoyo.exe` 单文件主程序 | **4.66 MB**（内置 SQLite + `wasmi` WASM 引擎 + 拼音 + Everything IPC + PNG 编解码 + WinHTTP 自动更新） |
| `tomoyo-webview-host.exe`（设置中心 & 插件市场 & WebView 插件） | **1.20 MB**（**0 MB 后台常驻内存**，关闭即进程退出） |
| `tomoyo-cli.exe`（脚手架 + uTools 导入 + SHA-256 市场） | **2.14 MB** |
| 示例 WASM 插件 (`plugins/example-wasm/plugin.wasm`) | **166.1 KB** |
| 休眠后后台 Working Set 内存占用 | **0.13 MB**（0% CPU，0 MB GPU VRAM） |
| **100 轮完整查询 + 绘图 + 预览 + 休眠压测 Delta** | **GDI `+0` · USER `+0` · Handles `+0` · Private `-0.33 MB`** |

---

## 快速开始 (Quick Start)

```powershell
# 克隆仓库
git clone https://github.com/q871958549/Tomoyo.git
cd Tomoyo

# 编译全部 Release 可执行文件
cargo build --release --workspace

# 启动 Tomoyo（默认 Alt+Space 呼出，右键图标或 Ctrl+, 打开设置中心）
.\target\release\tomoyo.exe

# 运行内置 100 轮反泄露与全链路自检
.\target\release\tomoyo.exe --self-test
.\target\release\tomoyo-webview-host.exe --self-test

# 开放插件生态 CLI
.\target\release\tomoyo-cli.exe registry list
.\target\release\tomoyo-cli.exe verify plugins\example-wasm
.\target\release\tomoyo-cli.exe new my-tool --lang wasm
.\target\release\tomoyo-cli.exe import-utools C:\path\to\utools-plugin
```

---

## 内置指令与快捷键一览

| 前缀 / 快捷键 | 功能说明 |
|---|---|
| *(直接输入)* | 混合极速检索：本机应用 + UWP/MSIX（支持中文拼音/首字母 `wx`/`jsq`/`vsc`）、计算器、意图识别（`#hex` 颜色、时间戳、JSON、URL、路径）、窗口、Everything 置顶命中 |
| `Ctrl+V` *(粘贴截图/文件)* | **Super Match 粘贴胶囊**：自动识别图片（看图/转 Base64/定位）与文件（复制路径/SHA-256 校验/在目录开终端），按 `Backspace` 清除 |
| `f <词>` / `ev <词>` | Everything 毫秒级全盘文件搜索 + 右侧有界预览/PNG 缩略图 + **拖拽行直出文件** |
| `cb <词>` | SQLite 持久化剪贴板历史（**文本 + 截图图片**，支持置顶收藏与回车自动贴回原焦点窗口） |
| `kill <词>` / `ps <词>` | 进程内存巡检与一键结束失控进程（按内存占用降序排列） |
| `g` / `gh` / `crates` / `docs` / `bd` / `zh` / `mdn` | 可自定义快捷网页搜索（在设置中心即时增删保存） |
| `w <词>` | 实时打开窗口快速切换 |
| `:update` | 联网检查 GitHub Releases 最新版本、预览更新日志并直达下载（支持启动静默自动检查） |
| `:settings` / `Ctrl+,` | 打开图形化设置中心（自动保存、快捷键直按录入、主题实时同步、内置 SHA-256 插件市场、更新检查） |
| `:stats` / `:plugins` / `:gc` | 实时内存/句柄/插件监控与手动休眠整理 |
| `:theme dark\|light\|midnight` / `:lang zh\|en` | 即时切换三套主题或中英文界面 |
| `hi` / `py` / `wasm` / `jwt` / `unit` / `json` | 四大内置示例插件（Rust STDIO / Python STDIO / WASM 能力沙箱 / WebView uTools 兼容桥） |
| `Ctrl+P` / `Tab` / `右键` / `Alt+1..8` | 预览面板开关 / 动作菜单 / 原生右键菜单 / 快速直达 |

---

## 仓库架构 (Workspace Layout)

- [`crates/tomoyo-protocol`](crates/tomoyo-protocol) — 零 OS 依赖的插件协议、Manifest、注册表 Schema 与 SDUI 原语
- [`crates/tomoyo-everything`](crates/tomoyo-everything) — 纯 Rust Everything 1.4 / 1.5a 原生 `WM_COPYDATA` IPC 客户端
- [`crates/tomoyo-core`](crates/tomoyo-core) — `config.toml`、QueryRouter、WinHTTP 客户端、GitHub Releases 自动更新器、有界文件/PNG 缩略图预览、拼音/模糊匹配、Frecency、多语言 i18n、内存与句柄遥测
- [`crates/tomoyo-providers`](crates/tomoyo-providers) — Built-in Providers：Apps (含 UWP)、Everything、Calc、Text+Image SQLite Clipboard、Process Killer、Web Search、Window Switcher、System/Super Match/`:update`/`:stats`
- [`crates/tomoyo-plugin-host`](crates/tomoyo-plugin-host) — 统一 STDIO (Job Object) + WASM (`wasmi` 燃料/权限沙箱) + WebView 宿主管理器
- [`crates/tomoyo-sdk`](crates/tomoyo-sdk) & [`crates/tomoyo-wasm-sdk`](crates/tomoyo-wasm-sdk) — Rust 原生与 WebAssembly 插件开发 SDK
- [`crates/tomoyo-webview-host`](crates/tomoyo-webview-host) — 按需短生命周期 WebView2 宿主：承载 `window.utools` 插件、图形化设置中心、SHA-256 插件市场与更新检查
- [`crates/tomoyo-cli`](crates/tomoyo-cli) — 插件脚手架、验证器、SHA-256 哈希工具、uTools 导入器与本地/远程注册表安装器
- [`crates/tomoyo-app`](crates/tomoyo-app) — 原生 Win32 桌面启动器主程序（SDF 渲染器、阿里巴巴普惠体、现代白品牌图标、OLE 拖出、休眠引擎）
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) & [`docs/PLUGIN_PROTOCOL.md`](docs/PLUGIN_PROTOCOL.md) — 完整架构蓝图与插件通信协议规范
- [`plugins/`](plugins) & [`registry/index.json`](registry/index.json) — 4 个多运行时参考插件与开源去中心化插件索引
- [`.github/workflows/`](.github/workflows) — Windows CI 自动化测试与 Tag 触发便携包 + SHA256SUMS 自动发布流水线

---

## 贡献与开源协议

欢迎所有开发者提交 Issue、PR 或向 `registry/index.json` 贡献开源插件！  
本项目采用 **MIT** 或 **Apache-2.0** 双许可协议（详见 [LICENSE-MIT](LICENSE-MIT) 与 [LICENSE-APACHE](LICENSE-APACHE)）。
