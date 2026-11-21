/// Lightweight context/intent classifier (uTools-style "Super Match" foundation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectedIntent {
    Url(String),
    WindowsPath(String),
    HexColor { hex: String, r: u8, g: u8, b: u8 },
    UnixTimestamp { raw: i64, is_millis: bool },
    JsonSnippet,
    MathCandidate,
}

/// Inspects raw input text and extracts recognized structured intents without heavy regex engines.
pub fn detect_intents(input: &str) -> Vec<DetectedIntent> {
    let s = input.trim();
    if s.is_empty() || s.len() > 4096 {
        return Vec::new();
    }

    let mut out = Vec::new();

    // URL detection
    if (s.starts_with("http://") || s.starts_with("https://")) && !s.contains(char::is_whitespace) {
        out.push(DetectedIntent::Url(s.to_string()));
    }

    // Windows drive / UNC path
    if is_windows_path(s) {
        out.push(DetectedIntent::WindowsPath(s.to_string()));
    }

    // Hex color (#RGB or #RRGGBB)
    if let Some((r, g, b)) = parse_hex_color(s) {
        out.push(DetectedIntent::HexColor {
            hex: format!("#{r:02X}{g:02X}{b:02X}"),
            r,
            g,
            b,
        });
    }

    // Unix timestamp (10-digit seconds or 13-digit milliseconds)
    if (s.len() == 10 || s.len() == 13) && s.bytes().all(|b| b.is_ascii_digit())
        && let Ok(val) = s.parse::<i64>() {
            let is_millis = s.len() == 13;
            let secs = if is_millis { val / 1000 } else { val };
            if (946_684_800..=4_102_444_800).contains(&secs) {
                out.push(DetectedIntent::UnixTimestamp {
                    raw: val,
                    is_millis,
                });
            }
        }

    // JSON object / array
    if ((s.starts_with('{') && s.ends_with('}')) || (s.starts_with('[') && s.ends_with(']')))
        && serde_json::from_str::<serde_json::Value>(s).is_ok() {
            out.push(DetectedIntent::JsonSnippet);
        }

    // Math candidate
    if looks_like_math(s) {
        out.push(DetectedIntent::MathCandidate);
    }

    out
}

pub fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn is_windows_path(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
    {
        return true;
    }
    s.starts_with("\\\\") && s.len() > 4
}

fn looks_like_math(s: &str) -> bool {
    if s.starts_with("0x") || s.starts_with("0X") || s.starts_with("0b") || s.starts_with("0B") {
        return true;
    }
    let has_digit = s.bytes().any(|b| b.is_ascii_digit());
    let has_op = s
        .bytes()
        .any(|b| matches!(b, b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'('));
    has_digit && has_op
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_intents() {
        assert!(matches!(
            detect_intents("https://rust-lang.org")[0],
            DetectedIntent::Url(_)
        ));
        assert!(matches!(
            detect_intents("#7c6af7")[0],
            DetectedIntent::HexColor {
                r: 0x7c,
                g: 0x6a,
                b: 0xf7,
                ..
            }
        ));
        assert!(matches!(
            detect_intents("1700000000")[0],
            DetectedIntent::UnixTimestamp {
                is_millis: false,
                ..
            }
        ));
        assert!(matches!(
            detect_intents(r#"{"a":1}"#)[0],
            DetectedIntent::JsonSnippet
        ));
    }
}
