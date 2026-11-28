# Wayland `org.freedesktop.portal.GlobalShortcuts` Pre-Research & Integration Design

## 1. 背景与当前状态
Tomoyo Linux 端目前采用两层全局快捷键策略：
- **KDE Plasma 6 Wayland/X11**：通过 `org.kde.kglobalaccel` D-Bus 原生注册与接管，零弹窗、支持设置中心无感重绑与动态热重载；
- **其余 X11 / XWayland 桌面**：支持 XGrabKey 或环境适配器。

但在非 KDE 的严格 Wayland 合成器（如 Hyprland、Sway 配合 `xdg-desktop-portal-hyprland` / `xdg-desktop-portal-wlr`、GNOME 47+ `xdg-desktop-portal-gnome`）中，X11 `XGrabKey` 无法捕获原生 Wayland 窗口焦点，必须通过标准化 **XDG Desktop Portal** 的 `org.freedesktop.portal.GlobalShortcuts` 接口向合成器申领全局快捷键。

## 2. 协议流程与版本差异
- **D-Bus 对象**：`org.freedesktop.portal.Desktop` (`/org/freedesktop/portal/desktop`)，`org.freedesktop.portal.GlobalShortcuts` (`version = 1` / `2`)
- **标准调用链**：
  1. `CreateSession({ handle_token, session_handle_token })` → 获取长期 `session_handle`；
  2. `BindShortcuts(session_handle, [(id, { description, preferred_trigger })], parent_window, options)` → 返回用户授权确认后的真实绑定列表（v2 新增无需弹窗仅更新触发器或系统设置跳转参数）；
  3. 监听 `Activated(session_handle, shortcut_id, timestamp, options)` 与 `Deactivated(...)` 信号，直接唤醒 Tomoyo 对应槽位（主启动器 / 剪贴板 / 翻译 / 截屏 / WorkFlow）；
  4. 通过 `ListShortcuts(session_handle, options)` 同步合成器侧实际被修改后的键位回写至本地 UI 状态显示（对应 `hotkey_status.json`）。

## 3. 合成器实测矩阵与接入结论
| 桌面环境 / 合成器 | Portal 后端实现 | `BindShortcuts` UX 行为 | 对 Tomoyo 的接入结论 |
|---|---|---|---|
| **KDE Plasma 6.8+** | `xdg-desktop-portal-kde` | 通过 KGlobalAccel 授权一次，支持系统设置调整 | **保持 `KGlobalAccel` 为第一优先级**（无额外授权弹窗且可直接重绑热键）；Portal 作为跨桌面通用后端备用 |
| **Hyprland** | `xdg-desktop-portal-hyprland` | 遵循 `BindShortcuts` `preferred_trigger` 与 Hyprland 绑定同步 | 适用 Portal `Activated` 信号通道（免用户手改 `hyprland.conf`） |
| **GNOME 47+** | `xdg-desktop-portal-gnome` | 弹出 GNOME 快捷键授权对话窗，由 Mutter 托管拦截 | 需引导用户在 Portal 授权后以 `ListShortcuts` 回读实际触发键 |

## 4. 本阶段落地范围与后续计划
- **Wave 2 已落地**：`tomoyo-cli doctor` 已接入 `org.freedesktop.portal.GlobalShortcuts` 运行时可用性探测；`official-winlayout` 亦在 `wayland_wm.rs` 率先完成 Hyprland/Sway/KWin 多合成器支持。
- **Stage 4 Wave 1 已落地 ✅**：已在 [`portal_hotkeys.c`](../crates/tomoyo-app/src/linux/portal_hotkeys.c) 与 [`portal_hotkeys.rs`](../crates/tomoyo-app/src/linux/portal_hotkeys.rs) 实现原生 `GlobalShortcuts` Portal 回退通道（复用 `gio-2.0`，零新增 crate，主进程体积严守 `≤ 6.2 MB`）：KDE Plasma 继续优先使用 `org.kde.kglobalaccel`，当 `KGlobalAccel` 未在会话总线运行且检测到 `GlobalShortcuts` Portal v1+ 时自动完成 `CreateSession` → `BindShortcuts` → `Activated` 响应并写入热键体检状态。
- **后续增强（Stage 4 Wave 2）**：接入 Portal `ListShortcuts` 实绑键位回读与托盘状态联动，覆盖 Hyprland / GNOME 47+ 实机触发键重映射回写。
