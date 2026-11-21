# Tomoyo 极简现代界面与组件设计规范 (Minimalist Modern UI/UX Standards)

> **适用范围**：`tomoyo-app` 原生主启动器与自绘右键菜单、`tomoyo-capture` 原生截图/标注/贴图浮层，以及 `tomoyo-webview-host` 承载的设置中心、多源翻译工作台、剪贴板工作台、OCR 审校窗及后续富交互界面。  
> **设计哲学**：极简现代（Minimalist Modern，对标 Linear × Raycast 的克制秩序感）—— 低饱和中性表面、清晰信息层级、严格值域受控组件、双端同源视觉 Token、0 常驻 WebView 内存占用。

---

## 一、核心视觉语言与 Design Tokens

### 1.1 空间、圆角与层级阶梯
- **4px 基准空间网格**：外边距、内边距与组件间距严守 `4 / 8 / 12 / 16 / 20 / 24 px`。内容区最大宽度 `720 px`，卡片间距 `14 px`，行内控件间距 `8 px`。
- **圆角阶梯**：徽标/快捷键键帽 `5 px` → 控件（输入框/按钮/菜单项/标签）`7 px` → 浮层（下拉菜单/右键菜单/Tooltip）`10 px` → 内容卡片 `12 px`。
- **中性表面优先，单色强调克制**：90% 画面由 4 档中性灰构成（`--canvas` / `--surface` / `--elevated` / `--hover`）；强调色 `--accent` **仅用于唯一主操作按钮、激活态指示线与 Focus Ring**，禁止大面积刺眼纯蓝实心色块（侧边栏选中态采用柔和淡底 + 左侧 `2.5 px` 细指示条 + 加粗深色文字）。
- **三套主题同步**：浅色（Light）、深色（Dark）、午夜蓝（Midnight）共享完整语义 Token（含 `--border`、`--border-strong`、`--ring`、`--danger`、`--danger-soft`、`--ok`、`--warn`、`--shadow-pop`），并在 DWM 标题栏、滚动条、原生右键菜单与 WebView 内实时同步。

### 1.2 字体与清晰度（ClearType GDI 禁伪粗体铁律）
- 全栈使用内置私有字体 `Alibaba PuHuiTi 3.0 55 Regular`（400）与 `65 Medium`（500/600），等宽路径/代码/键帽配 `Cascadia Mono` / `Consolas`。
- **原生 Win32 铁律**：调用 `CreateFontW` 时无论 `regular_face` 还是 `medium_face`，`weight` 参数**固定传 `FW_NORMAL` (`400`)**，杜绝 GDI 算法伪粗体导致 `11–12 px` 中文小字发糊重影。
- **三级文字对比度**：`text_primary`（正文/标题）、`text_secondary`（副标题/说明，满足 WCAG AA）、`text_muted`（弱化快捷键、耗时、辅助元数据）。

---

## 二、全组件样式与交互规范 (Component Specification)

### 2.1 输入组件选型矩阵（禁止退化为自由文本）
| 数据特征 | 强制指定组件 | 禁止替代形式 |
|---|---|---|
| 布尔开关（设置项） | **Switch 滑块开关**（`36×20 px` 胶囊） | 原生裸 `<checkbox>` 或文本 |
| 少量互斥枚举（2–5 项） | **Segmented Control 分段选择器** / 卡片单选 | `<input type="number">` |
| 中等离散枚举（5–15 项） | **自定义极简 Select 浮层下拉框** | 浏览器原生 `<select>` / 数字框 |
| 大型可检索集合（>15 项） | **自定义 Searchable Combobox 下拉查询框** | 裸文本框或超长未筛选下拉 |
| 有界连续数值 | **Slider 范围滑块 + 等宽右对齐数值** | `<input type="number">` |
| 本机目录 / 文件路径 | **Path Picker 路径浏览框（`IFileOpenDialog`）** | `<textarea>` 手输路径 |
| 可组合安全语法标记 | **Preset Toggle Chips + 只读预览** | 裸文本框手写语法 |
| 全局组合快捷键 | **Hotkey Recorder（含实时 OS 冲突状态灯）** | 普通文本框 |

### 2.2 文本框（Text / Password / URL / Search / Textarea）
- **全类型覆盖**：统一样式必须显式覆盖 `text` / `password` / `url` / `search`，严禁漏写导致某类输入框退化为系统原始黑框。
- **几何与状态**：标准高 `34 px`（紧凑 `30 px`），圆角 `7 px`，水平内边距 `10 px`，`1 px` 细描边；Hover 边框加深；Focus 显 `1 px --accent` 边框 + `0 0 0 3px var(--ring)` 柔和光环。
- **内置增强槽**：搜索框前置 `14 px` 放大镜图标 + 有值时右侧出现 `×` 清空按钮；密码框右侧内置显隐眼睛图标；非法输入仅边框切红 + 下方 `12 px` 红色行内说明，不挤压兄弟控件。

### 2.3 下拉选择框与下拉查询框（Select & Searchable Combobox）
- **弃用原生 `<select>`**：在所有重构后的 WebView 页面中，下拉框统一采用自研无障碍浮层 `Select` / `Combobox`。
- **触发器**：与文本框严格等高（`34 px`）、同圆角（`7 px`）、同边框语义，右侧固定 `14 px` 细 Chevron 图标（展开时平滑旋转 `180°`）。
- **浮层菜单**：距触发器 `4 px`，圆角 `10 px`，内边距 `4 px`，最大高度 `240 px` + 自定义细滚动条；菜单项高 `30–32 px`、圆角 `6 px`，支持完整键盘导航（`↑ / ↓ / Enter / Esc`）与选中项右侧细勾 `✓`。
- **下拉查询框（Combobox）**：浮层顶部内嵌吸附式搜索栏，实时匹配过滤并支持自定义新条目（如 `+ keepass.exe`），已选条目在下方以可关闭轻量胶囊展示。

### 2.4 目录 / 文件浏览框（Path Picker）
- **单目录模式**：横向复合控件——左侧为只读等宽路径框（前置 `14 px` 文件夹微图标 + 溢出省略 + Hover Tooltip 显示完整路径），右侧紧邻 `Secondary`「浏览…」按钮及有自定义值时显现的 `Ghost` 重置图标按钮。
- **多目录模式**：顶栏右侧放「浏览添加目录…」按钮，下方以无厚重外框的紧凑行列表（行高 `36 px`）展示全部已校验目录，每行右侧仅在 Hover 显现轻量删除图标。

### 2.5 各类按钮（Button Matrix）与 Switch 开关
- **不折行铁律**：所有按钮强制 `white-space: nowrap; flex-shrink: 0; display: inline-flex; align-items: center; justify-content: center; gap: 6px`，严禁出现中文按钮纵向折成两行。
- **尺寸阶梯**：`sm`（高 `28 px`，`12 px` 字，工具条/行内）、`md`（高 `34 px`，`13 px` 字，标准表单）、`lg`（高 `38 px`，`13.5 px` 字）。
- **5 种语义变体**：
  1. `Primary`：每屏最多 1 个主 CTA，实心 `--accent` 底 + 白字 + 微压感（`active: scale(0.98)`）；
  2. `Secondary`：常规功能按钮，中性卡片底 + `1 px` 边框 + Hover 柔和抬升；
  3. `Ghost`：表格/工具条轻量操作，默认无边框透明底，Hover 显现 `--hover` 柔底；
  4. `Destructive`（`danger`）：表单顶部或行内删除默认采用「透明/柔框 + 弱红字」，Hover 才显浅红底（`--danger-soft`）+ 鲜红字（`--danger`），避免满屏刺眼红框；
  5. `Icon Button`：正方形（`28×28` / `32×32 px`），必须提供 `title`/Tooltip 说明。

### 2.6 右键菜单与菜单选项（Native Win32 与 WebView 双端同源）
- **容器**：最小宽 `192 px`、最大宽 `340 px`，圆角 `10 px`，内边距 `4–5 px`，高一层表面底色 + `1 px` 细描边 + 柔和微阴影（Win32 端配合 `DWMWCP_ROUND`）。
- **菜单项**：统一高度 `32 px`、圆角 `6 px`、左右内边距 `8 px`，严格三段对齐：`[16 px 图标/勾选槽]` + `[单行主文案 flex-1 nowrap]` + `[右对齐等宽弱化快捷键 11.5 px]`。
- **悬停与语义态**：常规 Hover 采用低饱和柔底（不喧宾夺主）；**危险操作（如退出/删除）必须独立置于底部分隔线下方，仅 Hover 显现柔和红底 + 鲜红文字**；禁用项透明度 `42%` 且不响应 Hover。
- **分隔线**：`1 px` 细线，左右内缩 `8 px`，上下外距 `4 px`。

### 2.7 全局热键录制器（Live Hotkey Health）
- **真实 OS 状态回显**：主进程注册全部热键后将真实 Win32 `RegisterHotKey` 结果写入 `hotkey_status.json`，设置中心实时展示状态微点与徽标：`● 已生效`（绿）/ `⚠ 已被其他软件占用`（红，提醒改键或停用冲突软件）/ `○ 已停用`（灰），严禁在 OS 注册失败时误报绿色成功。
- **扩展键位支持**：支持 `A–Z`、`0–9`、`F1–F12`、`Space`、`Tab`、常用标点（`` ` - = [ ] \ ; ' , . / ``）及 `Insert / Delete / Home / End / PageUp / PageDown / PrintScreen`；右侧提供一键清空 `×` 与自定义预设下拉。

---

## 三、专项工作台与原生浮层 UX 规范

1. **多源翻译工作台（Translator）**：
   - 极简单行顶栏（自定义语言 `Select` ⇄ 切换 + 可选输入防抖自动翻译 + 紧凑图标工具组）；
   - 原文区无冗余重框，多引擎译文卡片配统一紧凑头部、复制/朗读/收藏图标按钮，请求态使用骨架屏 shimmer 占位；支持右键菜单快捷操作。
2. **原生截图与贴图浮层（`tomoyo-capture.exe`）**：
   - 选区动作条使用圆角暗色浮条 + 纯 Rust 抗锯齿 SDF 矢量图标（`16×16`），垂直细分隔线区隔「标注工具 | 撤销 | 动作」；Hover 显示中文快捷键 Tooltip；
   - 仅在激活标注工具时，在主浮条下方/上方展开轻量二级属性条（3 档圆点线宽 + 6 色圆形色点）；
   - 放大镜采用圆角卡片 HUD（`11×11` 网格 + 中心准星 + 实时色块点 + 坐标 + `Shift` 切换 `HEX / RGB` + `C` 复制提示）；选区尺寸使用半透明圆角胶囊显示。

---

## 四、双层防御校验与前端运行时门禁

1. **后端权威清洗（`TomoyoConfig::sanitize()`）**：前端受控组件为第一层防线，Rust 后端在加载/保存/IPC 时统一钳制边界、拦截盘符根目录/系统目录/非 HTTP(S) 协议并回传修正摘要。
2. **前端工程化与零异常门禁**：WebView UI 由 `frontend/`（Svelte 5 + Tailwind CSS v4 + TypeScript + Lucide）编译为单文件内联产物嵌入 `tomoyo-webview-host.exe`（仍保持关闭即退、0 MB 常驻）；`scripts/check_hygiene.ps1` 强制源码全仓 `≤ 450` 行，并执行 `scripts/check_webview_ui.mjs` 在真实 DOM 运行时中验证全部 4 个编译产物 0 JS 报错。
