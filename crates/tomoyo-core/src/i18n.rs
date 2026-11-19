//! Lightweight zero-allocation i18n helper (Chinese primary, English supported).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" | "en-us" | "en_us" | "english" => Self::En,
            _ => Self::Zh,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }

    #[inline]
    pub fn pick<'a>(self, zh: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Zh => zh,
            Self::En => en,
        }
    }
}
