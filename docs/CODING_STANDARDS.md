# Tomoyo 工程编码规范与防膨胀约束 (Coding Standards & Anti-Bloat Contract)

> **基本原则**：严格守住微内核常驻资源红线，杜绝单文件上帝模块与隐性依赖膨胀，确保前后端契约与模块边界长期清晰可维护。  
> 所有合入 `main` 分支的代码均须通过 7 道自动化卫生门禁、`cargo clippy -D warnings` 与严格资源预算压测。

---

## 一、模块划分与单文件双水位约束

### 1.1 规模双水位（由 `scripts/check_hygiene.ps1` 自动校验）
| 维度 | 软提醒水位 (Advisory) | CI 硬红线 (Hard Fail) | 处理方式 |
|---|---|---|---|
| 单源码文件行数（`.rs` / `.svelte` / `.ts` / `.js` / `.mjs` / `.css` / `.py`） | `> 380 行` 输出黄字重构提示 | **`≤ 450 行`** | 超限按单一职责拆分为专责子模块（如 `mod.rs` + 专责子文件或子组件） |
| 单个函数 / 方法体行数 | `≤ 45 行` | **`≤ 80 行`** | 提取命名清晰的辅助函数，避免深层分支嵌套 |
| 单个 `struct` 字段数 | `≤ 12 个` | **`≤ 20 个`** | 将内聚状态聚合为子结构体（如 `WindowGeometry`、`InteractionState`） |
| 单次 `unsafe` 块范围 | 最小化 | 仅包裹单组关联 FFI 调用 | Win32 句柄统一封装 RAII `Drop`，禁止大范围裸 `unsafe` 块 |

### 1.2 Rust 后端目录与单一职责约定
- **协议层 `tomoyo-protocol`**：纯数据结构、Manifest、筛选类型、JSON-RPC v2 与 UI 原语；禁止任何 OS API、文件 I/O 或网络依赖。
- **核心层 `tomoyo-core`**：路由引擎、配置 (`config*`)、工作流定义与 BYOM/BYOK LLM 管道 (`flows.rs` / `flows_llm.rs`)、热键诊断 (`hotkey_health.rs`)、安全与秘密存储 (`secrets.rs` / `permissions.rs`)、WinHTTP、字体与遥测；不依赖 UI 窗口或具体 Provider。
- **内置引擎 `tomoyo-providers`**：每个 Provider 独立文件或子目录（超过 400 行拆分为 `mod.rs` + 专责子文件）；`Provider::query()` 热路径严禁大文件全量读盘或同步网络请求。
- **插件宿主 `tomoyo-plugin-host`**：发现 (`discovery.rs`)、进程隔离 (`job.rs` / `process.rs`)、WASM 沙箱 (`wasm.rs`)、生命周期调度 (`host.rs`) 分离维护。
- **桌面外壳 `tomoyo-app`**：
  - `window/`：消息路由 (`wndproc.rs`)、初始化 (`bootstrap.rs`)、热键注册与钩子 (`hotkeys.rs` / `hotkey_hook.rs`)、布局几何 (`layout.rs`)、输入导航 (`input.rs`)、菜单 (`menus.rs`)、命令与工作流执行 (`commands.rs` / `flow_exec.rs`)、主调度 (`mod.rs`)；
  - `renderer/`：主题色板 (`theme.rs`)、SDF 光栅 (`canvas.rs`)、筛选栏 (`filter_bar.rs`)、GDI/DIB RAII (`gdi.rs`)、SDF/GDI 渲染通道 (`sdf_pass.rs` / `gdi_pass.rs`)；
  - `actions/`：动作分发 (`mod.rs`)、剪贴板 Win32 读写 (`clipboard_win.rs`)、Shell 与进程辅助 (`shell_win.rs`)。
- **独立截图工具 `tomoyo-capture`**：截图与多屏 DPI 选区 (`main.rs` / `overlay.rs`)、标注模型与渲染 (`annotate.rs` / `annotate_render.rs` / `annotate_arrow.rs` / `annotate_bar.rs`)、长截图拼接与控制条 (`scroll_capture.rs` / `scroll_hud.rs` / `stitch.rs`)、桌面贴图 (`pin.rs`)。
- **按需 WebView 宿主 `tomoyo-webview-host`**：原生宿主 (`win_host.rs`)、单实例守护 (`single_instance.rs`)、目录浏览 (`folder_picker.rs`)、桥接分发 (`bridge.rs` / `bridge_os.rs`)、设置/翻译/剪贴板/OCR 视图 (`settings_view.rs` / `tr_view.rs` / `clip_view.rs` / `ocr_view.rs`)、构建期 Gzip 压缩 (`build.rs` 将 4 个单文件 HTML 压为 `.html.gz` 嵌入，按需 `< 2 ms` 解压)。

### 1.3 WebView 前端（Svelte 5 + Tailwind v4 + TypeScript）工程规范
WebView 前端位于 `frontend/`，通过 Vite + `vite-plugin-singlefile` 编译为 4 个独立单文件 HTML（`settings.html` / `translator.html` / `clipboard.html` / `ocr.html`），按需拉起、关窗即释放内存：
- **Runes 状态边界规范（严禁 `ownership_invalid_mutation`）**：
  - 子组件若需修改父级 `$state` 对象字段，必须显式声明 `let { cfg = $bindable() } = $props()` 或回调 `onSave(...)`；禁止在未声明 `$bindable()` 的子组件中直接变更父级对象属性。
  - 从 Svelte 组件导出的 TypeScript 类型（如 `export interface WordItem`）必须放在 `<script module lang="ts">` 块中。
- **零重型前端依赖**：禁止引入 `react` / `vue` / `angular` / `antd` / `mui` / `element-plus` / `lodash` / `moment` / `axios` / `electron` 等重型框架或工具库；所有基础控件统一复用 `frontend/src/lib/ui/` 下的轻量原语（`Button` / `Input` / `Select` / `Switch` / `Slider` / `Segmented` / `Combobox` / `ContextMenu` / `HotkeyRecorder` / `PathPicker`）。
- **IPC 双向契约一致性**：所有向宿主发送的指令必须经由 `postHost({ cmd: '...' })` 且 `cmd` 为静态字面量，与 `crates/tomoyo-webview-host/src/bridge.rs` 中 `HostMessage` 枚举变体 100% 一一对应（由 `check_hygiene.ps1` 与 `frontend/tests/ipc_contract.test.ts` 双重守护）。
- **主题与设计令牌统一**：所有颜色、圆角、边框与阴影均消费 `frontend/src/styles/tokens.css` CSS 变量，支持 `light` / `dark` / `midnight` 三主题无缝同步。详见 [UI/UX 与组件设计规范](UI_DESIGN_STANDARDS.md)。

---

## 二、内存、体积与句柄预算契约 (Resource & Size Budgets)

### 2.1 主进程禁用依赖与渲染红线（CI 自动校验）
- 禁止 `tokio` / `async-std` 多线程异步运行时（主进程基于 Win32 消息循环与按需轻量工作线程）。
- 禁止 `reqwest` / `hyper` / `surf` / `rustls`（统一使用系统内置 `WinHTTP`）。
- 禁止将 `electron` / `tauri` / `wry` / `cef` 嵌入常驻主进程（WebView 严格隔离在短生命周期 `tomoyo-webview-host.exe` 中）。
- 禁止 `ID2D1DCRenderTarget` / `D2D1CreateFactory` / `d3d10warp.dll`（纯 Rust SDF + ClearType GDI 渲染通道）。
- 禁止高频轮询定时器读取剪贴板或前台窗口，严格事件驱动（`AddClipboardFormatListener` / `RegisterHotKey` / `GetMessageW`）。

### 2.2 前端产物与 Release 二进制体积天花板
由 `scripts/check_hygiene.ps1 -CheckBinaries` 在本地与 CI 强制把关：
| 产物 | 硬上限 | 说明 |
|---|---|---|
| `frontend/dist/settings.html` | **≤ 320 KB** | 12 标签设置中心单文件产物（构建期再 Gzip 压缩嵌入宿主） |
| `frontend/dist/{translator,clipboard,ocr}.html` | **≤ 150 KB** | 三个按需工作台单文件产物 |
| `tomoyo.exe` | **≤ 6.5 MB** | 常驻主内核（含 SDF 字体光栅、SQLite、WASM 宿主） |
| `tomoyo-webview-host.exe` | **≤ 5.5 MB** | 按需 WebView2 宿主（含 Gzip 压缩的 4 套前端单文件包） |
| `tomoyo-capture.exe` | **≤ 1.5 MB** | 原生多屏截图、标注、长截图与桌面贴图工具 |
| `tomoyo-plugin-ocr.exe` / `tomoyo-plugin-notes.exe` | **≤ 0.8 MB** | 官方独立 STDIO 插件 |

### 2.3 运行时预算门禁（`--self-test --strict-budget`）
| 指标 | 允许上限 |
|---|---|
| 休眠态 Working Set | **≤ 2.0 MB** |
| 休眠态 Private Commit（含应用索引、SQLite 与私有字体） | **≤ 28.0 MB** |
| 100 轮查询/绘制/休眠压测 GDI / USER 句柄漂移 | **`+0` / `+0`** |
| 100 轮压测内核句柄漂移 | **`≤ +2`** |
| 100 轮压测 Private Commit 漂移 | **`≤ +0.50 MB`** |

---

## 三、提交前 7 步验证清单

提交 PR 或打 Tag 发布前依次执行：
1. `cd frontend && npm run check && npm test && npm run build && cd ..` — Svelte 5 零错误/零警告类型检查、全量 Vitest 单测（含 IPC 契约测试）与 4 套单文件产物构建；
2. `pwsh -File scripts/check_hygiene.ps1` — 行数双水位、禁用 Rust/npm 依赖、防 WARP、产物预算、Rust-TS IPC 契约与 `happy-dom` 运行时冒烟；
3. `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets -- -D warnings` — 全工作区 Clippy 零警告静态检查；
4. `RUSTFLAGS="-D warnings" cargo test --workspace` — 全量 Rust 单测与集成测试零警告通过；
5. `cargo build --release --workspace` & `pwsh -File scripts/check_hygiene.ps1 -CheckBinaries` — Release 构建并校验 5 个核心二进制体积预算；
6. `.\target\release\tomoyo.exe --self-test --strict-budget` — 100 轮内存与句柄零泄漏预算门禁；
7. `tomoyo-webview-host --self-test`、`tomoyo-capture --self-test`、`tomoyo-plugin-ocr --self-test`、`tomoyo-cli registry check --manifest-only` — 子进程与插件全链路自检。
