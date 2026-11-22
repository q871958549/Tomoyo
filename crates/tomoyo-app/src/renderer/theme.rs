//! Multi-theme color palettes (`light`, `dark`, `midnight`) & category accents.

use tomoyo_protocol::ResultCategory;
#[cfg(windows)]
use windows::Win32::Foundation::COLORREF;

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[cfg(windows)]
impl Rgb {
    pub fn to_colorref(self) -> COLORREF {
        COLORREF((self.0 as u32) | ((self.1 as u32) << 8) | ((self.2 as u32) << 16))
    }
}

#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub is_dark: bool,
    pub bg: Rgb,
    pub footer_bg: Rgb,
    pub preview_bg: Rgb,
    pub accent: Rgb,
    pub card_sel: Rgb,
    pub sel_border: Rgb,
    pub card_hover: Rgb,
    pub scroll_thumb: Rgb,
    pub pill_bg: Rgb,
    pub chip_bg: Rgb,
    pub chip_border: Rgb,
    pub chip_text: Rgb,
    pub chip_hover_bg: Rgb,
    pub chip_hover_border: Rgb,
    pub chip_hover_text: Rgb,
    pub chip_active_bg: Rgb,
    pub chip_active_border: Rgb,
    pub chip_active_text: Rgb,
    pub pop_bg: Rgb,
    pub border: Rgb,
    pub divider: Rgb,
    pub text_primary: Rgb,
    pub text_secondary: Rgb,
    pub text_muted: Rgb,
}

impl ThemePalette {
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "light" | "white" => Self {
                is_dark: false,
                bg: Rgb(0xF9, 0xFA, 0xFC),
                footer_bg: Rgb(0xF2, 0xF4, 0xF8),
                preview_bg: Rgb(0xFF, 0xFF, 0xFF),
                accent: Rgb(0x3B, 0x5B, 0xDB),
                card_sel: Rgb(0xEE, 0xF2, 0xFF),
                sel_border: Rgb(0xD2, 0xDC, 0xF9),
                card_hover: Rgb(0xF2, 0xF4, 0xF9),
                scroll_thumb: Rgb(0xC6, 0xCE, 0xDE),
                pill_bg: Rgb(0xEE, 0xF1, 0xF7),
                chip_bg: Rgb(0xFF, 0xFF, 0xFF),
                chip_border: Rgb(0xDC, 0xE2, 0xEC),
                chip_text: Rgb(0x47, 0x55, 0x69),
                chip_hover_bg: Rgb(0xF3, 0xF6, 0xFB),
                chip_hover_border: Rgb(0xB8, 0xC4, 0xDA),
                chip_hover_text: Rgb(0x1E, 0x29, 0x3B),
                chip_active_bg: Rgb(0xEB, 0xF0, 0xFF),
                chip_active_border: Rgb(0x4F, 0x6A, 0xF5),
                chip_active_text: Rgb(0x1E, 0x3A, 0x8A),
                pop_bg: Rgb(0xFF, 0xFF, 0xFF),
                border: Rgb(0xD6, 0xDD, 0xE9),
                divider: Rgb(0xE6, 0xEA, 0xF2),
                text_primary: Rgb(0x11, 0x18, 0x27),
                text_secondary: Rgb(0x4B, 0x55, 0x63),
                text_muted: Rgb(0x68, 0x73, 0x85),
            },
            "midnight" => Self {
                is_dark: true,
                bg: Rgb(0x0E, 0x11, 0x1B),
                footer_bg: Rgb(0x0A, 0x0D, 0x15),
                preview_bg: Rgb(0x12, 0x16, 0x24),
                accent: Rgb(0x63, 0x66, 0xF1),
                card_sel: Rgb(0x19, 0x20, 0x38),
                sel_border: Rgb(0x32, 0x3F, 0x6E),
                card_hover: Rgb(0x14, 0x19, 0x2C),
                scroll_thumb: Rgb(0x36, 0x42, 0x68),
                pill_bg: Rgb(0x1B, 0x22, 0x38),
                chip_bg: Rgb(0x15, 0x1B, 0x2E),
                chip_border: Rgb(0x25, 0x30, 0x4D),
                chip_text: Rgb(0xAE, 0xBB, 0xD3),
                chip_hover_bg: Rgb(0x1B, 0x23, 0x3B),
                chip_hover_border: Rgb(0x39, 0x49, 0x73),
                chip_hover_text: Rgb(0xE6, 0xED, 0xF8),
                chip_active_bg: Rgb(0x1F, 0x28, 0x4D),
                chip_active_border: Rgb(0x63, 0x66, 0xF1),
                chip_active_text: Rgb(0xDC, 0xE4, 0xFF),
                pop_bg: Rgb(0x14, 0x18, 0x28),
                border: Rgb(0x26, 0x30, 0x4A),
                divider: Rgb(0x1C, 0x23, 0x38),
                text_primary: Rgb(0xF1, 0xF5, 0xF9),
                text_secondary: Rgb(0xA3, 0xB0, 0xC9),
                text_muted: Rgb(0x78, 0x86, 0xA3),
            },
            _ => Self {
                is_dark: true,
                bg: Rgb(0x16, 0x17, 0x1F),
                footer_bg: Rgb(0x12, 0x13, 0x1A),
                preview_bg: Rgb(0x1A, 0x1B, 0x25),
                accent: Rgb(0x7C, 0x83, 0xFD),
                card_sel: Rgb(0x22, 0x25, 0x38),
                sel_border: Rgb(0x37, 0x3E, 0x63),
                card_hover: Rgb(0x1D, 0x1F, 0x2D),
                scroll_thumb: Rgb(0x3C, 0x40, 0x5A),
                pill_bg: Rgb(0x24, 0x27, 0x36),
                chip_bg: Rgb(0x1E, 0x20, 0x2C),
                chip_border: Rgb(0x2E, 0x32, 0x45),
                chip_text: Rgb(0xB4, 0xBC, 0xD0),
                chip_hover_bg: Rgb(0x25, 0x28, 0x38),
                chip_hover_border: Rgb(0x43, 0x4A, 0x69),
                chip_hover_text: Rgb(0xE8, 0xEC, 0xF6),
                chip_active_bg: Rgb(0x27, 0x2B, 0x47),
                chip_active_border: Rgb(0x7C, 0x83, 0xFD),
                chip_active_text: Rgb(0xE0, 0xE5, 0xFF),
                pop_bg: Rgb(0x1C, 0x1D, 0x2A),
                border: Rgb(0x2E, 0x31, 0x42),
                divider: Rgb(0x23, 0x26, 0x35),
                text_primary: Rgb(0xF3, 0xF5, 0xFA),
                text_secondary: Rgb(0xA9, 0xB1, 0xC6),
                text_muted: Rgb(0x7C, 0x84, 0x9B),
            },
        }
    }
}

pub fn category_rgb(cat: ResultCategory) -> Rgb {
    match cat {
        ResultCategory::App => Rgb(0x3B, 0x82, 0xF6),
        ResultCategory::File => Rgb(0x10, 0xB9, 0x81),
        ResultCategory::Folder => Rgb(0x05, 0x96, 0x69),
        ResultCategory::Calc => Rgb(0xF5, 0x9E, 0x0B),
        ResultCategory::Clipboard => Rgb(0x0E, 0xA5, 0xE9),
        ResultCategory::Window => Rgb(0x06, 0xB6, 0xD4),
        ResultCategory::System => Rgb(0xEC, 0x48, 0x99),
        ResultCategory::Plugin => Rgb(0x8B, 0x5C, 0xF6),
        ResultCategory::Web => Rgb(0x63, 0x66, 0xF1),
    }
}
