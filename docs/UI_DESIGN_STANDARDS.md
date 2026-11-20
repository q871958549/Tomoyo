# Tomoyo 界面组件选型与视觉设计规范 (UI/UX & Component Design Standards)

> **适用范围**：`tomoyo-app` 原生主启动器窗口、自定义弹出菜单，以及 `tomoyo-webview-host` 设置中心与内置 WebView 插件界面。  
> **基本目标**：以值域受控组件替代无约束自由文本输入，降低误操作与异常配置风险；确保 ClearType 字体清晰度与界面层级在浅色、深色、午夜蓝三套主题下保持一致。

---

## 一、输入组件选型规范 (Control Selection Matrix)

新增或修改任何设置项与表单字段时，**必须按数据的实际值域与系统风险特征选择最窄约束的组件**。能通过选择完成的交互，不允许退化为自由文本框。

| 数据类型与值域特征 | 指定组件 | 典型适用场景 | 禁止替代形式 |
|---|---|---|---|
| **布尔开关**（是 / 否） | 复选框 / 开关 (`checkbox` / `switch`) | 开机自启、自动预览、显示筛选栏、桌面整理目录识别 | 文本输入 `true/false` |
| **少量互斥枚举**（2–5 项） | 分段选择器 (Segmented Control) 或卡片单选 | 界面语言、主题预设、目录扫描深度（`1 / 2 / 3 / 4 层`） | `<input type="number">` 或自由文本 |
| **中等离散枚举**（5–15 项） | 下拉选择框 (`<select>`) | 剪贴板最大保留条数（`100..2000`）、更新检查周期、更新源模式、快捷网页引擎预设 | `<input type="number">` |
| **大型动态集合 / 超过 15 项可筛选列表** | 下拉搜索框 (Combobox / Searchable Select) | 字体列表、进程选择、插件/命令快速定位、多时区或语言扩展列表 | 纯文本框或超长未分组下拉框 |
| **连续有界数值**（带明确上下限与步长） | 范围滑块 (`<input type="range">` + 实时数值显示) | 休眠延迟（`5–120 s`）、窗口宽度（`520–1280 px`）、可见行数（`4–16 行`）、Everything 全局混排条数（`0–10`）与专属条数（`10–50`） | 裸 `<input type="number">` |
| **本机文件或目录路径** | 原生系统浏览框（Win32 `IFileOpenDialog`）+ 结构化只读路径列表与逐项移除按钮 | 自定义应用与便携软件扫描目录、导出路径、插件包选择 | 多行 `<textarea>` 手输路径 |
| **可组合的安全标记 / 语法片段** | 预设多选标签组 (Preset Toggle Chips) + 只读表达式预览 | Everything 默认排除规则（`!*.tmp`、`!node_modules`、`!.git`、`!*.log`、`!target` 等） | 裸文本框手写易错查询语法 |
| **系统快捷键组合** | 专用按键录入框 (Hotkey Recorder) + 快捷预设下拉框 | 全局呼出快捷键（强制要求含 `Alt / Ctrl / Shift / Win` 修饰键） | 文本手输键名 |
| **确实需要自由输入的受限标识** | 强校验单行文本框 (`<input type="text" maxlength=...>`) + 实时行内错误提示 | 网页搜索关键词/名称/URL 模板、自定义 Fork 仓库（仅在下拉框显式切换为「自定义」后展开） | 不做格式与协议校验的输入框 |

### 1.1 明确禁止的表单反模式
1. **禁止对有界参数使用 `<input type="number">`**（例如目录扫描深度、结果上限、剪贴板容量）：浏览器允许手动键入 `12000`、负数或超大值，既绕过视觉预期，也易引发深层目录递归卡顿或内存飙升。
2. **禁止使用 `<textarea>` 承载操作系统路径列表**：用户容易误填盘符根目录（如 `C:\`）、网络共享路径（`\\server\share`）、`..` 路径穿越或系统目录（`C:\Windows`），且难以在输入阶段给出明确反馈。
3. **禁止对外部启动目标跳过协议与格式校验**：凡涉及 `ShellExecuteW` 或网络请求的 URL 模板与仓库地址，禁止无约束保存或直接执行。

---

## 二、双层校验与配置自愈契约 (Defense-in-Depth Validation)

前端控件仅构成第一层约束；用户仍可能直接编辑 `%LOCALAPPDATA%\Tomoyo\config.toml` 或导入旧版配置，因此所有配置项必须满足**前端约束 + Rust 后端强制清洗**的双层防护：

1. **前端即时拦截**：
   - 受控组件自身限定合法值域；
   - 必须使用单行文本框的字段（网页搜索、自定义 Fork 仓库）在输入时实时校验，存在非法行时标红边框（`.input-invalid`）、显示明确行内错误说明（`.field-err`），并暂停自动保存，防止脏数据写入磁盘。
2. **后端权威清洗（`TomoyoConfig::sanitize()`）**：
   - 在 `load_from_path()`、`save_to_path()` 与 `SaveSettings` IPC 入口统一执行；
   - **扫描深度**：严格钳制在 `1..=4`（历史异常值如 `12000` 在加载时自动修复为 `4` 并写回磁盘）；
   - **扫描目录**：经 `validate_scan_dir()` 拦截空路径、控制字符、盘符根目录（`C:\`）、UNC 路径（`\\...`）、`..` 相对段及高危系统目录（`Windows`、`ProgramData`、`$Recycle.Bin`、`System Volume Information`、`Recovery`、`AppData\Local\Temp`），列表上限 16 项并大小写不敏感去重；
   - **网页搜索模板**：关键词限定 1–16 位 `[a-z0-9_-]` 且不可与内置命令前缀（`f`/`ev`/`cb`/`w`/`kill`/`ps`/`snip`/`=`/`b64`/`b64d`）冲突；URL 必须以 `https://` 或 `http://` 开头、无空白字符且包含 `{q}`，拒绝 `file://`、`cmd:`、`powershell:` 等协议；
   - **更新源仓库**：必须严格符合 `owner/repo` 字符集规范且不含 `..`，不合法时回退官方仓库；
   - **IPC 安全门槛**：`OpenExternal` 仅放行 `http://` / `https://`；`OpenPath` 校验本地路径存在；
   - **状态回执**：若后端触发自动钳制或剔除，通过 `settings_saved` 事件回传修正后的配置与提示摘要，前端同步刷新状态并以警示色提示用户。

---

## 三、主启动器字体渲染与视觉层级规范 (Typography & Contrast)

### 3.1 ClearType GDI 私有字体禁伪粗体原则
Tomoyo 内置 `Alibaba PuHuiTi 3.0 55 Regular` 与 `Alibaba PuHuiTi 3.0 65 Medium` 两套独立 TrueType 字重文件，不包含 `700 Bold` 字重。

- **规则**：调用 Win32 `CreateFontW` 时，无论使用 `regular_face` 还是 `medium_face`，**`weight` 参数必须固定传入 `FW_NORMAL` (`400`)**。
- **原因**：向无对应粗体字族的私有字体传入 `FW_MEDIUM` (`500`)、`FW_SEMIBOLD` (`600`) 或 `FW_BOLD` (`700`)，会触发 Windows GDI 算法伪粗体（在 ClearType 亚像素抗锯齿上叠加横向 1 px 双重描影），导致 `11–12 px` 中文小字出现明显模糊与边缘重影。
- **字阶规范**（`FontPack`，随 `dpi_scale` 线性缩放）：
  - `title`：`15.5 DIP`，`medium_face` + `FW_NORMAL`（列表主标题、预览标题）
  - `subtitle`：`12.5 DIP`，`regular_face` + `FW_NORMAL`（列表副标题）
  - `chip_active`：`12.5 DIP`，`medium_face` + `FW_NORMAL`（当前激活筛选标签、模式徽标、菜单选中项）
  - `chip`：`12.5 DIP`，`regular_face` + `FW_NORMAL`（未激活/悬停筛选标签、菜单普通项）
  - `badge`：`11.5 DIP`，`medium_face` + `FW_NORMAL`（右侧小徽标、分类图标字母、当前行 `Enter` 提示）
  - `footer`：`12.0 DIP`，`regular_face` + `FW_NORMAL`（底部状态栏、快捷键次级提示）
  - `preview_body` / `preview_mono`：`12.5 / 12.0 DIP`，`regular_face` / `Consolas` + `FW_NORMAL`

### 3.2 三级文字对比度与色板语义
避免仅使用两档文字颜色导致副标题、标签与快捷键提示层次混乱。所有主题（`light` / `dark` / `midnight`）均提供明确的三级文字与语义表面令牌（`ThemePalette`）：

1. **文字层级**：
   - `text_primary`：主标题与输入文字（高对比度，正文核心焦点）；
   - `text_secondary`：列表副标题、未激活筛选标签、底部状态提示（清晰易读，避免浅灰发糊，满足 WCAG AA 对比度要求）；
   - `text_muted`：非当前行快捷键提示（`Alt+2..8`）、内存与命中计数（明确弱化但不失真）。
2. **筛选标签与选中态（避免生硬黑块）**：
   - 筛选栏高度 `38 DIP`，标签高度 `24 DIP`，圆角 `7 DIP`，水平内边距 `11 DIP`，间距 `6 DIP`；
   - **未激活标签**：使用独立表面色（`chip_bg`）+ 精确 `1.0 DIP` 细描边（`chip_border`）+ `chip_text`，与窗口背景自然分离；
   - **悬停标签**：柔和抬升背景（`chip_hover_bg`）与加深边框（`chip_hover_border`）；
   - **激活标签 / 菜单高亮 / 列表选中项**：采用低饱和主题浅底（`chip_active_bg` / `card_sel`）配合 `1.0 DIP` 强调边框（`chip_active_border` / `sel_border`）与深色主题文字（`chip_active_text`），替代突兀的纯黑实心色块，在浅色、深色、午夜蓝三套主题下保持统一节奏。
3. **几何边界与细节**：
   - SDF 细描边半宽固定为 `0.5 * dpi_scale`（1 逻辑像素清晰细线）；
   - 底部右侧缩放角标与状态统计保留安全间距（`≥ 18 DIP`），防止角标与文本重叠。

---

## 四、窗口生命周期与交互状态反馈

1. **设置中心全局单实例**：使用系统命名互斥体（`Local\TomoyoSettingsCenterSingleton`）与已运行窗口枚举查找；重复点击托盘「设置」或按 `Ctrl+,` 时直接恢复并置前已有窗口，不重复拉起第二个 WebView2 进程。
2. **主窗口响应不阻塞**：图标提取、文件/图片预览与 Everything IPC 均在后台线程异步完成；输入与筛选切换先展示内存结果，再无抖动合并文件结果。
3. **可预期的状态反馈**：设置中心底部状态栏区分常规保存成功（绿色）与后端自动纠正提示（琥珀色 `.warn`），并确保界面控件状态与最终落盘的 `config.toml` 严格一致。
