# Tomoyo 微内核彻底解耦与开放插件平台蓝图 (`v2.7 → v3.0`)

> **核心原则**：**纯微内核基座 + 零特权官方插件生态**。翻译、截图/录屏、备忘录、Markdown 编辑器、离线 OCR 与剪贴板工作台均为**严格可选的独立插件**——官方插件与社区第三方插件共享完全相同的公开协议与分发渠道，不安装即零体积、零配置、零菜单干扰、零常驻内存。

---

## 1. 架构边界总览

### 1.1 永久留存的 Tomoyo Core（微内核基座）
仅保留「极速桌面入口 + 插件操作系统（Launcher OS）」本身：
- **检索与路由核心**：应用索引、本地文件检索（Everything / Linux 索引）、基础算式/颜色/时间戳即时计算、窗口切换/进程管理/系统指令、自定义网页快捷搜索、分类筛选栏与有界通用预览；
- **多运行时插件宿主**：STDIO JSON-RPC、WASM 沙箱（`wasmi`）、通用按需 WebView 宿主（零业务页面内嵌，关闭即销毁）；
- **统一基础服务 API（对所有插件平等开放）**：
  - 声明式设置表单与系统级凭据保险箱（Secret Storage：Win32 DPAPI / Linux Secret Service）；
  - 统一热键治理（含全屏/远控避让与 Hotkey Doctor）；
  - 通用事件总线（剪贴板文本/图片广播、热键事件、通用划词读取原语）；
  - 去中心化多平台商店引擎（分片索引 + `platforms` 多架构下载 + SHA-256 校验 + 一键升级/彻底卸载）。

### 1.2 全部外迁为独立可选插件（Zero-Privilege First-Party Plugins）
| 插件模块 | 当前位置 / 耦合点 | 目标形态 | 计划阶段 |
| :--- | :--- | :--- | :--- |
| **Quiet.md MD 编辑器** | `tomoyo-core/src/quiet_md.rs`、`InternalCommandKind::InstallQuietMd`、`file_actions.rs`、`PluginConfigDrawer.svelte` | 100% 自包含于 `plugins/official-md-editor`，核心零感知 | **Phase 1 (`v2.7`)** |
| **Quick Notes 备忘录** | `plugins/official-notes`、`OFFICIAL_PLUGINS` 占位展示 | 仅安装后出现在「已安装」，未安装仅在商店按需下载 | **Phase 1 (`v2.7`)** |
| **示例插件 (`example-*`)** | `install-local.{sh,ps1}` 强制复制至用户插件目录 | 移出默认终端用户安装，仅 `--with-examples` 或 `tomoyo-cli new` 生成 | **Phase 1 (`v2.7`)** |
| **离线 OCR** | `tomoyo-webview-host/src/ocr_view.rs` + `frontend/src/apps/ocr/` | Hybrid 插件（`tomoyo-plugin-ocr` + 自带前端页面），移出 `tomoyo-webview-host` | **Phase 3a (`v2.9`)** |
| **多源划词翻译** | `tomoyo-core/src/translate/`、`TranslateProvider`、`tr_view.rs`、`TranslateTab.svelte` | 独立 Hybrid 插件（走 Protocol v3 `secret` 设置 + `SelectionRead` + `callPlugin`） | **Phase 3b (`v2.9`)** |
| **原生截图与录屏** | `tomoyo-capture`、`CaptureConfig`、`CaptureProvider`、`CaptureTab.svelte` | 独立原生插件（走 Protocol v3 直接热键动作 + `hotkey`/`dir` 设置 + `[[tray_actions]]`） | **Phase 3c (`v3.0`)** |
| **剪贴板历史工作台** | `ClipboardProvider`、`clip_view.rs`、`ClipboardApp.svelte` | 核心仅留极简剪贴板事件广播器（无订阅零监听），历史库与工作台整体插件化 | **Phase 3d (`v3.0`)** |

---

## 2. Protocol v3 & Registry v2.1 通用协议基建（先造桥，再拆墙）

为了让重型官方工具能够零降级地迁出核心，同时赋予社区第三方开发者同等自由度，必须补齐以下 5 项通用协议原语：

1. **Registry Schema v2.1 多平台产物矩阵（Phase 1 落地）**
   - `RegistryPluginEntry` 新增 `platforms: BTreeMap<String, PlatformPackage>`（键为 `windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`、`macos-aarch64`、`macos-x86_64` 或跨平台 `any`）及 `min_tomoyo_version`；
   - 客户端与 `tomoyo-cli install` 自动按当前 OS/架构解析匹配包体并校验对应 SHA-256；不支持当前系统的插件在商店明确标注不可安装，向下 100% 兼容现有 `source` + `sha256` 清单。
2. **Hybrid 混合运行时：WebView ↔ 插件后端双向 RPC（Phase 2）**
   - 扩展 `tomoyo-plugin.toml` 支持同一插件同时声明 `runtime`（STDIO/WASM 后端）与 `ui.entry`（WebView 前端）；
   - 在 `shim_js.rs` / `docs/tomoyo.d.ts` 暴露 `window.tomoyo.callPlugin(method, params)`，通用转发至对应插件后端进程并由空闲回收机制统一管理——彻底消除 `tomoyo-webview-host` 内针对单一业务的 `HostMessage` 特化分支。
3. **声明式 `[[settings]]` Schema 升级与通用密钥库（Phase 2）**
   - 扩充 `SettingFieldKind`：`secret`（系统加密凭据存储，前端仅显示已配置状态）、`hotkey`（复用 `HotkeyRecorder` 与冲突检测）、`dir` / `file`（复用系统路径选择框），并支持 `section` 分组与 `visible_if` 条件显隐；
   - 让翻译、截图、剪贴板与所有社区插件均通过清单声明即可获得原生质感配置抽屉，彻底释放 `settings.html`（目标 `< 200 KB`）。
4. **公开宿主扩展钩子（消灭 `InternalCommandKind` 与硬编码托盘/文件动作，Phase 2）**
   - `[[tray_actions]]`：托盘菜单按已安装插件动态生成；
   - `triggers.file_extensions` / `file_actions`：插件可声明对 `.md`、`.png` 等文件扩展名的 `Tab` 二级动作，替代 `crates/tomoyo-providers/src/file_actions.rs` 中的硬编码分支；
   - `Permission::SelectionRead`：将无痕复制划词抽象为通用权限原语，供翻译、AI 润色、查词等所有插件公平调用；
   - 直接热键执行模式：插件 `[[hotkeys]]` 触发 `hotkey_triggered` 时可直接拉起原生窗/WebView，无需强迫经过搜索框。
5. **首次启动「轻装向导 / Starter Packs」（Phase 4）**
   - 主程序安装包保持 100% 纯净无捆绑；首次打开设置/引导页时提供按需勾选卡片（截图 / 翻译 / 剪贴板 / 备忘录 / OCR / Quiet.md），勾选即从商店验签安装，跳过即永久零冗余。

---

## 3. 分阶段推进计划与量化验收门禁

### Phase 1（`v2.7` · 已落地 ✅）：多平台商店基建 + Quiet.md 彻底零特权拆离 + 纯净安装模式
- [x] **文档蓝图定稿**：新增 `docs/DECOUPLING_BLUEPRINT.md` 并对齐 `docs/ROADMAP.md`；
- [x] **Registry Schema v2.1**：`tomoyo-protocol`、`tomoyo-webview-host/src/store_index.rs`、`tomoyo-cli`（`install` / `pack` / `registry check`）与 `StoreTab.svelte` 全面支持 `platforms` 多架构解析与不兼容提示；插件图标解析优先读取插件自带 `icon.png`，不再依赖核心硬编码白名单；
- [x] **Quiet.md 彻底解耦**：
  - 删除 `crates/tomoyo-core/src/quiet_md.rs`、`plugin_assets.rs` 中的 `detect_quiet_md_binary` / `quiet_md_tools_dir`、`InternalCommandKind::InstallQuietMd`、`HostMessage::InstallQuietMd`、`file_actions.rs` 中的 Quiet.md 专属分支以及前端全部 `quietMdPath` / `onInstallQuietMd` 特化代码；
  - 将 SHA-256 校验安装器下沉为 `plugins/official-md-editor` 内部自包含能力（通过标准 `PluginExecute { action_id: "install_quiet_md" }` 触发，保持二进制 `< 0.8 MB`）；
- [x] **插件中心与安装脚本轻装化**：
  - `PluginsTab.svelte` 仅把尚未外迁的 3 个内置模块（剪贴板/翻译/截图）列为内置可开关模块；`OCR` / `Quick Notes` / `Quiet.md` 与所有第三方插件一样仅在实际安装后显示，彻底消除未装先占位的“幽灵条目”；
  - `scripts/install-local.sh` 与 `scripts/install-local.ps1` 支持 `--core-only` 纯微内核安装，且默认不再向终端用户目录复制 `example-*` 示例插件（开发者可通过 `--with-examples` 显式安装）。

### Phase 2（`v2.8` · 已落地 ✅）：Protocol v3 通用底座与开放 DX
- [x] **Hybrid `WebView ↔ STDIO/WASM` 双向 RPC**：`tomoyo-plugin.toml` 支持 `[ui]` + `runtime` 混合形态；`shim_js.rs` 与 `docs/tomoyo.d.ts` 暴露 `window.tomoyo.callPlugin(method, params)`，经 `tomoyo-webview-host/src/plugin_rpc.rs` 严格按当前 `plugin_id` 沙箱转发至后端 `tomoyo/call`（`TomoyoPlugin::call`），窗闭即回收子进程；
- [x] **声明式 `[[settings]]` v3 与通用加密密钥库**：新增 `secret` / `hotkey` / `dir` / `file`、`section` 分组与 `visible_if` 条件显隐；`tomoyo-core` 加密密钥库放开 `plugin:<plugin_id>:<key>` 命名空间（Windows DPAPI / Linux Secret Service），`PluginHost` 仅向对应插件自身上下文注入其声明的密钥，绝不落明文配置；
- [x] **开放宿主扩展钩子**：实现 `[[tray_actions]]`（Windows / Linux 系统托盘动态注册）、`[[file_actions]]`（`QueryRouter` 零 I/O 扩展名二级菜单，并率先在 `plugins/official-md-editor` 落地 `.md` 关联）、`Permission::SelectionRead`（热键 `capture_selection = true` 无痕划词权限控制）与 `ExecuteResponse.open_webview` 直接拉起插件窗；
- [x] **开发者工具链增强**：`tomoyo-cli dev --devtools --call <method>` 支持直接拉起带 Inspector/DevTools 的 WebView 与 Hybrid RPC 命令行冒烟测试；`tomoyo-protocol` / `tomoyo-sdk` / `tomoyo-wasm-sdk` 升级至 Protocol v3（待维护者凭 `CARGO_REGISTRY_TOKEN` 推送 `crates.io` 新版）。

### Phase 3（`v2.9` → `v3.0` · 已落地 ✅）：重型官方功能全量外迁
- [x] **3a 离线 OCR 外迁**：删除 `tomoyo-webview-host/src/ocr_view.rs` 及核心内嵌 `ocr.html`；`plugins/builtin-ocr` 升级为 Protocol v3 Hybrid 插件（自带 `ui/index.html` `79.1 KB`、`[[tray_actions]]` 与图像 `[[file_actions]]`），走 `window.tomoyo.callPlugin` 双向 RPC；
- [x] **3b 多源划词翻译外迁**：移除 `TranslateProvider`、`tr_view.rs`、`TranslateTab.svelte` 及 `tomoyo-webview-host` 内嵌 `translator.html`；`plugins/official-translate`（`dev.tomoyo.translate`，`0.61 MB`）以 Hybrid 插件自包含承载多源翻译、`ui/index.html`（`104.1 KB`）、加密 `secret` 凭据、`Permission::SelectionRead` 划词热键及 `--quick-translate` 无锁短生命周期内联翻译入口；**`v3.1.0` 已彻底移除 `tomoyo-core/src/translate/engines.rs` 与核心重复翻译历史存储，实现 100% 单一事实源**；
- [x] **3c 原生截图与录屏外迁**：移除 `CaptureProvider`、`CaptureTab.svelte` 及核心内置截图热键/托盘分支；新增独立原生插件 `plugins/official-capture`（`dev.tomoyo.capture`，`0.57 MB` + 按需拉起 `tomoyo-capture` `1.49 MB`），完整承载 4 个热键、2 个托盘动作、图像贴图文件动作与 21 项声明式设置；
- [x] **3d 剪贴板历史工作台外迁**：从 `tomoyo-core` / `tomoyo-providers` / `tomoyo-webview-host` 彻底移除 `rusqlite`、`ClipboardProvider`、`clip_view.rs`、`clipboard.html`、`tomoyo-clip-img`、`Clip*` IPC 与前端 `ClipboardPluginCard.svelte` / `OFFICIAL_PLUGINS` 硬编码；新增 `plugins/official-clipboard`（`dev.tomoyo.clipboard`，Hybrid UI `99.4 KB`）并实现 OS 剪贴板监听按订阅者动态启停（纯内核模式下零监听、零数据库占用）；
- [x] **实测验收指标**：`tomoyo` 主程序由 `7.00 MB` 降至 `5.86 MB`，`tomoyo-webview-host` 由 `5.23 MB` 降至 `3.40 MB`（降幅 `-35%`，仅内嵌唯一页面 `settings.html` `281.0 KB` / `96.65 KB` gzip），100 轮休眠压测 `Handles +0`、`Private 21.33 MB`（零句柄泄露）。

### Phase 4（`v3.0+` · 已落地 ✅）：Starter Packs 按需装机与多平台独立插件分发
- [x] **按需装机套件（`StarterPacksCard.svelte`）**：插件市场上架「纯净微内核 / 高频效率套件 / 写作识字套件 / 官方全家桶」四组一键按需补装卡片，均经 SHA-256 验签安装且每款插件可随时独立卸载；
- [x] **多平台独立插件产物流水线**：更新 `.github/workflows/release.yml`、`ci.yml`、`packaging/tomoyo.iss`、`packaging/linux/PKGBUILD` 与本地安装/卸载脚本，保持 Core 分发包仅含 `3` 个核心二进制（0 插件），同时自动产出全部 6 款官方插件的 Linux/Windows 独立 ZIP 包（含 `tomoyo-capture` 自包含打包与独立 `SHA256SUMS` + SLSA 溯源证明），分片索引 10/10 全绿。
