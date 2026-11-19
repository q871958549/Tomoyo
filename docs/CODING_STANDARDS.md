# Tomoyo 工程编码规范与防膨胀架构宪法 (Coding Standards & Anti-Bloat Constitution)

> **核心信条**：**异常资源占用是可耻的行为**；**上帝文件（God File）是长期演进的毒药**。  
> 任何破坏低内存、零泄露或模块化边界的提交，均不得合入 `main` 分支。

---

## 一、模块划分与防膨胀红线（告别上帝类 / 上帝文件）

### 1.1 硬性规模上限（由 CI 自动检查阻断）
| 维度 | 推荐目标 | 硬性红线（CI 阻断） | 处理方式 |
|---|---|---|---|
| 单源码文件行数（`.rs` / `.js` / `.css` / `.html` / `.py`） | ≤ 300 行 | **≤ 450 行** | 超过必须按单一职责拆分为子模块目录（`mod.rs` + 专责子文件） |
| 单个函数 / 方法体行数 | ≤ 45 行 | **≤ 80 行** | 提取清晰命名的纯辅助函数，禁止千行 `match` 大杂烩 |
| 单个 `struct` 字段数 | ≤ 12 个 | **≤ 20 个** | 将内聚状态聚合为子结构体（如 `WindowGeometry`、`PreviewCache`、`DragState`） |
| 单次 `unsafe` 块范围 | 最小化 | 仅包裹单组关联 FFI 调用 | 严禁把整个函数包在巨型 `unsafe {}` 块中；Win32 句柄必须封装 RAII `Drop` |

### 1.2 单一职责（SRP）与目录分工约定
每个文件只做一件事，名字即职责：
- **协议层 `tomoyo-protocol`**：只定义纯数据结构、Manifest、JSON-RPC 与 SDUI 原语；**绝对禁止**任何 OS API、文件 I/O、网络或重型依赖。
- **核心层 `tomoyo-core`**：路由引擎、配置、匹配器、Frecency、轻量预览、原生 WinHTTP、更新器、字体与遥测；**禁止**依赖任何 UI 窗口或具体 Provider。
- **内置引擎 `tomoyo-providers`**：每个 Provider 独立文件或子目录（超过 400 行必须拆分为 `mod.rs` + `store.rs`/`eval.rs`/`diagnostics.rs` 等）；严禁在 `query()` 热路径执行大文件全量读盘或同步网络请求。
- **插件宿主 `tomoyo-plugin-host`**：发现 (`discovery.rs`)、进程隔离 (`job.rs`/`process.rs`)、WASM 沙箱 (`wasm.rs`)、生命周期编排 (`host.rs`) 彻底分离。
- **桌面外壳 `tomoyo-app`**：
  - `window/`：消息路由 (`wndproc.rs`)、布局几何 (`layout.rs`)、输入导航 (`input.rs`)、菜单 (`menus.rs`)、动作命令 (`commands.rs`)、主生命周期 (`mod.rs`) 严格解耦；
  - `renderer/`：主题色板 (`theme.rs`)、纯 Rust SDF 软光栅 (`canvas.rs`)、GDI/DIB RAII (`gdi.rs`)、主绘调度 (`mod.rs`) 严格解耦；
  - `actions/`：动作路由 (`mod.rs`)、剪贴板 Win32 编解码 (`clipboard_win.rs`)、Shell/进程/自动粘贴 (`shell_win.rs`) 严格解耦。
- **按需 WebView 宿主 `tomoyo-webview-host`**：原生窗口宿主 (`win_host.rs`)、桥接分发 (`bridge.rs`)、注入脚本 (`shim_js.rs`)、插件市场 (`store.rs`)、本地 DB (`db.rs`)，以及 `ui/` 下独立的 `settings.html` / `settings.css` / `i18n.js` / `settings.js`（编译期拼接内嵌，源码零耦合）。

---

## 二、反泄露与超低资源占用铁律（Resource Budget Contract）

### 2.1 禁用重型依赖黑名单（CI 自动校验 `Cargo.toml`）
常驻主进程 `tomoyo.exe` **绝对禁止**引入以下依赖或机制：
- 禁止 `tokio` / `async-std` 多线程重型异步运行时（主进程使用 Win32 消息循环 + 单查询工作线程即可实现 0% 空闲 CPU）。
- 禁止 `reqwest` / `hyper` / `surf` / `rustls`（使用系统自带零体积 `WinHTTP`）。
- 禁止 `electron` / `tauri` / `wry` / `cef` 常驻主进程（WebView 只允许存在于独立短生命周期进程 `tomoyo-webview-host.exe`）。
- 禁止 `ID2D1DCRenderTarget` / `d3d10warp.dll`（历史实测永久膨胀 +51.4 MB Private Commit 与 +219 句柄）。
- 禁止无休止后台轮询定时器（例如每 100ms 读剪贴板/扫窗口）；一切监控必须事件驱动（`AddClipboardFormatListener`、`RegisterHotKey`、`GetMessageW`）。

### 2.2 强制有界原则（Bounded Everything）
- **所有内存集合必须显式声明上限常量**（如 `MAX_RAM_ITEMS`、`MAX_CACHE_ENTRIES`、`MAX_PREVIEW_READ_BYTES`、`MAX_DISK_IMAGES`）。
- **查询热路径 (`Provider::query`) 禁止**：
  - 禁止同步网络请求；
  - 禁止全量读取大于 64 KB 的磁盘文件或预先 Base64 编码二进制文件（重型转换必须延迟到用户按 `Enter` 执行动作时按需触发）；
  - 禁止在 RAM 中常驻解压后的位图/截图（图片一律内容寻址 PNG 落盘 + LRU 淘汰，仅在预览选中项时解码有界缩略图，休眠时立即销毁）。
- **所有 Win32 资源必须 RAII**：`HICON`、`HFONT`、`HBRUSH`、`HBITMAP`、`HDC`、`HANDLE`、`HKEY`、`HINTERNET` 一律由 `Drop` 确定性释放，严禁裸句柄散落。
- **休眠彻底归零**：隐藏超时或 `:gc` 必须释放全部离屏 DIB、字体、图标缓存、预览缩略图、收缩 SQLite 页缓存（`PRAGMA shrink_memory`）、触发 `mimalloc` 页归还（`mi_collect(true)`）并执行 `K32EmptyWorkingSet`。

### 2.3 自动化资源预算硬门禁（`--self-test --strict-budget`）
每次 CI 构建必须通过以下硬性实测指标，超标即直接失败：
| 指标 | 硬红线 |
|---|---|
| 休眠态 Working Set | **≤ 2.0 MB**（当前目标 < 0.5 MB） |
| 休眠态 Private Commit（含 279 应用索引 + SQLite + 阿里巴巴字体） | **≤ 28.0 MB** |
| 100 轮完整查询+渲染+预览+休眠压测 GDI 句柄漂移 | **`+0`** |
| 100 轮完整压测 USER 句柄漂移 | **`+0`** |
| 100 轮完整压测内核句柄漂移 | **`≤ +2`**（目标 `+0`） |
| 100 轮完整压测 Private Commit 漂移 | **`≤ +0.50 MB`**（目标 `≤ 0 MB`） |

---

## 三、代码质量与提交前自检清单

提交 PR / 推送前必须依次执行并全部通过：
1. `pwsh -File scripts/check_hygiene.ps1` — 源码行数红线、禁用依赖黑名单与禁用渲染路径检查；
2. `cargo test --workspace` — 全量单元与集成测试 100% 通过且 **0 编译警告**；
3. `cargo build --release --workspace` — Release 零警告构建；
4. `.\target\release\tomoyo.exe --self-test --strict-budget` — 100 轮反泄露与严格资源预算硬门禁通过；
5. `.\target\release\tomoyo-webview-host.exe --self-test` — WebView 宿主与设置中心自检通过。
