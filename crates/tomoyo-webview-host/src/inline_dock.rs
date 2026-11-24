//! Borderless preview-pane docking, host bootstrap selection, & one-click window pop-out.

use crate::db::PluginDb;
use crate::shim_js::build_init_script;
use crate::win_theme::{apply_host_window_theme, apply_window_icons, to_file_url};
use crate::{build_settings_init_script_for, LaunchArgs};
use std::path::PathBuf;

pub struct HostBootstrapData {
    pub init_script: String,
    pub entry_url: String,
}

pub fn prepare_host_bootstrap(opts: &LaunchArgs, db: &PluginDb) -> HostBootstrapData {
    let settings_mode = opts.settings_mode;
    let init_script = if settings_mode {
        build_settings_init_script_for(&opts.query)
    } else {
        build_init_script(
            &opts.plugin_id,
            &opts.query,
            &opts.theme,
            opts.utools_compat,
            &db.snapshot_json(),
        )
    };
    let entry_url = if settings_mode {
        String::new()
    } else {
        to_file_url(&opts.entry)
    };
    HostBootstrapData {
        init_script,
        entry_url,
    }
}
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    SetWindowTextW, ShowWindow, GWL_EXSTYLE, GWL_STYLE, GWLP_HWNDPARENT, HICON, HWND_TOP,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_SHOW, WNDPROC,
    WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};

pub fn inline_hwnd_marker_path() -> PathBuf {
    tomoyo_core::logs_dir().join("active_inline_host.json")
}

pub fn write_inline_hwnd_marker(hwnd: HWND, pid: u32, plugin_id: &str) {
    let payload = serde_json::json!({
        "hwnd": hwnd.0 as isize,
        "pid": pid,
        "plugin_id": plugin_id,
    });
    let _ = std::fs::write(inline_hwnd_marker_path(), payload.to_string());
}

pub fn clear_inline_hwnd_marker(hwnd: HWND) {
    let p = inline_hwnd_marker_path();
    if let Ok(raw) = std::fs::read_to_string(&p)
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw)
        && v.get("hwnd").and_then(|h| h.as_i64()) == Some(hwnd.0 as isize as i64)
    {
        let _ = std::fs::remove_file(&p);
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn create_inline_dock_window(
    cls: PCWSTR,
    wndproc: WNDPROC,
    title: &str,
    theme: &str,
    owner_hwnd: isize,
    dock_rect: (i32, i32, i32, i32),
    icons: (Option<HICON>, Option<HICON>),
) -> HWND {
    unsafe {
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
        let wc = windows::Win32::UI::WindowsAndMessaging::WNDCLASSEXW {
            cbSize: std::mem::size_of::<windows::Win32::UI::WindowsAndMessaging::WNDCLASSEXW>() as u32,
            lpfnWndProc: wndproc,
            hInstance: hinst.into(),
            hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                None,
                windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
            )
            .unwrap_or_default(),
            hbrBackground: crate::win_theme::create_theme_bg_brush(theme),
            lpszClassName: cls,
            ..Default::default()
        };
        let _ = windows::Win32::UI::WindowsAndMessaging::RegisterClassExW(&wc);
        let owner = HWND(owner_hwnd as *mut std::ffi::c_void);
        let title_w = HSTRING::from(title);
        let (x, y, w, h) = dock_rect;
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            cls,
            PCWSTR(title_w.as_ptr()),
            WS_POPUP | WS_CLIPCHILDREN | WS_VISIBLE,
            x,
            y,
            w.max(220),
            h.max(180),
            Some(owner),
            None,
            Some(hinst.into()),
            None,
        )
        .expect("CreateWindowExW inline dock failed");
        apply_window_icons(hwnd, icons);
        apply_host_window_theme(hwnd, theme);
        let _ = ShowWindow(hwnd, SW_SHOW);
        hwnd
    }
}

/// Promotes a docked borderless inline panel into an independent top-level window with title bar & logo.
#[allow(clippy::too_many_arguments)]
pub unsafe fn promote_inline_to_window(
    hwnd: HWND,
    width: i32,
    height: i32,
    title: &str,
    theme: &str,
    icons: (Option<HICON>, Option<HICON>),
    notify_hwnd: Option<isize>,
) {
    unsafe {
        clear_inline_hwnd_marker(hwnd);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_TOOLWINDOW.0 as isize));
        SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0) as isize);
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, 0);

        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let (wx, wy, ww, wh) = if GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &mut mi,
        )
        .as_bool()
        {
            let wa: RECT = mi.rcWork;
            (wa.left, wa.top, wa.right - wa.left, wa.bottom - wa.top)
        } else {
            (0, 0, 1280, 800)
        };
        let target_w = width.clamp(420, (ww - 40).max(420));
        let target_h = height.clamp(320, (wh - 40).max(320));
        let tx = wx + (ww - target_w) / 2;
        let ty = wy + (wh - target_h) / 2;

        let tw = HSTRING::from(title);
        let _ = SetWindowTextW(hwnd, PCWSTR(tw.as_ptr()));
        apply_window_icons(hwnd, icons);
        apply_host_window_theme(hwnd, theme);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            tx,
            ty,
            target_w,
            target_h,
            SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOZORDER,
        );
        let _ = SetWindowPos(
            hwnd,
            None,
            tx,
            ty,
            target_w,
            target_h,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
        let _ = SetForegroundWindow(hwnd);
    }
    if let Some(target) = notify_hwnd {
        crate::bridge_os::notify_parent_mode(target, 7);
    }
}

/// Injects a sleek non-intrusive inline pill control (`独立窗口` / `收起`) when docked inside Tomoyo.
pub fn inline_dock_overlay_js(theme: &str) -> String {
    let is_light = theme.eq_ignore_ascii_case("light");
    let bg = if is_light { "rgba(255,255,255,0.9)" } else { "rgba(24,25,36,0.88)" };
    let fg = if is_light { "#151822" } else { "#f3f4f8" };
    let bd = if is_light { "#d5dbe7" } else { "#2c2e40" };
    format!(
        r#"(function(){{
  if (window.__TOMOYO_INLINE_PILL__) return;
  window.__TOMOYO_INLINE_PILL__ = true;
  function mount(){{
    if (!document.body || document.getElementById('__tomoyo_dock_bar__')) return;
    const bar = document.createElement('div');
    bar.id = '__tomoyo_dock_bar__';
    bar.style.cssText = 'position:fixed;right:10px;top:8px;z-index:2147483647;display:flex;gap:6px;align-items:center;padding:3px 6px;border-radius:999px;background:{bg};border:1px solid {bd};backdrop-filter:blur(8px);box-shadow:0 2px 10px rgba(0,0,0,0.18);font-family:system-ui,sans-serif;';
    function mkBtn(lbl, cmd){{
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = lbl;
      b.style.cssText = 'border:0;background:transparent;color:{fg};font-size:11.5px;padding:3px 8px;border-radius:999px;cursor:pointer;line-height:1.2;';
      b.onmouseenter = () => b.style.background = 'rgba(128,128,128,0.18)';
      b.onmouseleave = () => b.style.background = 'transparent';
      b.onclick = () => {{
        if (cmd === 'pop_out_window') bar.remove();
        window.chrome?.webview?.postMessage(JSON.stringify({{ cmd }}));
      }};
      return b;
    }}
    bar.appendChild(mkBtn('⧉ 独立窗口', 'pop_out_window'));
    bar.appendChild(mkBtn('✕', 'close'));
    document.body.appendChild(bar);
  }}
  if (document.body) mount(); else window.addEventListener('DOMContentLoaded', mount, {{ once: true }});
}})();"#
    )
}
