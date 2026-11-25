//! Pure-Rust timestamp (Howard Hinnant civil date) & SplitMix64 UUID/Token generator for WASM.

use tomoyo_wasm_sdk::{
    ActionKind, ActionSpec, IconSpec, PreviewSpec, ResultCategory, ResultItem,
};

const PID: &str = "dev.tomoyo.devtools";

pub fn build_timestamp_items(input: &str, now_ms: i64) -> Vec<ResultItem> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("now") {
        let secs = now_ms / 1000;
        let ms = now_ms;
        let iso = format_utc_iso(secs, (ms.rem_euclid(1000)) as u32);
        return vec![
            copy_item("dev:ts:now_s", &format!("当前 Unix 时间戳 (秒)：{secs}"), &format!("UTC: {iso} · Enter 复制秒级时间戳"), &secs.to_string(), 9_900, "TS"),
            copy_item("dev:ts:now_ms", &format!("当前 Unix 时间戳 (毫秒)：{ms}"), &format!("UTC: {iso} · Enter 复制毫秒级时间戳"), &ms.to_string(), 9_850, "MS"),
            copy_item("dev:ts:now_iso", &format!("当前 ISO-8601 (UTC)：{iso}"), "标准 RFC 3339 时间字符串 · Enter 复制", &iso, 9_800, "ISO"),
        ];
    }

    if let Ok(num) = trimmed.parse::<i64>() {
        let (secs, sub_ms, kind) = if num.abs() >= 100_000_000_000 {
            (num.div_euclid(1000), num.rem_euclid(1000) as u32, "毫秒 (ms)")
        } else {
            (num, 0, "秒 (s)")
        };
        let iso = format_utc_iso(secs, sub_ms);
        let bj = format_offset_iso(secs, 8 * 3600, "+08:00");
        let rel = format_relative(secs, now_ms / 1000);
        return vec![ResultItem {
            id: format!("dev:ts:num:{num}"),
            provider_id: PID.into(),
            title: format!("{bj}  (UTC+8)"),
            subtitle: format!("UTC: {iso} · {kind} · {rel} · Enter 复制"),
            category: ResultCategory::Calc,
            score: 9_920,
            icon: IconSpec::Monogram("TS".into()),
            badge: Some("UTC+8".into()),
            copy_text: Some(bj.clone()),
            actions: vec![
                ActionSpec { id: "copy_bj".into(), label: "复制 UTC+8 时间".into(), shortcut: Some("Enter".into()), kind: ActionKind::CopyText { text: bj.clone(), paste_immediately: false } },
                ActionSpec { id: "copy_iso".into(), label: "复制 UTC ISO-8601".into(), shortcut: Some("Ctrl+Enter".into()), kind: ActionKind::CopyText { text: iso.clone(), paste_immediately: false } },
            ],
            preview: Some(PreviewSpec::RichCard {
                title: "Unix 时间戳解析".into(),
                subtitle: format!("输入值: {num} ({kind})"),
                badges: vec!["WASM".into(), "RFC 3339".into()],
                rows: vec![
                    ("北京时间 (UTC+8)".into(), bj),
                    ("UTC ISO-8601".into(), iso),
                    ("Unix 秒 (s)".into(), secs.to_string()),
                    ("Unix 毫秒 (ms)".into(), (secs * 1000 + sub_ms as i64).to_string()),
                    ("相对当前".into(), rel),
                ],
                body: String::new(),
            }),
        }];
    }

    if let Some(secs) = parse_iso_utc(trimmed) {
        let ms = secs * 1000;
        return vec![
            copy_item("dev:ts:parsed_s", &format!("Unix 秒：{secs}"), &format!("解析自 `{trimmed}` (UTC) · Enter 复制"), &secs.to_string(), 9_900, "SEC"),
            copy_item("dev:ts:parsed_ms", &format!("Unix 毫秒：{ms}"), &format!("解析自 `{trimmed}` (UTC) · Enter 复制"), &ms.to_string(), 9_860, "MS"),
        ];
    }

    Vec::new()
}

pub fn build_uuid_items(now_ms: i64, counter: u64) -> Vec<ResultItem> {
    let seed_a = (now_ms as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ counter.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let mut rng = SplitMix64::new(seed_a ^ 0xD1B5_4A32_D192_ED03);
    let u1 = rng.next_uuid_v4();
    let u2 = rng.next_uuid_v4();
    let hex32 = rng.next_hex(32);
    let hex64 = rng.next_hex(64);
    vec![
        copy_item("dev:uuid:1", &format!("UUID v4：{u1}"), "RFC 4122 标准随机 UUID · Enter 复制 · Ctrl+Enter 直接粘贴", &u1, 9_920, "UUID"),
        copy_item("dev:uuid:compact", &format!("紧凑 UUID：{}", u2.replace('-', "")), "32 位无连字符 UUID · Enter 复制", &u2.replace('-', ""), 9_860, "HEX"),
        copy_item("dev:tok:32", &format!("随机 128-bit Hex：{hex32}"), "适合临时 Token / Salt · Enter 复制", &hex32, 9_820, "128b"),
        copy_item("dev:tok:64", &format!("随机 256-bit Secret：{}…", &hex64[..24]), "64 字符十六进制密钥 · Enter 复制完整密钥", &hex64, 9_780, "256b"),
    ]
}

pub fn format_utc_iso(unix_secs: i64, ms: u32) -> String {
    let (y, m, d, hh, mm, ss) = split_unix_secs(unix_secs);
    if ms > 0 {
        format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{ms:03}Z")
    } else {
        format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
    }
}

pub fn format_offset_iso(unix_secs: i64, offset_secs: i64, suffix: &str) -> String {
    let (y, m, d, hh, mm, ss) = split_unix_secs(unix_secs.saturating_add(offset_secs));
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02} {suffix}")
}

fn split_unix_secs(unix_secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = unix_secs.div_euclid(86_400);
    let sod = unix_secs.rem_euclid(86_400) as u32;
    let (y, m, d) = civil_from_days(days);
    (y, m, d, sod / 3600, (sod % 3600) / 60, sod % 60)
}

/// Howard Hinnant civil-from-days algorithm (exact proleptic Gregorian calendar, zero deps).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (y + i64::from(m <= 2), m as u32, d as u32)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m_adj = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * m_adj + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

pub fn parse_iso_utc(s: &str) -> Option<i64> {
    let clean = s.trim().trim_end_matches('Z').replace('T', " ");
    let mut parts = clean.split_whitespace();
    let date = parts.next()?;
    let mut dp = date.split('-');
    let y: i64 = dp.next()?.parse().ok()?;
    let m: u32 = dp.next()?.parse().ok()?;
    let d: u32 = dp.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let (hh, mm, ss) = if let Some(t) = parts.next() {
        let mut tp = t.split(':');
        let h: u32 = tp.next()?.parse().ok()?;
        let mi: u32 = tp.next().unwrap_or("0").parse().ok()?;
        let sc: u32 = tp.next().unwrap_or("0").split('.').next()?.parse().ok()?;
        if h > 23 || mi > 59 || sc > 59 {
            return None;
        }
        (h, mi, sc)
    } else {
        (0, 0, 0)
    };
    Some(days_from_civil(y, m, d) * 86_400 + (hh as i64) * 3600 + (mm as i64) * 60 + ss as i64)
}

pub fn format_relative(target_secs: i64, now_secs: i64) -> String {
    if now_secs <= 0 {
        return "UTC".into();
    }
    let diff = target_secs - now_secs;
    let abs = diff.abs();
    let (val, unit) = if abs < 60 {
        (abs, "秒")
    } else if abs < 3600 {
        (abs / 60, "分钟")
    } else if abs < 86_400 {
        (abs / 3600, "小时")
    } else {
        (abs / 86_400, "天")
    };
    if diff >= 0 { format!("{val} {unit}后") } else { format!("{val} {unit}前") }
}

pub fn copy_item(id: &str, title: &str, subtitle: &str, copy: &str, score: i64, badge: &str) -> ResultItem {
    ResultItem {
        id: id.into(),
        provider_id: PID.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        category: ResultCategory::Plugin,
        score,
        icon: IconSpec::Monogram(badge.into()),
        badge: Some(badge.into()),
        copy_text: Some(copy.into()),
        actions: vec![
            ActionSpec {
                id: "copy".into(),
                label: "复制到剪贴板".into(),
                shortcut: Some("Enter".into()),
                kind: ActionKind::CopyText { text: copy.into(), paste_immediately: false },
            },
            ActionSpec {
                id: "paste".into(),
                label: "直接粘贴到原窗口".into(),
                shortcut: Some("Ctrl+Enter".into()),
                kind: ActionKind::CopyText { text: copy.into(), paste_immediately: true },
            },
        ],
        preview: Some(PreviewSpec::Text { title: title.into(), body: copy.into() }),
    }
}

struct SplitMix64(u64);
impl SplitMix64 {
    fn new(seed: u64) -> Self { Self(seed) }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn next_uuid_v4(&mut self) -> String {
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&self.next_u64().to_le_bytes());
        b[8..].copy_from_slice(&self.next_u64().to_le_bytes());
        b[6] = (b[6] & 0x0F) | 0x40;
        b[8] = (b[8] & 0x3F) | 0x80;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }
    fn next_hex(&mut self, chars: usize) -> String {
        let mut out = String::with_capacity(chars);
        while out.len() < chars {
            out.push_str(&format!("{:016x}", self.next_u64()));
        }
        out.truncate(chars);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_unix_timestamps_and_iso_roundtrip() {
        assert_eq!(format_utc_iso(1_700_000_000, 0), "2023-11-14T22:13:20Z");
        assert_eq!(parse_iso_utc("2023-11-14T22:13:20Z"), Some(1_700_000_000));
        assert_eq!(parse_iso_utc("1970-01-01 00:00:00"), Some(0));
        let uuids = build_uuid_items(1_700_000_000_123, 7);
        assert_eq!(uuids.len(), 4);
        assert_eq!(uuids[0].copy_text.as_deref().unwrap().len(), 36);
    }
}
