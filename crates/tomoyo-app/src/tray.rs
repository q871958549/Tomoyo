//! System tray icon & theme-matched context menu with RAII cleanup.

use crate::brand::generate_icon_bgra;
use crate::popup_menu::{show_themed_popup_menu, PopupEntry, PopupMenuStyle};
use crate::renderer::ThemePalette;
use tomoyo_core::Lang;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, HBITMAP};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, LoadImageW, SendMessageW, HICON, ICONINFO, ICON_BIG,
    ICON_SMALL, IMAGE_ICON, LR_DEFAULTSIZE, WM_APP, WM_SETICON,
};

pub const WM_TRAY_CALLBACK: u32 = WM_APP + 10;

pub const MENU_ID_SHOW: u32 = 2001;
pub const MENU_ID_SETTINGS: u32 = 2002;
pub const MENU_ID_UPDATE: u32 = 2008;
pub const MENU_ID_STATS: u32 = 2003;
pub const MENU_ID_CONFIG: u32 = 2004;
pub const MENU_ID_RELOAD: u32 = 2005;
pub const MENU_ID_GC: u32 = 2006;
pub const MENU_ID_QUIT: u32 = 2007;
pub const MENU_ID_PLUGIN_TRAY_BASE: u32 = 2200;

pub struct TrayIcon {
    hwnd: HWND,
    hicon: HICON,
    owns_icon: bool,
    installed: bool,
    hotkey_label: String,
}

impl TrayIcon {
    pub fn install(hwnd: HWND, hotkey_label: &str) -> Self {
        let (hicon, owns_icon) = load_embedded_hicon()
            .map(|h| (h, false))
            .unwrap_or_else(|| (create_modern_white_hicon(32).unwrap_or_default(), true));

        if !hicon.is_invalid() {
            unsafe {
                SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(windows::Win32::Foundation::WPARAM(ICON_SMALL as usize)),
                    Some(windows::Win32::Foundation::LPARAM(hicon.0 as isize)),
                );
                SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(windows::Win32::Foundation::WPARAM(ICON_BIG as usize)),
                    Some(windows::Win32::Foundation::LPARAM(hicon.0 as isize)),
                );
            }
        }

        let installed = apply_nid_icon(hwnd, hicon, hotkey_label, NIM_ADD);
        if !installed {
            tracing::warn!("Initial Shell_NotifyIconW(NIM_ADD) failed; will retry on TaskbarCreated");
        }
        Self {
            hwnd,
            hicon,
            owns_icon,
            installed,
            hotkey_label: hotkey_label.to_string(),
        }
    }

    /// Re-registers the tray icon after Explorer restarts (`TaskbarCreated` broadcast).
    pub fn reinstall(&mut self) {
        self.installed = apply_nid_icon(self.hwnd, self.hicon, &self.hotkey_label, NIM_ADD);
        tracing::info!(
            "Re-registered system tray icon after taskbar rebuild: ok={}",
            self.installed
        );
    }

    pub fn update_tooltip(&mut self, hotkey_label: &str) {
        self.hotkey_label = hotkey_label.to_string();
        if self.installed {
            let _ = apply_nid_icon(self.hwnd, self.hicon, &self.hotkey_label, NIM_MODIFY);
        } else {
            self.reinstall();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show_context_menu(
        &self,
        lang: Lang,
        pal: ThemePalette,
        dpi_scale: f32,
        font_regular: &str,
        font_medium: &str,
        plugin_actions: &[tomoyo_plugin_host::ActivePluginTrayAction],
    ) -> Option<u32> {
        let mut entries = vec![
            PopupEntry::item(MENU_ID_SHOW, lang.pick("显示 Tomoyo", "Show Tomoyo"), None, false),
            PopupEntry::item(
                MENU_ID_SETTINGS,
                lang.pick("设置中心...", "Settings..."),
                Some("Ctrl+,"),
                false,
            ),
        ];
        if !plugin_actions.is_empty() {
            entries.push(PopupEntry::Separator);
            for (idx, act) in plugin_actions.iter().enumerate() {
                entries.push(PopupEntry::item(
                    MENU_ID_PLUGIN_TRAY_BASE + idx as u32,
                    &act.label,
                    None,
                    false,
                ));
            }
        }
        entries.extend([
            PopupEntry::Separator,
            PopupEntry::item(
                MENU_ID_UPDATE,
                lang.pick("检查更新 (:update)", "Check for Updates"),
                None,
                false,
            ),
            PopupEntry::item(
                MENU_ID_STATS,
                lang.pick("内存与插件监控 (:stats)", "Memory & Plugin Stats"),
                None,
                false,
            ),
            PopupEntry::item(
                MENU_ID_CONFIG,
                lang.pick("打开 config.toml", "Open config.toml"),
                None,
                false,
            ),
            PopupEntry::item(
                MENU_ID_RELOAD,
                lang.pick("热重载配置与应用 (:reload)", "Hot-Reload All"),
                None,
                false,
            ),
            PopupEntry::item(
                MENU_ID_GC,
                lang.pick("立即休眠整理内存 (:gc)", "Hibernate & Trim RAM"),
                None,
                false,
            ),
            PopupEntry::Separator,
            PopupEntry::danger_item(MENU_ID_QUIT, lang.pick("退出 Tomoyo", "Exit Tomoyo"), None),
        ]);
        show_themed_popup_menu(
            PopupMenuStyle {
                pal,
                dpi_scale,
                font_regular,
                font_medium,
                align_bottom: true,
                owner: None,
            },
            &entries,
        )
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            if self.installed {
                let nid = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: self.hwnd,
                    uID: 1,
                    ..Default::default()
                };
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            if self.owns_icon && !self.hicon.is_invalid() {
                let _ = DestroyIcon(self.hicon);
            }
        }
    }
}

fn apply_nid_icon(
    hwnd: HWND,
    hicon: HICON,
    hotkey_label: &str,
    msg: windows::Win32::UI::Shell::NOTIFY_ICON_MESSAGE,
) -> bool {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY_CALLBACK,
        hIcon: hicon,
        ..Default::default()
    };

    let tip: Vec<u16> = format!("Tomoyo 效率启动器 ({hotkey_label})")
        .encode_utf16()
        .collect();
    for (dst, &src) in nid.szTip.iter_mut().zip(tip.iter()) {
        *dst = src;
    }

    unsafe { Shell_NotifyIconW(msg, &nid).as_bool() }
}

fn load_embedded_hicon() -> Option<HICON> {
    unsafe {
        let hinst = GetModuleHandleW(None).ok()?;
        let h = LoadImageW(
            Some(hinst.into()),
            PCWSTR(std::ptr::without_provenance::<u16>(1)),
            IMAGE_ICON,
            32,
            32,
            LR_DEFAULTSIZE,
        )
        .ok()?;
        Some(HICON(h.0))
    }
}

pub fn create_modern_white_hicon(size: i32) -> Option<HICON> {
    let bgra = generate_icon_bgra(size);
    unsafe {
        let color_bmp: HBITMAP = CreateBitmap(size, size, 1, 32, Some(bgra.as_ptr() as *const _));
        let mask_bytes = vec![0u8; ((size * size) / 8) as usize];
        let mask_bmp: HBITMAP =
            CreateBitmap(size, size, 1, 1, Some(mask_bytes.as_ptr() as *const _));

        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color_bmp.into());
        let _ = DeleteObject(mask_bmp.into());
        icon
    }
}
