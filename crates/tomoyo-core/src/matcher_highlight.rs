//! Computes UTF-8-safe byte highlight ranges in result titles for exact, Pinyin,
//! polyphone, acronym, and subsequence matches so users immediately see why a row matched.

use pinyin::ToPinyinMulti;

/// Returns merged, sorted byte ranges `(start, end)` in `title` that match `raw_query`.
/// Every returned offset is guaranteed to lie on a valid UTF-8 char boundary in `title`.
pub fn highlight_title_ranges(raw_query: &str, title: &str) -> Vec<(usize, usize)> {
    if title.is_empty() {
        return Vec::new();
    }
    let clean = strip_query_prefix(raw_query);
    if clean.is_empty() {
        return Vec::new();
    }

    let chars: Vec<(usize, usize, char)> = title
        .char_indices()
        .map(|(s, c)| (s, s + c.len_utf8(), c))
        .collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut raw_spans: Vec<(usize, usize)> = Vec::new();
    for tok in clean.split_whitespace().take(4) {
        let tok_low: Vec<char> = tok.to_lowercase().chars().collect();
        if tok_low.is_empty() {
            continue;
        }
        if let Some(span) = find_substring_span(&chars, &tok_low) {
            raw_spans.push(span);
            continue;
        }
        let py_spans = find_pinyin_or_acronym_spans(&chars, &tok_low);
        if !py_spans.is_empty() {
            raw_spans.extend(py_spans);
            continue;
        }
        if tok_low.len() >= 2 {
            raw_spans.extend(find_subsequence_spans(&chars, &tok_low));
        }
    }

    merge_spans(raw_spans)
}

fn strip_query_prefix(raw: &str) -> &str {
    let t = raw.trim();
    if t.is_empty() || t.starts_with(':') || t.starts_with("= ") {
        return "";
    }
    if let Some((first, rest)) = t.split_once(char::is_whitespace) {
        let low = first.to_ascii_lowercase();
        let is_prefix = matches!(
            low.as_str(),
            "f" | "ev" | "cb" | "w" | "kill" | "ps" | "snip" | "b64" | "b64d"
        ) || low.starts_with('@');
        if is_prefix {
            return rest.trim();
        }
    } else if t.starts_with('@') {
        return "";
    }
    t
}

fn find_substring_span(chars: &[(usize, usize, char)], needle: &[char]) -> Option<(usize, usize)> {
    let n = needle.len();
    if n == 0 || n > chars.len() {
        return None;
    }
    for i in 0..=(chars.len() - n) {
        let mut matched = true;
        for j in 0..n {
            let hc = chars[i + j].2.to_lowercase().next().unwrap_or(chars[i + j].2);
            if hc != needle[j] {
                matched = false;
                break;
            }
        }
        if matched {
            return Some((chars[i].0, chars[i + n - 1].1));
        }
    }
    None
}

/// Maps Pinyin initials/syllables (including polyphones) and English word initials back to
/// exact byte ranges in `title`.
fn find_pinyin_or_acronym_spans(
    chars: &[(usize, usize, char)],
    needle: &[char],
) -> Vec<(usize, usize)> {
    if !needle.iter().all(|c| c.is_ascii_alphanumeric()) {
        return Vec::new();
    }
    let needle_str: String = needle.iter().collect();

    // 1. Word-initial / Chinese-initial contiguous match (e.g. `wx` -> 微信, `vsc` -> Visual Studio Code)
    let mut init_slots: Vec<(usize, usize, Vec<char>)> = Vec::new();
    let mut prev_sep = true;
    for &(bs, be, ch) in chars {
        if let Some(multi) = ch.to_pinyin_multi() {
            let mut inits = Vec::with_capacity(2);
            for py in multi {
                if let Some(f) = py.plain().chars().next() {
                    if !inits.contains(&f) {
                        inits.push(f);
                    }
                }
            }
            if !inits.is_empty() {
                init_slots.push((bs, be, inits));
            }
            prev_sep = false;
        } else if ch.is_alphanumeric() {
            if prev_sep || ch.is_uppercase() {
                init_slots.push((bs, be, vec![ch.to_ascii_lowercase()]));
            }
            prev_sep = false;
        } else {
            prev_sep = true;
        }
    }

    if needle.len() <= init_slots.len() {
        for start in 0..=(init_slots.len() - needle.len()) {
            let ok = (0..needle.len()).all(|k| init_slots[start + k].2.contains(&needle[k]));
            if ok {
                return init_slots[start..start + needle.len()]
                    .iter()
                    .map(|(s, e, _)| (*s, *e))
                    .collect();
            }
        }
    }

    // 2. Full / prefix Pinyin syllable match across consecutive characters (e.g. `weixin` -> 微信, `chongq` -> 重庆)
    for start in 0..chars.len() {
        if let Some(end_idx) = match_pinyin_from(chars, start, &needle_str) {
            return vec![(chars[start].0, chars[end_idx].1)];
        }
    }
    Vec::new()
}

fn match_pinyin_from(
    chars: &[(usize, usize, char)],
    start: usize,
    rem: &str,
) -> Option<usize> {
    if rem.is_empty() {
        return None;
    }
    let (_, _, ch) = chars[start];
    if let Some(multi) = ch.to_pinyin_multi() {
        let mut tried = Vec::with_capacity(2);
        for py in multi {
            let plain = py.plain();
            if tried.contains(&plain) {
                continue;
            }
            tried.push(plain);
            if plain.starts_with(rem) {
                return Some(start);
            }
            if let Some(rest) = rem.strip_prefix(plain) {
                if rest.is_empty() {
                    return Some(start);
                }
                if start + 1 < chars.len() {
                    if let Some(end) = match_pinyin_from(chars, start + 1, rest) {
                        return Some(end);
                    }
                }
            }
        }
        return None;
    }
    if ch.is_ascii_alphanumeric() {
        let low = ch.to_ascii_lowercase();
        let mut it = rem.chars();
        if it.next() == Some(low) {
            let rest = it.as_str();
            if rest.is_empty() {
                return Some(start);
            }
            if start + 1 < chars.len() {
                return match_pinyin_from(chars, start + 1, rest);
            }
        }
    }
    None
}

fn find_subsequence_spans(chars: &[(usize, usize, char)], needle: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(needle.len());
    let mut ci = 0usize;
    for &nc in needle {
        let mut found = None;
        while ci < chars.len() {
            let hc = chars[ci].2.to_lowercase().next().unwrap_or(chars[ci].2);
            ci += 1;
            if hc == nc {
                found = Some((chars[ci - 1].0, chars[ci - 1].1));
                break;
            }
        }
        match found {
            Some(span) => out.push(span),
            None => return Vec::new(),
        }
    }
    out
}

fn merge_spans(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    if spans.is_empty() {
        return spans;
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (s, e) in spans {
        if s >= e {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice_all<'a>(title: &'a str, spans: &[(usize, usize)]) -> Vec<&'a str> {
        spans.iter().map(|&(s, e)| &title[s..e]).collect()
    }

    #[test]
    fn highlights_exact_pinyin_polyphone_and_acronyms_safely() {
        assert_eq!(
            slice_all("Android Studio", &highlight_title_ranges("as", "Android Studio")),
            vec!["A", "S"]
        );
        assert_eq!(
            slice_all("Visual Studio Code", &highlight_title_ranges("vsc", "Visual Studio Code")),
            vec!["V", "S", "C"]
        );
        assert_eq!(
            slice_all("微信", &highlight_title_ranges("wx", "微信")),
            vec!["微信"]
        );
        assert_eq!(
            slice_all("微信", &highlight_title_ranges("weixin", "微信")),
            vec!["微信"]
        );
        assert_eq!(
            slice_all("重庆银行", &highlight_title_ranges("cqyh", "重庆银行")),
            vec!["重庆银行"]
        );
        assert_eq!(
            slice_all("1118_核对表.xlsx", &highlight_title_ranges("f 1118", "1118_核对表.xlsx")),
            vec!["1118"]
        );
        assert!(highlight_title_ranges(":stats", "内存统计").is_empty());
    }
}
