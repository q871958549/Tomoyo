//! Post-OCR cleanup and multi-pass structural scoring for WinRT & Tesseract outputs.
//!
//! Repairs `zh-Hans-CN` OCR hallucinations on ASCII/technical strings (spurious `·`/`，`/`（）`
//! and CJK spacing) while preserving real Chinese prose typography unchanged.

/// Cleans spurious CJK spaces and repairs structured MAC / IPv4 / ASCII technical tokens
/// mis-punctuated by CJK-first OCR models.
pub fn clean_cjk_spaces(line: &str) -> String {
    let base = strip_cjk_spaces_base(line);
    let mac_fixed = normalize_mac_addresses(&base);
    let ip_fixed = normalize_ipv4_addresses(&mac_fixed);
    normalize_ascii_or_mixed_punct(&ip_fixed)
}

fn strip_cjk_spaces_base(line: &str) -> String {
    let chars: Vec<char> = line.trim().chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(line.len());
    for i in 0..chars.len() {
        let ch = chars[i];
        let prev = out.chars().next_back();
        let next = chars[i + 1..].iter().copied().find(|&c| c != ' ');
        if ch == ' ' {
            let drop_space = match (prev, next) {
                (Some(p), Some(n)) => {
                    (is_cjk_or_punct(p) && is_cjk_or_punct(n))
                        || (is_cjk_punct(p) && n.is_ascii_alphanumeric())
                        || (p.is_ascii_alphanumeric() && is_cjk_punct(n))
                        || (p.is_ascii_digit() && matches!(n, '.' | ':' | '/'))
                        || (matches!(p, '.' | ':' | '/') && n.is_ascii_digit())
                }
                _ => true,
            };
            if !drop_space && prev != Some(' ') {
                out.push(' ');
            }
        } else {
            let between_digits = prev.is_some_and(|p| p.is_ascii_digit())
                && next.is_some_and(|n| n.is_ascii_digit());
            let mapped = if between_digits {
                match ch {
                    '．' => '.',
                    '：' => ':',
                    '／' => '/',
                    _ => ch,
                }
            } else {
                ch
            };
            out.push(mapped);
        }
    }
    out
}

fn is_han(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
}

fn is_cjk_or_punct(c: char) -> bool {
    is_han(c) || matches!(c, '\u{3000}'..='\u{303F}' | '\u{FF01}'..='\u{FF5E}')
}

fn is_cjk_punct(c: char) -> bool {
    matches!(
        c,
        '，' | '。' | '；' | '：' | '！' | '？' | '（' | '）' | '【' | '】' | '《' | '》' | '、' | '“' | '”' | '‘' | '’' | '．' | '／'
    )
}

fn parse_mac_at(chars: &[char], start: usize) -> Option<(usize, String)> {
    if start > 0 && chars[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    let mut idx = start;
    let mut octets = Vec::with_capacity(6);
    for part in 0..6 {
        if idx + 1 >= chars.len()
            || !chars[idx].is_ascii_hexdigit()
            || !chars[idx + 1].is_ascii_hexdigit()
        {
            return None;
        }
        octets.push(format!("{}{}", chars[idx], chars[idx + 1]));
        idx += 2;
        if part < 5 {
            while idx < chars.len() && chars[idx] == ' ' {
                idx += 1;
            }
            if idx >= chars.len() || !matches!(chars[idx], '-' | '·' | '—' | '－' | ':') {
                return None;
            }
            idx += 1;
            while idx < chars.len() && chars[idx] == ' ' {
                idx += 1;
            }
        }
    }
    if idx < chars.len() && chars[idx].is_ascii_alphanumeric() {
        return None;
    }
    Some((idx, octets.join("-")))
}

fn normalize_mac_addresses(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some((end, mac)) = parse_mac_at(&chars, i) {
            out.push_str(&mac);
            i = end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn parse_ipv4_at(chars: &[char], start: usize) -> Option<(usize, String)> {
    if start > 0 && (chars[start - 1].is_ascii_digit() || chars[start - 1] == '.') {
        return None;
    }
    let mut idx = start;
    let mut octets = Vec::with_capacity(4);
    for part in 0..4 {
        let d_start = idx;
        while idx < chars.len() && chars[idx].is_ascii_digit() && idx - d_start < 3 {
            idx += 1;
        }
        let len = idx - d_start;
        if len == 0 {
            return None;
        }
        if idx < chars.len() && chars[idx].is_ascii_digit() {
            return None;
        }
        let num_str: String = chars[d_start..idx].iter().collect();
        if len > 1 && num_str.starts_with('0') {
            return None;
        }
        let val: u16 = num_str.parse().ok()?;
        if val > 255 {
            return None;
        }
        octets.push(num_str);
        if part < 3 {
            while idx < chars.len() && chars[idx] == ' ' {
                idx += 1;
            }
            if idx >= chars.len() || !matches!(chars[idx], '.' | '，' | '．') {
                return None;
            }
            idx += 1;
            while idx < chars.len() && chars[idx] == ' ' {
                idx += 1;
            }
        }
    }
    if idx < chars.len() && chars[idx] == '.' {
        return None;
    }
    Some((idx, octets.join(".")))
}

fn normalize_ipv4_addresses(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some((end, ip)) = parse_ipv4_at(&chars, i) {
            out.push_str(&ip);
            i = end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn normalize_ascii_or_mixed_punct(s: &str) -> String {
    let has_cjk = s.chars().any(is_han);
    let ascii_alnum = s.chars().filter(char::is_ascii_alphanumeric).count();
    if has_cjk || ascii_alnum < 2 {
        return normalize_mixed_ascii_parens(s);
    }
    let chars: Vec<char> = s.chars().collect();
    let mut mapped = String::with_capacity(s.len());
    for i in 0..chars.len() {
        let c = chars[i];
        let ch = match c {
            '（' => '(',
            '）' => ')',
            '：' => ':',
            '；' => ';',
            '！' => '!',
            '？' => '?',
            '．' => '.',
            '／' => '/',
            '－' | '—' => '-',
            '【' => '[',
            '】' => ']',
            '·' => {
                let left = mapped.chars().rev().find(|x| !x.is_whitespace());
                let right = chars[i + 1..].iter().copied().find(|x| !x.is_whitespace());
                if left.is_some_and(|l| l.is_ascii_alphanumeric())
                    && right.is_some_and(|r| r.is_ascii_alphanumeric())
                {
                    '-'
                } else {
                    c
                }
            }
            _ => c,
        };
        mapped.push(ch);
    }
    compact_bracket_spaces(&mapped)
}

fn compact_bracket_spaces(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for i in 0..chars.len() {
        let c = chars[i];
        if c == ' ' {
            let prev = out.chars().next_back();
            let next = chars[i + 1..].iter().copied().find(|&x| x != ' ');
            let drop = matches!(prev, Some('(' | '['))
                || matches!(next, Some(')' | ']' | ','))
                || (prev.is_some_and(|p| p.is_ascii_alphanumeric()) && next == Some('(')
                    && chars[i + 1..].iter().skip_while(|&&x| x == ' ').nth(1).is_some_and(char::is_ascii_digit));
            if !drop && prev != Some(' ') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn normalize_mixed_ascii_parens(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '（'
            && out.chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric())
            && let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '）')
        {
            let inner = &chars[i + 1..i + 1 + rel_end];
            if !inner.is_empty()
                && inner.iter().all(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '/' | '_' | ' ')
                })
            {
                out.push('(');
                for &ic in inner {
                    if ic != ' ' {
                        out.push(ic);
                    }
                }
                out.push(')');
                i += rel_end + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn count_structured_hits(cleaned: &str) -> (usize, usize) {
    let chars: Vec<char> = cleaned.chars().collect();
    let (mut macs, mut ips, mut i) = (0, 0, 0);
    while i < chars.len() {
        if let Some((end, _)) = parse_mac_at(&chars, i) {
            macs += 1;
            i = end;
        } else if let Some((end, _)) = parse_ipv4_at(&chars, i) {
            ips += 1;
            i = end;
        } else {
            i += 1;
        }
    }
    (macs, ips)
}

/// Scores an OCR pass so multi-pass selection prefers structurally sound, balanced output.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn score_ocr_lines(lines: &[String]) -> i64 {
    let mut score = 0i64;
    for line in lines {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        score += 120;
        let has_cjk = t.chars().any(is_han);
        let (mut parens, mut cn_parens, mut brackets) = (0i64, 0i64, 0i64);
        let (mut matched_pairs, mut dangling) = (0i64, 0i64);
        for ch in t.chars() {
            if ch.is_ascii_alphanumeric() {
                score += 10;
            } else if is_han(ch) {
                score += 12;
            } else if matches!(ch, '-' | '.' | ':' | '/' | '(' | ')' | '[' | ']' | '_' | '@' | '=' | '+') {
                score += 9;
                match ch {
                    '(' => parens += 1,
                    ')' => if parens > 0 { parens -= 1; matched_pairs += 1; } else { dangling += 1; },
                    '[' => brackets += 1,
                    ']' => if brackets > 0 { brackets -= 1; matched_pairs += 1; } else { dangling += 1; },
                    _ => {}
                }
            } else if is_cjk_punct(ch) || ch == '·' {
                match ch {
                    '（' => cn_parens += 1,
                    '）' => if cn_parens > 0 { cn_parens -= 1; matched_pairs += 1; } else { dangling += 1; },
                    _ => {}
                }
                if has_cjk {
                    score += 6;
                } else {
                    score += if ch == '，' { -2 } else { 1 };
                }
            } else if matches!(ch, '|' | '~' | '^' | '`' | '\\') {
                score -= 6;
            } else if !ch.is_whitespace() {
                score += 2;
            }
        }
        dangling += parens + cn_parens + brackets;
        score += matched_pairs * 16 - dangling * 14;
        let cleaned = clean_cjk_spaces(t);
        let (macs, ips) = count_structured_hits(&cleaned);
        score += (macs as i64) * 30 + (ips as i64) * 30;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_mac_ipv4_and_ascii_punct_without_harming_chinese() {
        assert_eq!(
            clean_cjk_spaces("00 · BE · 43 · D5 · 7C · CF （ 192 ， 168 ， 6 ， 181 ）"),
            "00-BE-43-D5-7C-CF(192.168.6.181)"
        );
        assert_eq!(
            clean_cjk_spaces("00 · BE · 43 · D5 · 7C-CF （ 192 ， 168 ， 6 ， 181 ）"),
            "00-BE-43-D5-7C-CF(192.168.6.181)"
        );
        assert_eq!(
            clean_cjk_spaces("00 · BE · 43 · D5 · 7C · CF(192 ． 168 ． 6 ． 181 ）"),
            "00-BE-43-D5-7C-CF(192.168.6.181)"
        );
        assert_eq!(
            clean_cjk_spaces("网卡 00 · BE · 43 · D5 · 7C · CF（192，168，6，181）"),
            "网卡 00-BE-43-D5-7C-CF(192.168.6.181)"
        );
        assert_eq!(clean_cjk_spaces("今天，天气很好（测试）"), "今天，天气很好（测试）");
        assert_eq!(clean_cjk_spaces("张三·李四：选项 1，2，3"), "张三·李四：选项 1，2，3");
        assert_eq!(clean_cjk_spaces("离 线 识 图 取 字"), "离线识图取字");
        assert_eq!(clean_cjk_spaces("Security Posture Awareness"), "Security Posture Awareness");
        assert_eq!(clean_cjk_spaces("华为 防 火 墙 192.168.1.7"), "华为防火墙 192.168.1.7");
        assert_eq!(clean_cjk_spaces("192 ． 168 ． 1 ． 7：60186"), "192.168.1.7:60186");
        assert_eq!(clean_cjk_spaces("2026 ／ 11 ／ 20 14：49：51"), "2026/11/20 14:49:51");
        assert_eq!(clean_cjk_spaces("内存 34 ． 2 MB"), "内存 34.2 MB");
    }

    #[test]
    fn scorer_prefers_clean_mac_ip_over_fused_garble() {
        let good = vec!["00 · BE · 43 · D5 · 7C-CF （ 192 ， 168 ， 6 ， 181 ）".to_string()];
        let fused = vec!["00 · BE · 43 · D5 · 70CF092，168，6，181）".to_string()];
        assert!(score_ocr_lines(&good) > score_ocr_lines(&fused));
    }
}
