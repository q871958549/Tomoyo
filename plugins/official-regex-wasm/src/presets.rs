//! Built-in curated regex presets (`re :<id> [text]`).

pub struct RegexPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub pattern: &'static str,
    pub flags: &'static str,
    pub sample: &'static str,
}

pub const PRESETS: &[RegexPreset] = &[
    RegexPreset {
        id: "email",
        label: "电子邮箱地址 + 用户名/域名捕获",
        pattern: r"([\w.+-]+)@([\w-]+\.[\w.-]+)",
        flags: "g",
        sample: "contact: alice@tomoyo.dev, ops@rust-lang.org",
    },
    RegexPreset {
        id: "url",
        label: "HTTP/HTTPS 网址提取",
        pattern: r"https?://[\w\-./?%&=:#+~@]+",
        flags: "gi",
        sample: "See https://github.com/q871958549/Tomoyo/releases for details",
    },
    RegexPreset {
        id: "ipv4",
        label: "IPv4 地址四段提取",
        pattern: r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b",
        flags: "g",
        sample: "listen=127.0.0.1 gateway=192.168.1.1",
    },
    RegexPreset {
        id: "semver",
        label: "SemVer 语义化版本号 (major.minor.patch)",
        pattern: r"v?(\d+)\.(\d+)\.(\d+)(?:-[\w.-]+)?",
        flags: "g",
        sample: "Tomoyo v3.4.0-alpha + SDK v3.2.0",
    },
    RegexPreset {
        id: "uuid",
        label: "UUID v4/标准 8-4-4-4-12 识别",
        pattern: r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
        flags: "gi",
        sample: "id=550e8400-e29b-41d4-a716-446655440000",
    },
    RegexPreset {
        id: "date",
        label: "ISO 日期 (YYYY-MM-DD HH:MM)",
        pattern: r"(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}:\d{2}(?::\d{2})?))?",
        flags: "g",
        sample: "2026-11-27 21:10:00 -> 2026-12-01",
    },
    RegexPreset {
        id: "hex",
        label: "CSS / 设计 Hex 色值",
        pattern: r"#([0-9a-fA-F]{6}|[0-9a-fA-F]{3})\b",
        flags: "g",
        sample: "accent: #6366f1, border: #334155, short: #fff",
    },
    RegexPreset {
        id: "cn-phone",
        label: "中国大陆手机号提取",
        pattern: r"\b1[3-9]\d{9}\b",
        flags: "g",
        sample: "tel: 13800138000 / invalid: 12345678901",
    },
];
