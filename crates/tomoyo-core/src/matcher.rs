use pinyin::{ToPinyin, ToPinyinMulti};

/// Precomputed searchable representation supporting English, Chinese Pinyin full
/// spelling, polyphone readings (多音字), and Chinese/English acronym initials
/// (e.g. "微信" -> "weixin" / "wx", "重庆" -> "chongqing" / "cq" & "zq",
/// "Visual Studio Code" -> "vsc").
#[derive(Debug, Clone)]
pub struct IndexedText {
    pub lower: String,
    pub pinyin_full: String,
    pub pinyin_initials: String,
    /// Bounded alternate `(compact_full, initials)` readings when polyphone chars exist.
    pub alt_variants: Vec<(String, String)>,
}

impl IndexedText {
    pub fn new(text: &str) -> Self {
        let lower = text.to_lowercase();
        let mut pinyin_full = String::with_capacity(text.len());
        let mut pinyin_initials = String::with_capacity(16);

        // Track up to 4 compact `(full, initials)` combinations for polyphone chars.
        let mut combos: Vec<(String, String)> = vec![(String::new(), String::new())];
        let mut has_polyphone = false;
        let mut prev_sep = true;

        for ch in text.chars() {
            if let Some(multi) = ch.to_pinyin_multi() {
                let mut readings: Vec<&'static str> = Vec::with_capacity(2);
                for py in multi {
                    let plain = py.plain();
                    if !readings.contains(&plain) {
                        readings.push(plain);
                        if readings.len() == 2 {
                            break;
                        }
                    }
                }
                if readings.is_empty() {
                    if let Some(py) = ch.to_pinyin() {
                        readings.push(py.plain());
                    }
                }
                if let Some(&primary) = readings.first() {
                    pinyin_full.push_str(primary);
                    if let Some(first) = primary.chars().next() {
                        pinyin_initials.push(first);
                    }
                }
                if readings.len() > 1 {
                    has_polyphone = true;
                    let mut next_combos = Vec::with_capacity((combos.len() * 2).min(4));
                    for (cf, ci) in &combos {
                        for &r in &readings {
                            if next_combos.len() >= 4 {
                                break;
                            }
                            let mut nf = cf.clone();
                            nf.push_str(r);
                            let mut ni = ci.clone();
                            if let Some(f) = r.chars().next() {
                                ni.push(f);
                            }
                            next_combos.push((nf, ni));
                        }
                    }
                    combos = next_combos;
                } else if let Some(&primary) = readings.first() {
                    let first_ch = primary.chars().next();
                    for (cf, ci) in &mut combos {
                        cf.push_str(primary);
                        if let Some(f) = first_ch {
                            ci.push(f);
                        }
                    }
                }
                prev_sep = false;
            } else if ch.is_alphanumeric() {
                let low = ch.to_ascii_lowercase();
                let is_init = prev_sep || ch.is_uppercase();
                pinyin_full.push(low);
                if is_init {
                    pinyin_initials.push(low);
                }
                for (cf, ci) in &mut combos {
                    cf.push(low);
                    if is_init {
                        ci.push(low);
                    }
                }
                prev_sep = false;
            } else {
                pinyin_full.push(' ');
                prev_sep = true;
            }
        }

        let alt_variants = if has_polyphone && combos.len() > 1 {
            combos.into_iter().skip(1).collect()
        } else {
            Vec::new()
        };

        Self {
            lower,
            pinyin_full,
            pinyin_initials,
            alt_variants,
        }
    }
}

/// Scores `query` against `primary` text, falling back to an optional `alias`
/// (e.g., resolved `.lnk` target executable name), and supporting space-separated
/// multi-token queries spanning primary + alias.
pub fn score_match_with_alias(
    query: &str,
    primary: &IndexedText,
    alias: Option<&IndexedText>,
) -> Option<i64> {
    let mut best = score_single_or_multi(query, primary);
    if let Some(a) = alias {
        if let Some(s) = score_single_or_multi(query, a) {
            let adj = (s - 250).max(1_000);
            best = Some(best.map_or(adj, |b| b.max(adj)));
        }
        // Cross-field multi-token intersection (e.g. "微信 wechat" or "desk 1118")
        let tokens: Vec<&str> = query.split_whitespace().take(4).collect();
        if tokens.len() >= 2 {
            let mut sum = 0i64;
            let mut all_ok = true;
            for tok in &tokens {
                let p_score = score_single_token(tok, primary);
                let a_score = score_single_token(tok, a).map(|s| (s - 250).max(1_000));
                match (p_score, a_score) {
                    (Some(x), Some(y)) => sum += x.max(y),
                    (Some(x), None) => sum += x,
                    (None, Some(y)) => sum += y,
                    (None, None) => {
                        all_ok = false;
                        break;
                    }
                }
            }
            if all_ok {
                let multi = (sum / tokens.len() as i64) + 180;
                best = Some(best.map_or(multi, |b| b.max(multi)));
            }
        }
    }
    best
}

/// Scores how well `query` matches `indexed`. Returns `None` if there is no match.
pub fn score_match(query: &str, indexed: &IndexedText) -> Option<i64> {
    score_single_or_multi(query, indexed)
}

fn score_single_or_multi(query: &str, indexed: &IndexedText) -> Option<i64> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(100);
    }
    let single = score_single_token(&q, indexed);
    let tokens: Vec<&str> = q.split_whitespace().take(4).collect();
    if tokens.len() >= 2 {
        let mut sum = 0i64;
        let mut all_ok = true;
        for tok in &tokens {
            if let Some(s) = score_single_token(tok, indexed) {
                sum += s;
            } else {
                all_ok = false;
                break;
            }
        }
        if all_ok {
            let multi = (sum / tokens.len() as i64) + 180;
            return Some(single.map_or(multi, |s| s.max(multi)));
        }
    }
    single
}

fn score_single_token(q: &str, indexed: &IndexedText) -> Option<i64> {
    if q.is_empty() {
        return Some(100);
    }

    // 1. Exact title match
    if indexed.lower == q {
        return Some(10_000);
    }

    // 2. Prefix match on original text
    if indexed.lower.starts_with(q) {
        let bonus = (1000i64 - (indexed.lower.len() as i64 - q.len() as i64).max(0) * 15).max(0);
        return Some(8_500 + bonus);
    }

    let compact_py: String = indexed
        .pinyin_full
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    // 3. Exact acronym / pinyin initials match (primary or polyphone variant)
    if indexed.pinyin_initials == q
        || indexed.alt_variants.iter().any(|(_, init)| init == q)
    {
        return Some(8_200);
    }

    // 4. Exact full pinyin match (primary or polyphone variant)
    if compact_py == q || indexed.alt_variants.iter().any(|(full, _)| full == q) {
        return Some(7_800);
    }

    // 5. Prefix match on pinyin initials (primary or polyphone variant)
    if indexed.pinyin_initials.starts_with(q)
        || indexed
            .alt_variants
            .iter()
            .any(|(_, init)| init.starts_with(q))
    {
        return Some(7_300);
    }

    // 6. Prefix match on compact pinyin (primary or polyphone variant)
    if compact_py.starts_with(q)
        || indexed
            .alt_variants
            .iter()
            .any(|(full, _)| full.starts_with(q))
    {
        return Some(6_900);
    }

    // 7. Word-boundary vs mid-word substring match on original text
    if let Some(pos) = indexed.lower.find(q) {
        let is_word_start = pos == 0
            || indexed.lower[..pos]
                .chars()
                .next_back()
                .map(|c| !c.is_alphanumeric())
                .unwrap_or(true);
        let base = if is_word_start { 6_200 } else { 4_400 };
        return Some((base - (pos as i64 * 12)).max(3_200));
    }

    // 8. Substring match in pinyin full (primary or polyphone variant)
    if let Some(pos) = compact_py.find(q) {
        return Some((4_300 - pos as i64 * 10).max(3_100));
    }
    for (full, _) in &indexed.alt_variants {
        if let Some(pos) = full.find(q) {
            return Some((4_250 - pos as i64 * 10).max(3_100));
        }
    }

    // 9. Subsequence fuzzy match on original text (kept below high-confidence file hits)
    if let Some(s) = subsequence_score(q, &indexed.lower) {
        return Some(2_400 + s);
    }

    // 10. Subsequence fuzzy match on pinyin initials or compact pinyin
    if let Some(s) = subsequence_score(q, &indexed.pinyin_initials) {
        return Some(2_200 + s);
    }
    if let Some(s) = subsequence_score(q, &compact_py) {
        return Some(2_000 + s);
    }
    for (full, init) in &indexed.alt_variants {
        if let Some(s) = subsequence_score(q, init) {
            return Some(2_150 + s);
        }
        if let Some(s) = subsequence_score(q, full) {
            return Some(1_950 + s);
        }
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
    fn matches_chinese_pinyin_polyphones_and_multi_tokens() {
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
        assert!(score_match("visual code", &vsc).unwrap() >= 6_000);

        // Polyphone tests: 重庆 (chongqing/cq), 银行 (yinhang/yh), 调试 (tiaoshi/ts)
        let cq = IndexedText::new("重庆");
        assert!(score_match("cq", &cq).unwrap() >= 8_000);
        assert!(score_match("chongqing", &cq).unwrap() >= 7_500);

        let yh = IndexedText::new("招商银行");
        assert!(score_match("zsyh", &yh).unwrap() >= 8_000);
        assert!(score_match("yinhang", &yh).is_some());

        let ts = IndexedText::new("网络调试助手");
        assert!(score_match("wlts", &ts).unwrap() >= 7_000);

        // Cross-field alias + multi-token
        let cn = IndexedText::new("微信");
        let alias = IndexedText::new("WeChat");
        assert!(score_match_with_alias("wechat", &cn, Some(&alias)).unwrap() >= 8_000);
        assert!(score_match_with_alias("wx wechat", &cn, Some(&alias)).unwrap() >= 7_500);
    }
}
