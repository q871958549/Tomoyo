//! Multi-theme color palettes (`light` Modern White, `dark` Obsidian, `midnight`) & category accents.

use tomoyo_protocol::ResultCategory;
use windows::Win32::Foundation::COLORREF;

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

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
    pub card_hover: Rgb,
    pub scroll_thumb: Rgb,
    pub pill_bg: Rgb,
    pub pop_bg: Rgb,
    pub border: Rgb,
    pub divider: Rgb,
    pub text_primary: Rgb,
    pub text_muted: Rgb,
    pub on_accent: Rgb,
}

impl ThemePalette {
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "light" | "white" => Self {
                is_dark: false,
                bg: Rgb(0xF8, 0xF9, 0xFC),
                footer_bg: Rgb(0xF0, 0xF2, 0xF7),
                preview_bg: Rgb(0xFF, 0xFF, 0xFF),
                accent: Rgb(0x18, 0x1C, 0x28),
                card_sel: Rgb(0xEA, 0xEE, 0xF6),
                card_hover: Rgb(0xF1, 0xF4, 0xFA),
                scroll_thumb: Rgb(0xC2, 0xCA, 0xDB),
                pill_bg: Rgb(0xE6, 0xEA, 0xF2),
                pop_bg: Rgb(0xFF, 0xFF, 0xFF),
                border: Rgb(0xD5, 0xDB, 0xE7),
                divider: Rgb(0xE2, 0xE6, 0xF0),
                text_primary: Rgb(0x15, 0x18, 0x22),
                text_muted: Rgb(0x63, 0x6B, 0x7E),
                on_accent: Rgb(0xFC, 0xFC, 0xFE),
            },
            "midnight" => Self {
                is_dark: true,
                bg: Rgb(0x0E, 0x10, 0x1A),
                footer_bg: Rgb(0x0A, 0x0B, 0x13),
                preview_bg: Rgb(0x12, 0x15, 0x23),
                accent: Rgb(0x63, 0x66, 0xF1),
                card_sel: Rgb(0x1B, 0x20, 0x35),
                card_hover: Rgb(0x15, 0x19, 0x2B),
                scroll_thumb: Rgb(0x36, 0x42, 0x68),
                pill_bg: Rgb(0x1E, 0x24, 0x3C),
                pop_bg: Rgb(0x14, 0x18, 0x28),
                border: Rgb(0x28, 0x30, 0x4A),
                divider: Rgb(0x1E, 0x24, 0x38),
                text_primary: Rgb(0xF1, 0xF5, 0xF9),
                text_muted: Rgb(0x88, 0x94, 0xB0),
                on_accent: Rgb(0xFF, 0xFF, 0xFF),
            },
            _ => Self {
                is_dark: true,
                bg: Rgb(0x16, 0x16, 0x1E),
                footer_bg: Rgb(0x11, 0x11, 0x18),
                preview_bg: Rgb(0x1A, 0x1B, 0x25),
                accent: Rgb(0x7C, 0x6A, 0xF7),
                card_sel: Rgb(0x24, 0x25, 0x36),
                card_hover: Rgb(0x1D, 0x1E, 0x2B),
                scroll_thumb: Rgb(0x3C, 0x40, 0x5A),
                pill_bg: Rgb(0x28, 0x29, 0x3C),
                pop_bg: Rgb(0x1C, 0x1D, 0x2A),
                border: Rgb(0x2E, 0x2F, 0x40),
                divider: Rgb(0x26, 0x27, 0x36),
                text_primary: Rgb(0xF3, 0xF4, 0xF8),
                text_muted: Rgb(0x8E, 0x93, 0xA8),
                on_accent: Rgb(0xFF, 0xFF, 0xFF),
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
