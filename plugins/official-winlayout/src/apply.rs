//! OS backends for probing foreground window context and applying window tiling layouts.

use crate::layouts::PRESETS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWindowInfo {
    pub app: String,
    pub title: String,
}

#[allow(dead_code)]
pub fn build_kwin_layout_js(preset_id: &str, gap_px: i32, center_pct: i32) -> String {
    let gap = gap_px.clamp(0, 48);
    let pct = center_pct.clamp(50, 95);
    format!(
        r#"(function() {{
  function pickTarget() {{
    var a = workspace.activeWindow;
    if (a && !a.specialWindow && (!a.resourceClass || String(a.resourceClass).toLowerCase().indexOf('tomoyo') === -1)) return a;
    var list = workspace.stackingOrder || [];
    for (var i = list.length - 1; i >= 0; i--) {{
      var w = list[i];
      if (!w || w.specialWindow || w.minimized || !w.normalWindow) continue;
      var rc = String(w.resourceClass || '').toLowerCase();
      if (rc.indexOf('tomoyo') !== -1) continue;
      return w;
    }}
    return null;
  }}
  var w = pickTarget();
  if (!w) return;
  var p = {preset_json};
  if (p === 'max') {{
    var full = w.fullScreen;
    w.setMaximize(w.width < workspace.clientArea(KWin.MaximizeArea, w).width * 0.96, w.height < workspace.clientArea(KWin.MaximizeArea, w).height * 0.96);
    if (full) w.fullScreen = false;
    return;
  }}
  if (p === 'always-top') {{
    w.keepAbove = !w.keepAbove;
    return;
  }}
  if (p === 'minimize-others') {{
    var list = workspace.stackingOrder || [];
    for (var i = 0; i < list.length; i++) {{
      var o = list[i];
      if (o && o !== w && o.normalWindow && !o.minimized) o.minimized = true;
    }}
    return;
  }}
  var area = workspace.clientArea(KWin.MaximizeArea, w);
  var gap = {gap};
  var x0 = Math.round(area.x + gap), y0 = Math.round(area.y + gap);
  var iw = Math.max(240, Math.round(area.width - gap * 2));
  var ih = Math.max(180, Math.round(area.height - gap * 2));
  var hw = Math.floor((iw - gap) / 2), hh = Math.floor((ih - gap) / 2);
  var tw = Math.floor((iw - gap * 2) / 3);
  var rx = x0, ry = y0, rw = iw, rh = ih;
  if (p === 'left') {{ rw = hw; }}
  else if (p === 'right') {{ rx = x0 + hw + gap; rw = iw - hw - gap; }}
  else if (p === 'top') {{ rh = hh; }}
  else if (p === 'bottom') {{ ry = y0 + hh + gap; rh = ih - hh - gap; }}
  else if (p === 'tl') {{ rw = hw; rh = hh; }}
  else if (p === 'tr') {{ rx = x0 + hw + gap; rw = iw - hw - gap; rh = hh; }}
  else if (p === 'bl') {{ ry = y0 + hh + gap; rw = hw; rh = ih - hh - gap; }}
  else if (p === 'br') {{ rx = x0 + hw + gap; ry = y0 + hh + gap; rw = iw - hw - gap; rh = ih - hh - gap; }}
  else if (p === 'third-l') {{ rw = tw; }}
  else if (p === 'third-c') {{ rx = x0 + tw + gap; rw = tw; }}
  else if (p === 'third-r') {{ rx = x0 + (tw + gap) * 2; rw = iw - (tw + gap) * 2; }}
  else if (p === 'two-thirds-l') {{ rw = tw * 2 + gap; }}
  else if (p === 'two-thirds-r') {{ rx = x0 + tw + gap; rw = iw - tw - gap; }}
  else if (p === 'center') {{
    rw = Math.round((iw * {pct}) / 100); rh = Math.round((ih * {pct}) / 100);
    rx = x0 + Math.floor((iw - rw) / 2); ry = y0 + Math.floor((ih - rh) / 2);
  }}
  w.setMaximize(false, false);
  w.frameGeometry = Object.assign({{}}, w.frameGeometry, {{ x: rx, y: ry, width: rw, height: rh }});
}})();"#,
        preset_json = serde_json::to_string(preset_id).unwrap_or_else(|_| "\"left\"".into()),
    )
}

pub fn apply_preset(preset_id: &str, gap_px: i32, center_pct: i32) -> Result<String, String> {
    let label = PRESETS.iter().find(|p| p.id == preset_id).map(|p| p.title).ok_or_else(|| format!("未知窗口布局：{preset_id}"))?;
    #[cfg(target_os = "linux")]
    { apply_linux(preset_id, gap_px, center_pct)?; Ok(label.into()) }
    #[cfg(windows)]
    { apply_windows(preset_id, gap_px, center_pct)?; Ok(label.into()) }
    #[cfg(not(any(target_os = "linux", windows)))]
    { let _ = (gap_px, center_pct); Err(format!("当前平台暂未实现窗口布局控制：{label}")) }
}

pub fn probe_active_window() -> Option<ActiveWindowInfo> {
    #[cfg(target_os = "linux")]
    { probe_linux_active_window() }
    #[cfg(windows)]
    { probe_windows_active_window() }
    #[cfg(not(any(target_os = "linux", windows)))]
    { None }
}

#[cfg(target_os = "linux")]
fn apply_linux(preset_id: &str, gap_px: i32, center_pct: i32) -> Result<(), String> {
    if run_kwin_oneshot(&build_kwin_layout_js(preset_id, gap_px, center_pct)).is_ok() {
        return Ok(());
    }
    if preset_id == "max"
        && let Ok(out) = std::process::Command::new("wmctrl").args(["-r", ":ACTIVE:", "-b", "toggle,maximized_vert,maximized_horz"]).output()
        && out.status.success()
    {
        return Ok(());
    }
    if let Some(area) = probe_x11_workarea()
        && let Some(rect) = crate::layouts::compute_rect(preset_id, area, gap_px, center_pct)
    {
        let mvarg = format!("0,{},{},{},{}", rect.x, rect.y, rect.w, rect.h);
        if let Ok(out) = std::process::Command::new("wmctrl").args(["-r", ":ACTIVE:", "-e", &mvarg]).output()
            && out.status.success()
        {
            return Ok(());
        }
    }
    Err("未能定位支持的工作区窗口管理器（需 KDE KWin 6 / qdbus6 或 X11 wmctrl）".into())
}

#[cfg(target_os = "linux")]
fn run_kwin_oneshot(js: &str) -> Result<(), String> {
    let qdbus = if std::process::Command::new("qdbus6").arg("--help").output().is_ok() { "qdbus6" } else { "qdbus" };
    let name = format!("tomoyo_wl_{}", std::process::id());
    let path = std::env::temp_dir().join(format!("{name}.js"));
    std::fs::write(&path, js).map_err(|e| e.to_string())?;
    let p_str = path.to_string_lossy().into_owned();
    let _ = std::process::Command::new(qdbus).args(["org.kde.KWin", "/Scripting", "unloadScript", &name]).output();
    let load = std::process::Command::new(qdbus).args(["org.kde.KWin", "/Scripting", "loadScript", &p_str, &name]).output().map_err(|e| e.to_string())?;
    let res = if load.status.success() {
        let sid = String::from_utf8_lossy(&load.stdout).trim().to_string();
        if let Ok(id_num) = sid.parse::<i32>() && id_num >= 0 {
            let obj = format!("/Scripting/Script{id_num}");
            let _ = std::process::Command::new(qdbus).args(["org.kde.KWin", &obj, "run"]).output();
        }
        let _ = std::process::Command::new(qdbus).args(["org.kde.KWin", "/Scripting", "unloadScript", &name]).output();
        Ok(())
    } else {
        Err("qdbus loadScript failed".into())
    };
    let _ = std::fs::remove_file(&path);
    res
}

#[cfg(target_os = "linux")]
fn probe_x11_workarea() -> Option<crate::layouts::WorkArea> {
    let out = std::process::Command::new("wmctrl").arg("-d").output().ok()?;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if !line.contains(" * ") { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(wa_pos) = parts.iter().position(|&p| p == "WA:")
            && let (Some(xy), Some(wh)) = (parts.get(wa_pos + 1), parts.get(wa_pos + 2))
            && let (Some((xs, ys)), Some((ws, hs))) = (xy.split_once(','), wh.split_once('x'))
            && let (Ok(x), Ok(y), Ok(w), Ok(h)) = (xs.parse(), ys.parse(), ws.parse(), hs.parse())
        {
            return Some(crate::layouts::WorkArea { x, y, w, h });
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn probe_linux_active_window() -> Option<ActiveWindowInfo> {
    if let Ok(out) = std::process::Command::new("xdotool").args(["getactivewindow", "getwindowname"]).output()
        && out.status.success()
    {
        let title = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !title.is_empty() && !title.to_lowercase().contains("tomoyo") {
            return Some(ActiveWindowInfo { app: "X11 Active".into(), title });
        }
    }
    None
}

#[cfg(windows)]
mod win32 {
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct RECT { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }
    #[repr(C)]
    pub struct MONITORINFO { pub cb_size: u32, pub rc_monitor: RECT, pub rc_work: RECT, pub dw_flags: u32 }
    pub type HWND = *mut core::ffi::c_void;
    pub type HMONITOR = *mut core::ffi::c_void;
    pub const MONITOR_DEFAULTTONEAREST: u32 = 2;
    pub const SW_RESTORE: i32 = 9;
    pub const SW_MAXIMIZE: i32 = 3;
    pub const SW_MINIMIZE: i32 = 6;
    pub const HWND_TOPMOST: HWND = (-1isize) as HWND;
    pub const HWND_NOTOPMOST: HWND = (-2isize) as HWND;
    pub const SWP_NOSIZE: u32 = 0x0001;
    pub const SWP_NOMOVE: u32 = 0x0002;
    pub const SWP_NOZORDER: u32 = 0x0004;
    pub const SWP_NOACTIVATE: u32 = 0x0010;
    pub const GW_HWNDNEXT: u32 = 2;
    pub const GWL_EXSTYLE: i32 = -20;
    pub const WS_EX_TOPMOST: isize = 0x0008;
    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn GetForegroundWindow() -> HWND;
        pub fn GetWindowTextW(hwnd: HWND, lp_string: *mut u16, n_max_count: i32) -> i32;
        pub fn MonitorFromWindow(hwnd: HWND, dw_flags: u32) -> HMONITOR;
        pub fn GetMonitorInfoW(hmonitor: HMONITOR, lpmi: *mut MONITORINFO) -> i32;
        pub fn ShowWindow(hwnd: HWND, n_cmd_show: i32) -> i32;
        pub fn SetWindowPos(hwnd: HWND, insert_after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        pub fn GetWindowLongPtrW(hwnd: HWND, n_index: i32) -> isize;
        pub fn GetTopWindow(hwnd: HWND) -> HWND;
        pub fn GetWindow(hwnd: HWND, u_cmd: u32) -> HWND;
        pub fn IsWindowVisible(hwnd: HWND) -> i32;
    }
}

#[cfg(windows)]
fn apply_windows(preset_id: &str, gap_px: i32, center_pct: i32) -> Result<(), String> {
    use crate::layouts::{compute_rect, WorkArea};
    let mut hwnd = unsafe { win32::GetForegroundWindow() };
    if hwnd.is_null() { return Err("未找到活动前景窗口".into()); }
    if read_hwnd_title(hwnd).to_lowercase().contains("tomoyo") {
        let next = find_non_tomoyo_window();
        if !next.is_null() { hwnd = next; }
    }
    if preset_id == "max" {
        unsafe { win32::ShowWindow(hwnd, win32::SW_MAXIMIZE); }
        return Ok(());
    }
    if preset_id == "always-top" {
        let ex = unsafe { win32::GetWindowLongPtrW(hwnd, win32::GWL_EXSTYLE) };
        let z = if (ex & win32::WS_EX_TOPMOST) != 0 { win32::HWND_NOTOPMOST } else { win32::HWND_TOPMOST };
        unsafe { win32::SetWindowPos(hwnd, z, 0, 0, 0, 0, win32::SWP_NOMOVE | win32::SWP_NOSIZE | win32::SWP_NOACTIVATE); }
        return Ok(());
    }
    if preset_id == "minimize-others" {
        let mut cur = unsafe { win32::GetTopWindow(std::ptr::null_mut()) };
        while !cur.is_null() {
            if cur != hwnd && unsafe { win32::IsWindowVisible(cur) } != 0 && !read_hwnd_title(cur).trim().is_empty() {
                unsafe { win32::ShowWindow(cur, win32::SW_MINIMIZE); }
            }
            cur = unsafe { win32::GetWindow(cur, win32::GW_HWNDNEXT) };
        }
        return Ok(());
    }
    let mon = unsafe { win32::MonitorFromWindow(hwnd, win32::MONITOR_DEFAULTTONEAREST) };
    let mut mi = win32::MONITORINFO { cb_size: std::mem::size_of::<win32::MONITORINFO>() as u32, rc_monitor: win32::RECT { left: 0, top: 0, right: 0, bottom: 0 }, rc_work: win32::RECT { left: 0, top: 0, right: 0, bottom: 0 }, dw_flags: 0 };
    if unsafe { win32::GetMonitorInfoW(mon, &mut mi) } == 0 { return Err("读取显示器工作区失败".into()); }
    let area = WorkArea { x: mi.rc_work.left, y: mi.rc_work.top, w: mi.rc_work.right - mi.rc_work.left, h: mi.rc_work.bottom - mi.rc_work.top };
    let rect = compute_rect(preset_id, area, gap_px, center_pct).ok_or("无效布局预设")?;
    unsafe {
        win32::ShowWindow(hwnd, win32::SW_RESTORE);
        win32::SetWindowPos(hwnd, std::ptr::null_mut(), rect.x, rect.y, rect.w, rect.h, win32::SWP_NOZORDER | win32::SWP_NOACTIVATE);
    }
    Ok(())
}

#[cfg(windows)]
fn read_hwnd_title(hwnd: win32::HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { win32::GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 0 { String::new() } else { String::from_utf16_lossy(&buf[..len as usize]) }
}

#[cfg(windows)]
fn find_non_tomoyo_window() -> win32::HWND {
    let mut cur = unsafe { win32::GetTopWindow(std::ptr::null_mut()) };
    while !cur.is_null() {
        if unsafe { win32::IsWindowVisible(cur) } != 0 {
            let t = read_hwnd_title(cur);
            if !t.trim().is_empty() && !t.to_lowercase().contains("tomoyo") { return cur; }
        }
        cur = unsafe { win32::GetWindow(cur, win32::GW_HWNDNEXT) };
    }
    std::ptr::null_mut()
}

#[cfg(windows)]
fn probe_windows_active_window() -> Option<ActiveWindowInfo> {
    let fg = unsafe { win32::GetForegroundWindow() };
    let title = read_hwnd_title(fg);
    let target = if title.trim().is_empty() || title.to_lowercase().contains("tomoyo") { find_non_tomoyo_window() } else { fg };
    let t = read_hwnd_title(target);
    (!t.trim().is_empty()).then(|| ActiveWindowInfo { app: "Win32 Foreground".into(), title: t })
}

#[cfg(test)]
mod tests {
    use super::build_kwin_layout_js;

    #[test]
    fn kwin_script_uses_object_assign_and_skips_tomoyo() {
        let js = build_kwin_layout_js("left", 8, 72);
        assert!(js.contains("Object.assign"));
        assert!(js.contains("tomoyo"));
        assert!(js.contains("setMaximize(false, false)"));
    }
}
