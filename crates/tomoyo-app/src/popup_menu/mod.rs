//! Theme-matched SDF + ClearType GDI popup context menu replacing legacy Win32 `CreatePopupMenu`.
//!
//! Renders with Tomoyo's active `ThemePalette`, Alibaba PuHuiTi 3.0 fonts, Win11 rounded DWM corners,
//! cascading Level-1/Level-2 submenus, persistent try-before-dismiss mode, and deterministic cleanup.

mod cascade;
mod paint;
mod panel;

use crate::renderer::ThemePalette;
use windows::Win32::Foundation::HWND;

#[derive(Debug, Clone)]
pub enum PopupEntry {
    Item {
        id: u32,
        label: String,
        shortcut: Option<String>,
        checked: bool,
        danger: bool,
        disabled: bool,
    },
    Submenu {
        label: String,
        children: Vec<PopupEntry>,
    },
    Separator,
}

impl PopupEntry {
    pub fn item(id: u32, label: impl Into<String>, shortcut: Option<&str>, checked: bool) -> Self {
        Self::Item {
            id,
            label: label.into(),
            shortcut: shortcut.map(ToString::to_string),
            checked,
            danger: false,
            disabled: false,
        }
    }

    pub fn item_disabled(
        id: u32,
        label: impl Into<String>,
        shortcut: Option<&str>,
        disabled: bool,
    ) -> Self {
        Self::Item {
            id,
            label: label.into(),
            shortcut: shortcut.map(ToString::to_string),
            checked: false,
            danger: false,
            disabled,
        }
    }

    pub fn danger_item(id: u32, label: impl Into<String>, shortcut: Option<&str>) -> Self {
        Self::Item {
            id,
            label: label.into(),
            shortcut: shortcut.map(ToString::to_string),
            checked: false,
            danger: true,
            disabled: false,
        }
    }

    pub fn submenu(label: impl Into<String>, children: Vec<PopupEntry>) -> Self {
        Self::Submenu {
            label: label.into(),
            children,
        }
    }
}

pub enum MenuStepOutcome {
    StayOpen {
        entries: Vec<PopupEntry>,
        pal: ThemePalette,
    },
    Close,
}

pub(super) struct RowMetric {
    pub y0: f32,
    pub y1: f32,
    pub is_sep: bool,
    pub disabled: bool,
}

pub struct PopupMenuStyle<'a> {
    pub pal: ThemePalette,
    pub dpi_scale: f32,
    pub font_regular: &'a str,
    pub font_medium: &'a str,
    pub align_bottom: bool,
    pub owner: Option<HWND>,
}

pub fn show_themed_popup_menu(
    style: PopupMenuStyle<'_>,
    entries: &[PopupEntry],
) -> Option<u32> {
    let mut picked = None;
    let mut handler = |id: u32| {
        picked = Some(id);
        MenuStepOutcome::Close
    };
    cascade::run_session(style, entries.to_vec(), &mut handler);
    picked
}

/// Runs a cascading Level-1/Level-2 context menu that stays open across tryable commands
/// until the user clicks outside, presses `Esc`, or selects an explicit close/navigate action.
/// Returns `true` when dismissed because focus explicitly moved to another process.
pub fn run_interactive_popup_menu(
    style: PopupMenuStyle<'_>,
    entries: Vec<PopupEntry>,
    mut on_command: impl FnMut(u32) -> MenuStepOutcome,
) -> bool {
    cascade::run_session(style, entries, &mut on_command)
}
