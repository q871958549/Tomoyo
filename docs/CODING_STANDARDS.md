# Tomoyo 工程编码规范与资源约束 (Coding Standards & Resource Constraints)

> **基本原则**：严格控制常驻资源占用，避免单文件职责过重，确保模块边界清晰可维护。  
> 提交合入 `main` 分支的代码需通过自动化规范检查与资源预算门禁。

---

## 一、模块划分与单文件规模约束

### 1.1 规模限制（由 CI 自动检查）
| 维度 | 推荐目标 | CI 检查上限 | 处理方式 |
|---|---|---|---|
| 单源码文件行数（`.rs` / `.js` / `.css` / `.html` / `.py`） | ≤ 300 行 | **≤ 450 行** | 超限按单一职责拆分为子模块目录（`mod.rs` + 专责子文件） |
| 单个函数 / 方法体行数 | ≤ 45 行 | **≤ 80 行** | 提取命名清晰的辅助函数，避免过长分支逻辑 |
| 单个 `struct` 字段数 | ≤ 12 个 | **≤ 20 个** | 将内聚状态聚合为子结构体（如 `WindowGeometry`、`InteractionState`） |
| 单次 `unsafe` 块范围 | 最小化 | 仅包裹单组关联 FFI 调用 | Win32 句柄统一封装 RAII `Drop`，避免大范围裸 `unsafe` 块 |

### 1.2 目录与单一职责划分约定
每个文件保持单一明确职责：
- **协议层 `tomoyo-protocol`**：只定义纯数据结构、Manifest、筛选类型、JSON-RPC 与 UI 原语；禁止 OS API、文件 I/O 或网络依赖。
- **核心层 `tomoyo-core`**：路由引擎、配置、匹配器、Frecency、轻量预览、WinHTTP、更新器、字体与遥测；不依赖 UI 窗口或具体 Provider 实现。
- **内置引擎 `tomoyo-providers`**：每个 Provider 使用独立文件或子目录（超过 400 行拆分为 `mod.rs` + 专责子文件）；避免在 `query()` 热路径执行大文件全量读盘或同步网络请求。
- **插件宿主 `tomoyo-plugin-host`**：发现 (`discovery.rs`)、进程隔离 (`job.rs`/`process.rs`)、WASM 沙箱 (`wasm.rs`)、生命周期调度 (`host.rs`) 分离维护。
- **桌面外壳 `tomoyo-app`**：
  - `window/`：消息路由 (`wndproc.rs`)、初始化 (`bootstrap.rs`)、布局几何 (`layout.rs`)、输入导航 (`input.rs`)、菜单 (`menus.rs`)、命令执行 (`commands.rs`)、主调度 (`mod.rs`)；
  - `renderer/`：主题色板 (`theme.rs`)、SDF 光栅 (`canvas.rs`)、筛选栏 (`filter_bar.rs`)、GDI/DIB RAII (`gdi.rs`)、SDF/GDI 绘制通道 (`sdf_pass.rs` / `gdi_pass.rs`)、主绘制调度 (`mod.rs`)；
  - `actions/`：动作分发 (`mod.rs`)、剪贴板 Win32 读写 (`clipboard_win.rs`)、Shell 与进程辅助 (`shell_win.rs`)。
- **按需 WebView 宿主 `tomoyo-webview-host`**：原生宿主 (`win_host.rs`)、单实例守护 (`single_instance.rs`)、原生目录浏览 (`folder_picker.rs`)、桥接分发 (`bridge.rs`)、注入脚本 (`shim_js.rs`)、市场数据 (`store.rs`)、本地 DB (`db.rs`)，以及 `ui/` 下独立的 `settings.html` / `settings.css` / `i18n.js` / `store_updater.js` / `settings.js`。

> 表单组件选型、双层配置校验与 ClearType 字体/主题对比度规范详见 [UI/UX 与组件设计规范](UI_DESIGN_STANDARDS.md)。

---

## 二、内存与句柄约束 (Resource Budget Contract)

### 2.1 主进程禁用依赖列表（CI 自动校验 `Cargo.toml`）
常驻主进程 `tomoyo.exe` 禁止引入以下依赖或渲染路径：
- 禁止 `tokio` / `async-std` 多线程异步运行时（主进程基于 Win32 消息循环与轻量工作线程处理查询）。
- 禁止 `reqwest` / `hyper` / `surf` / `rustls`（统一使用系统内置 `WinHTTP`）。
- 禁止将 `electron` / `tauri` / `wry` / `cef` 嵌入常驻主进程（WebView 仅在独立短生命周期进程 `tomoyo-webview-host.exe` 中按需启动）。
- 禁止 `ID2D1DCRenderTarget` / `d3d10warp.dll`（历史上曾带来约 +51.4 MB Private Commit 与 +219 句柄开销）。
- 禁止高频轮询定时器读取剪贴板或轮询窗口，改用 `AddClipboardFormatListener`、`RegisterHotKey` 与 `GetMessageW` 事件驱动。

### 2.2 有界缓存与资源回收原则
- **集合有界**：内存集合显式声明上限常量（如 `MAX_CACHED_ICONS`、`MAX_PREVIEW_READ_BYTES` 等）。
- **查询热路径约束 (`Provider::query`)**：
  - 不在查询期发起同步网络请求；
  - 不在查询期全量读取大文件或预先做 Base64 编码；
  - 不在 RAM 常驻大图位图（缩略图按需在后台线程解码并在休眠时释放）。
- **句柄 RAII 管理**：`HICON`、`HFONT`、`HBRUSH`、`HBITMAP`、`HDC`、`HANDLE`、`HKEY`、`HINTERNET` 均由 `Drop` 确定性释放。
- **休眠清理**：隐藏超时或执行 `:gc` 时释放离屏 DIB、字体、图标缓存、预览缓冲，收缩 SQLite 页缓存（`PRAGMA shrink_memory`），并在主线程及异步工作线程调用 `mi_collect(true)` 归还空闲内存页。

### 2.3 自动化预算门禁（`--self-test --strict-budget`）
每次 CI 构建均需通过以下实测预算校验：
| 指标 | 允许上限 |
|---|---|
| 休眠态 Working Set | **≤ 2.0 MB** |
| 休眠态 Private Commit（含应用索引、SQLite 与私有字体） | **≤ 28.0 MB** |
| 100 轮查询/绘制/休眠压测 GDI 句柄漂移 | **`+0`** |
| 100 轮压测 USER 句柄漂移 | **`+0`** |
| 100 轮压测内核句柄漂移 | **`≤ +2`** |
| 100 轮压测 Private Commit 漂移 | **`≤ +0.50 MB`** |

---

## 三、提交前验证清单

提交 PR 或打 Tag 发布前依次执行以下检查：
1. `pwsh -File scripts/check_hygiene.ps1` — 源码行数上限、禁用依赖与渲染路径校验；
2. `RUSTFLAGS="-D warnings" cargo test --workspace` — 全量测试零警告通过；
3. `cargo build --release --workspace` — Release 构建成功；
4. `.\target\release\tomoyo.exe --self-test --strict-budget` — 100 轮内存与句柄预算门禁通过；
5. `.\target\release\tomoyo-webview-host.exe --self-test` — WebView 宿主与设置界面自检通过。
