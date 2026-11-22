use tomoyo_core::{score_match, IndexedText, Provider};
use tomoyo_protocol::{ActionKind, ActionSpec, IconSpec, QueryContext, ResultCategory, ResultItem};

/// Live desktop window switcher (`w <query>` or global match).
pub struct WindowsSwitcherProvider;

impl WindowsSwitcherProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsSwitcherProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for WindowsSwitcherProvider {
    fn id(&self) -> &'static str {
        "builtin.windows"
    }

    fn name(&self) -> &'static str {
        "Window Switcher"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["w"]
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let dedicated = ctx.keyword.is_some();
        let q = if dedicated {
            ctx.args.trim()
        } else {
            ctx.trimmed.as_str()
        };

        if !dedicated && q.chars().count() < 2 {
            return Vec::new();
        }

        let windows = enumerate_desktop_windows();
        let mut results = Vec::new();
        for (idx, (hwnd, title)) in windows.into_iter().enumerate() {
            let score = if q.is_empty() {
                7_500 - idx as i64 * 15
            } else {
                let indexed = IndexedText::new(&title);
                let Some(s) = score_match(q, &indexed) else {
                    continue;
                };
                if dedicated {
                    s
                } else {
                    (s - 1_500).max(2_500)
                }
            };

            results.push(ResultItem {
                id: format!("win:{hwnd}"),
                provider_id: self.id().into(),
                title: title.clone(),
                subtitle: "Open Window · Enter to switch focus".into(),
                category: ResultCategory::Window,
                score,
                icon: IconSpec::Monogram("W".into()),
                badge: Some("WIN".into()),
                copy_text: Some(title),
                actions: vec![ActionSpec {
                    id: "focus".into(),
                    label: "Switch to Window".into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::FocusWindow { hwnd },
                }],
                preview: None,
            });
        }

        results.sort_by_key(|a| std::cmp::Reverse(a.score));
        results.truncate(if dedicated { ctx.limit.min(20) } else { 4 });
        results
    }
}

#[cfg(windows)]
fn enumerate_desktop_windows() -> Vec<(isize, String)> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
        GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
                return BOOL(1);
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 || len > 512 {
                return BOOL(1);
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let copied = GetWindowTextW(hwnd, &mut buf);
            if copied > 0 {
                let title = String::from_utf16_lossy(&buf[..copied as usize]);
                if title != "Program Manager" && title != "Tomoyo" {
                    let list = &mut *(lparam.0 as *mut Vec<(isize, String)>);
                    list.push((hwnd.0 as isize, title));
                }
            }
            BOOL(1)
        }
    }

    let mut list: Vec<(isize, String)> = Vec::with_capacity(32);
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

#[cfg(target_os = "linux")]
mod linux_win {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use std::sync::{OnceLock, RwLock};

    static WINDOW_IDS: OnceLock<RwLock<HashMap<isize, String>>> = OnceLock::new();

    fn registry() -> &'static RwLock<HashMap<isize, String>> {
        WINDOW_IDS.get_or_init(|| RwLock::new(HashMap::new()))
    }

    pub fn enumerate_desktop_windows() -> Vec<(isize, String)> {
        let Some(rows) = tomoyo_core::linux_sys::dbus_krunner_windows() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(rows.len());
        let mut map = HashMap::with_capacity(rows.len());
        for (match_id, title, app_id) in rows {
            if title == "Tomoyo" || title.trim().is_empty() {
                continue;
            }
            let mut h = std::collections::hash_map::DefaultHasher::new();
            match_id.hash(&mut h);
            let token = (h.finish() as isize).abs().max(1);
            let display = if app_id.is_empty() || title.to_lowercase().contains(&app_id.to_lowercase()) {
                title
            } else {
                format!("{title} ({app_id})")
            };
            map.insert(token, match_id);
            out.push((token, display));
        }
        if let Ok(mut guard) = registry().write() {
            *guard = map;
        }
        out
    }

    pub fn resolve_linux_window_id(token: isize) -> Option<String> {
        registry().read().ok()?.get(&token).cloned()
    }
}

#[cfg(target_os = "linux")]
pub use linux_win::resolve_linux_window_id;

#[cfg(target_os = "linux")]
fn enumerate_desktop_windows() -> Vec<(isize, String)> {
    linux_win::enumerate_desktop_windows()
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn enumerate_desktop_windows() -> Vec<(isize, String)> {
    Vec::new()
}
