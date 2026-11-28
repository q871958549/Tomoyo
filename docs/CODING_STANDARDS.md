# Tomoyo 工程编码规范与防膨胀约束 (Coding Standards & Anti-Bloat Contract)

> **基本原则**：严格守住微内核常驻资源红线，杜绝单文件上帝模块与隐性依赖膨胀，确保前后端契约与模块边界长期清晰可维护。  
> 所有合入 `main` 分支的代码均须通过 7 道自动化卫生门禁、`cargo clippy -D warnings` 与严格资源预算压测。

---

## 一、模块划分与单文件双水位约束

### 1.1 规模双水位（由 `scripts/check_hygiene.mjs` 与 `scripts/check_hygiene.ps1` 自动校验）
| 维度 | 软提醒水位 (Advisory) | CI 硬红线 (Hard Fail) | 处理方式 |
|---|---|---|---|
| 单源码文件行数（`.rs` / `.c` / `.h` / `.m` / `.svelte` / `.ts` / `.js` / `.mjs` / `.css` / `.py`） | `> 380 行` 输出警告（棘轮总篇数 `≤ 91`） | **`≤ 450 行`** | 超限按单一职责拆分为专责子模块；新增文件优先保持 `≤ 380 行` 防止软水位蔓延 |
| 单个函数 / 方法体行数 | `≤ 45 行` | **`≤ 80 行`**（由 [`clippy.toml`](../clippy.toml) 守门） | 提取命名清晰的辅助函数，避免深层分支嵌套 |
| 单个 `struct` 字段数 | `≤ 12 个` | **`≤ 20 个`** | 将内聚状态聚合为子结构体（如 `WindowGeometry`、`InteractionState`） |
| 单次 `unsafe` 块范围 | 最小化 | 仅包裹单组关联 FFI 调用 | Linux C shim 与 Win32 句柄统一封装 RAII `Drop`，C 层开启 `-Wall -Wextra -Werror` |

### 1.2 Rust 后端与原生 C Shim 模块职责约定
- **协议层 `tomoyo-protocol`**：纯数据结构、Manifest、筛选类型、JSON-RPC v2 与 UI 原语；禁止任何 OS API、文件 I/O 或网络依赖。
- **核心层 `tomoyo-core`**：路由引擎、配置 (`config*`)、工作流定义与 BYOM/BYOK LLM 管道 (`flows.rs` / `flows_llm.rs`)、热键诊断 (`hotkey_health.rs`)、安全与秘密存储（Linux `libsecret-1` + `linux_sys.c` / Windows DPAPI）、HTTP（Linux `libcurl` / Windows WinHTTP）、字体（`fontconfig` / Win32）与遥测（`/proc` / Win32）；不依赖 UI 窗口。
- **内置引擎 `tomoyo-providers`**：每个 Provider 独立文件或子目录（超过 400 行拆分为 `mod.rs` + 专责子文件）；支持 Linux XDG `.desktop`/`inotify`/`fd`+`plocate`/KWin 与 Windows Everything/UWP；`Provider::query()` 热路径严禁大文件全量读盘或同步网络请求。
- **插件宿主 `tomoyo-plugin-host`**：发现 (`discovery.rs`)、进程隔离 (`job.rs` Linux 进程组 + `PR_SET_PDEATHSIG` / Windows Job Object / `process.rs`)、WASM 沙箱 (`wasm.rs`)、生命周期调度 (`host.rs`) 分离维护。
- **桌面外壳 `tomoyo-app`**：
  - Linux (`linux/`)：模块化外壳状态 (`shell_state.rs`)、异步查询与托盘 (`shell_query.rs`)、事件 (`shell_events.rs`)、命令执行 (`shell_commands.rs`)、Pango/Cairo + SDF 渲染 (`render_pango.rs`)、KGlobalAccel/X11 热键 (`hotkeys.rs`)、单实例 Unix Socket IPC (`ipc.rs`) 与 `-Werror` C shim (`shell.c` / `pango_draw.c` / `kga_x11.c`)；
  - Windows (`window/` + `renderer/` + `actions/`)：Win32 消息循环、SDF+GDI 渲染通道、Win32/Linux 剪贴板与文件操作按平台模块分离。
- **独立截图工具 `tomoyo-capture`**：截图采集 (`screen.rs`，Linux Spectacle/`grim`/X11 与 Windows GDI)、GTK3/Cairo 选区与贴图 (`linux_overlay.rs` / `linux_pin.rs` / `linux_cap.c`)、标注模型与渲染 (`annotate*.rs`)、长截图拼接 (`stitch.rs`)。
- **按需 WebView 宿主 `tomoyo-webview-host`**：Linux (`linux_host.rs` / `linux_workers.rs` / `linux_hotkeys.rs` / `linux_wv.c` / `linux_kga.c` 基于 WebKitGTK 4.1) 与 Windows (`win_host.rs` / `win_workers.rs` 基于 WebView2)、单实例守护、桥接分发 (`bridge*.rs`)、设置/翻译/剪贴板/OCR 视图与构建期 Gzip 压缩嵌入。

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
- 禁止 `tokio` / `async-std` 多线程异步运行时（Linux 基于 GLib/GTK 主事件循环，Windows 基于 Win32 消息循环）。
- 禁止 `reqwest` / `hyper` / `surf` / `rustls`（Linux 统一使用系统 `libcurl`，Windows 使用系统 `WinHTTP`）。
- 禁止将 `electron` / `tauri` / `wry` / `cef` 嵌入常驻主进程（WebView 严格隔离在短生命周期 `tomoyo-webview-host` 中，Linux 基于 WebKitGTK 4.1，Windows 基于 WebView2）。
- 禁止 `ID2D1DCRenderTarget` / `D2D1CreateFactory` / `d3d10warp.dll`（纯 Rust SDF + Linux Cairo/Pango / Windows ClearType GDI）。
- 禁止高频轮询定时器读取剪贴板或应用目录，严格事件驱动（Linux GTK `owner-change` / `inotify` / `org.kde.kglobalaccel` D-Bus；Windows `AddClipboardFormatListener` / `RegisterHotKey` / `GetMessageW`）。

### 2.2 前端产物与 Release 二进制体积天花板
由 `node scripts/check_hygiene.mjs --check-binaries`（Linux/macOS/Windows 通用）与 `scripts/check_hygiene.ps1 -CheckBinaries`（Windows）强制把关：
| 产物 | 硬上限 | 说明 |
|---|---|---|
| `frontend/dist/settings.html` | **≤ 300 KB** | 12 标签设置中心单文件产物（构建期再 Gzip 压缩嵌入宿主） |
| `plugins/{official-translate,official-clipboard,official-notes,builtin-ocr}/ui/index.html` | **≤ 150 KB** | 按需 WebView 工作台单文件产物 |
| `tomoyo` / `tomoyo.exe` | **≤ 6.2 MB (Linux) / ≤ 5.8 MB (Win)** | 常驻主内核（`lto = "fat"` 紧致编译，含 SDF 字体光栅、SQLite、WASM 宿主） |
| `tomoyo-webview-host` / `.exe` | **≤ 4.2 MB** | 按需 WebView 宿主（WebKitGTK 4.1 / WebView2 + 前端单文件包） |
| `tomoyo-capture` / `.exe` | **≤ 1.6 MB** | 原生多屏截图、标注、长截图与桌面贴图工具 |
| `tomoyo-plugin-clipboard` / `.exe` | **≤ 2.5 MB** | 独立剪贴板历史守护插件（含内嵌 SQLite；图片目录总上限 `48 MB` + 孤儿文件自动回收） |
| `tomoyo-plugin-{ocr,notes,md-editor,translate,capture,winlayout,ai}` / `.exe` | **≤ 0.8 MB** | 其余官方独立 STDIO 插件 |

### 2.3 运行时预算门禁（`--self-test --strict-budget`）
| 指标 | Linux 上限 | Windows 上限 |
|---|---|---|
| 休眠态 Working Set | 监控无限制 | **≤ 2.0 MB** |
| 休眠态 Private Commit（含应用索引、SQLite 与字体缓存） | **≤ 45.0 MB** | **≤ 28.0 MB** |
| 100 轮查询/休眠压测 Handles 漂移 | **`≤ +2`** | **`+0` GDI/USER** |
| 100 轮压测 Private Commit 漂移 | **`≤ +0.50 MB`** | **`≤ +0.50 MB`** |

---

## 三、四层全自动防膨胀治理管线与提交前清单

仓库通过 **Git Hooks（`.githooks/`）+ 构建期卫生门禁 + 磁盘定时回收 + 运行时数据预算** 形成本地与 CI 同构的自动化闭环（一键激活：`node scripts/setup-governance.mjs --install-timer`）：
1. **本地提交强拦截（[`.githooks/pre-commit`](../.githooks/pre-commit) / [`pre-push`](../.githooks/pre-push)）**：
   - `pre-commit`：拦截 `> 2 MB` 的非白名单误提交大文件，并自动运行 `node scripts/check_hygiene.mjs`（含 `.rs/.c/.h/.m` 行数棘轮 `≤ 91` 软上限、零禁引入 crate/npm、前端产物限额及 WebView IPC 契约）；
   - `pre-push`：自动运行 `node scripts/check_hygiene.mjs`、`node scripts/verify-release-readiness.mjs` 与 `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets -- -D warnings`。
2. **本地磁盘增量自洁（[`scripts/workspace-gc.mjs`](../scripts/workspace-gc.mjs)）**：支持 `--dry-run` / `--apply --max-age-days=7` / `--prune-debug-all` 安全清理陈旧 `target/debug` 指纹与增量缓存，配合 Linux `systemd --user` `tomoyo-workspace-gc.timer` 每周低优先级自动清扫。
3. **运行时存储自限与健康检查**：`official-clipboard` 在启动与写入剪枝时强制执行 `clipboard-images` `48 MB` LRU 字节预算与孤儿 PNG 清扫；`tomoyo-cli doctor` 实时诊断本地存储占用与 Git Hooks 激活状态。

手动完整验证（含 Release 与运行时自检）可依次执行：
1. `cd frontend && npm run check && npm test && npm run build && cd ..` — Svelte 5 零错误/零警告类型检查、全量 Vitest 单测（含 IPC 契约测试）与 4 套单文件产物构建；
2. `node scripts/check_hygiene.mjs`（Windows 可选 `pwsh -File scripts/check_hygiene.ps1`）— 行数双水位、禁用 Rust/npm 依赖、防 WARP、产物预算、Rust-TS IPC 契约与 `happy-dom` 运行时冒烟；
3. `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets -- -D warnings` — 全工作区 Clippy 零警告静态检查（Linux C shim 同受 `-Wall -Wextra -Werror` 约束）；
4. `RUSTFLAGS="-D warnings" cargo test --workspace` — 全量 Rust 单测与集成测试零警告通过；
5. `cargo build --release --workspace` & `node scripts/check_hygiene.mjs --check-binaries` — Release 构建并校验 5 个核心二进制体积预算；
6. `./target/release/tomoyo --self-test --strict-budget` — 100 轮内存与句柄零泄漏预算门禁；
7. `tomoyo-webview-host --self-test`、`tomoyo-capture --self-test`、`tomoyo-plugin-ocr --self-test`、`tomoyo-cli registry check --manifest-only` — 子进程与插件全链路自检。
