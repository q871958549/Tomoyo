use pinyin::ToPinyin;

/// Precomputed searchable representation supporting English, Chinese Pinyin full
/// spelling, and Chinese/English acronym initials (e.g. "微信" -> "weixin" / "wx",
/// "Visual Studio Code" -> "vsc").
#[derive(Debug, Clone)]
pub struct IndexedText {
    pub lower: String,
    pub pinyin_full: String,
    pub pinyin_initials: String,
}

impl IndexedText {
    pub fn new(text: &str) -> Self {
        let lower = text.to_lowercase();
        let mut pinyin_full = String::with_capacity(text.len());
        let mut pinyin_initials = String::with_capacity(16);

        let mut prev_sep = true;
        for ch in text.chars() {
            if let Some(py) = ch.to_pinyin() {
                let plain = py.plain();
                pinyin_full.push_str(plain);
                if let Some(first) = plain.chars().next() {
                    pinyin_initials.push(first);
                }
                prev_sep = false;
            } else if ch.is_alphanumeric() {
                let low = ch.to_ascii_lowercase();
                pinyin_full.push(low);
                if prev_sep || ch.is_uppercase() {
                    pinyin_initials.push(low);
                }
                prev_sep = false;
            } else {
                pinyin_full.push(' ');
                prev_sep = true;
            }
        }

        Self {
            lower,
            pinyin_full,
            pinyin_initials,
        }
    }
}

/// Scores `query` against `primary` text, falling back to an optional `alias`
/// (e.g., resolved `.lnk` target executable name) with a small tie-breaker penalty.
pub fn score_match_with_alias(
    query: &str,
    primary: &IndexedText,
    alias: Option<&IndexedText>,
) -> Option<i64> {
    if let Some(s) = score_match(query, primary) {
        return Some(s);
    }
    alias
        .and_then(|a| score_match(query, a))
        .map(|s| (s - 250).max(1_000))
}

/// Scores how well `query` matches `indexed`. Returns `None` if there is no match.
pub fn score_match(query: &str, indexed: &IndexedText) -> Option<i64> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(100);
    }

    // 1. Exact title match
    if indexed.lower == q {
        return Some(10_000);
    }

    // 2. Prefix match on original text
    if indexed.lower.starts_with(&q) {
        let bonus = (1000i64 - (indexed.lower.len() as i64 - q.len() as i64).max(0) * 15).max(0);
        return Some(8_500 + bonus);
    }

    // 3. Exact acronym / pinyin initials match (e.g. `wx` -> 微信, `vsc` -> Visual Studio Code)
    if indexed.pinyin_initials == q {
        return Some(8_200);
    }

    // 4. Exact full pinyin match (`weixin` -> 微信)
    let compact_py: String = indexed
        .pinyin_full
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if compact_py == q {
        return Some(7_800);
    }

    // 5. Prefix match on pinyin initials
    if indexed.pinyin_initials.starts_with(&q) {
        return Some(7_300);
    }

    // 6. Prefix match on compact pinyin
    if compact_py.starts_with(&q) {
        return Some(6_900);
    }

    // 7. Word-boundary vs mid-word substring match on original text
    if let Some(pos) = indexed.lower.find(&q) {
        let is_word_start = pos == 0
            || indexed.lower[..pos]
                .chars()
                .next_back()
                .map(|c| !c.is_alphanumeric())
                .unwrap_or(true);
        let base = if is_word_start { 6_200 } else { 4_400 };
        return Some((base - (pos as i64 * 12)).max(3_200));
    }

    // 8. Substring match in pinyin full
    if let Some(pos) = compact_py.find(&q) {
        return Some((4_300 - pos as i64 * 10).max(3_100));
    }

    // 9. Subsequence fuzzy match on original text (kept below high-confidence file hits)
    if let Some(s) = subsequence_score(&q, &indexed.lower) {
        return Some(2_400 + s);
    }

    // 10. Subsequence fuzzy match on pinyin initials or compact pinyin
    if let Some(s) = subsequence_score(&q, &indexed.pinyin_initials) {
        return Some(2_200 + s);
    }
    if let Some(s) = subsequence_score(&q, &compact_py) {
        return Some(2_000 + s);
    }

    None
}

fn subsequence_score(needle: &str, haystack: &str) -> Option<i64> {
    let mut h_chars = haystack.char_indices().peekable();
    let mut gaps = 0i64;
    let mut last_idx: Option<usize> = None;
    let mut consecutive = 0i64;

    for nc in needle.chars() {
        let mut found = None;
        for (idx, hc) in h_chars.by_ref() {
            if hc == nc {
                found = Some(idx);
                break;
            }
        }
        let idx = found?;
        if let Some(prev) = last_idx {
            let delta = idx.saturating_sub(prev + 1);
            if delta == 0 {
                consecutive += 1;
            } else {
                gaps += delta as i64;
            }
        }
        last_idx = Some(idx);
    }

    Some((500 + consecutive * 45 - gaps * 12).clamp(0, 900))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_chinese_pinyin_and_initials() {
        let wx = IndexedText::new("微信");
        assert_eq!(wx.pinyin_full, "weixin");
        assert_eq!(wx.pinyin_initials, "wx");
        assert!(score_match("wx", &wx).unwrap() >= 8_000);
        assert!(score_match("weixin", &wx).unwrap() >= 7_500);
        assert!(score_match("wei", &wx).unwrap() >= 6_500);

        let vsc = IndexedText::new("Visual Studio Code");
        assert_eq!(vsc.pinyin_initials, "vsc");
        assert!(score_match("vsc", &vsc).unwrap() >= 8_000);
        assert!(score_match("code", &vsc).unwrap() >= 5_500);
        assert!(score_match("vscd", &vsc).is_some());

        let cn = IndexedText::new("微信");
        let alias = IndexedText::new("WeChat");
        assert!(score_match_with_alias("wechat", &cn, Some(&alias)).unwrap() >= 8_000);
    }
}
