//! Zero-dependency recursive descent math expression evaluator (supports `0x`/`0b`, `^`, `sqrt`, trig).

pub fn eval_expr(input: &str) -> Option<f64> {
    let mut p = Parser::new(input);
    let val = p.parse_add_sub()?;
    p.skip_ws();
    if p.pos == p.src.len() {
        Some(val)
    } else {
        None
    }
}

pub fn format_number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-10 && v.abs() < 1e15 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.8}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let rf = r as f64 / 255.0;
    let gf = g as f64 / 255.0;
    let bf = b as f64 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l * 100.0);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let mut h = if (max - rf).abs() < f64::EPSILON {
        (gf - bf) / d + if gf < bf { 6.0 } else { 0.0 }
    } else if (max - gf).abs() < f64::EPSILON {
        (bf - rf) / d + 2.0
    } else {
        (rf - gf) / d + 4.0
    };
    h *= 60.0;
    (h, s * 100.0, l * 100.0)
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            src: s.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.src.get(self.pos).copied()
    }

    fn parse_add_sub(&mut self) -> Option<f64> {
        let mut left = self.parse_mul_div()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.pos += 1;
                    left += self.parse_mul_div()?;
                }
                Some(b'-') => {
                    self.pos += 1;
                    left -= self.parse_mul_div()?;
                }
                _ => break,
            }
        }
        Some(left)
    }

    fn parse_mul_div(&mut self) -> Option<f64> {
        let mut left = self.parse_pow()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.pos += 1;
                    left *= self.parse_pow()?;
                }
                Some(b'/') => {
                    self.pos += 1;
                    let r = self.parse_pow()?;
                    if r == 0.0 {
                        return None;
                    }
                    left /= r;
                }
                Some(b'%') => {
                    self.pos += 1;
                    let r = self.parse_pow()?;
                    if r == 0.0 {
                        return None;
                    }
                    left %= r;
                }
                _ => break,
            }
        }
        Some(left)
    }

    fn parse_pow(&mut self) -> Option<f64> {
        let base = self.parse_unary()?;
        if self.peek() == Some(b'^') {
            self.pos += 1;
            let exp = self.parse_pow()?;
            Some(base.powf(exp))
        } else {
            Some(base)
        }
    }

    fn parse_unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(b'-') => {
                self.pos += 1;
                Some(-self.parse_unary()?)
            }
            Some(b'+') => {
                self.pos += 1;
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Option<f64> {
        let ch = self.peek()?;
        if ch == b'(' {
            self.pos += 1;
            let val = self.parse_add_sub()?;
            if self.peek() != Some(b')') {
                return None;
            }
            self.pos += 1;
            return Some(val);
        }

        if ch == b'0' && self.pos + 2 < self.src.len() {
            let next = self.src[self.pos + 1];
            if next == b'x' || next == b'X' {
                self.pos += 2;
                let start = self.pos;
                while self.pos < self.src.len() && self.src[self.pos].is_ascii_hexdigit() {
                    self.pos += 1;
                }
                let s = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
                return u64::from_str_radix(s, 16).ok().map(|v| v as f64);
            }
            if next == b'b' || next == b'B' {
                self.pos += 2;
                let start = self.pos;
                while self.pos < self.src.len() && matches!(self.src[self.pos], b'0' | b'1') {
                    self.pos += 1;
                }
                let s = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
                return u64::from_str_radix(s, 2).ok().map(|v| v as f64);
            }
        }

        if ch.is_ascii_digit() || ch == b'.' {
            let start = self.pos;
            while self.pos < self.src.len()
                && (self.src[self.pos].is_ascii_digit() || self.src[self.pos] == b'.')
            {
                self.pos += 1;
            }
            let s = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
            return s.parse::<f64>().ok();
        }

        if ch.is_ascii_alphabetic() {
            let start = self.pos;
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_alphanumeric() {
                self.pos += 1;
            }
            let ident = std::str::from_utf8(&self.src[start..self.pos])
                .ok()?
                .to_lowercase();
            return match ident.as_str() {
                "pi" => Some(std::f64::consts::PI),
                "e" => Some(std::f64::consts::E),
                fn_name => {
                    if self.peek() != Some(b'(') {
                        return None;
                    }
                    self.pos += 1;
                    let arg = self.parse_add_sub()?;
                    if self.peek() != Some(b')') {
                        return None;
                    }
                    self.pos += 1;
                    match fn_name {
                        "sqrt" => Some(arg.sqrt()),
                        "abs" => Some(arg.abs()),
                        "sin" => Some(arg.sin()),
                        "cos" => Some(arg.cos()),
                        "tan" => Some(arg.tan()),
                        "ln" => Some(arg.ln()),
                        "log" | "log10" => Some(arg.log10()),
                        "floor" => Some(arg.floor()),
                        "ceil" => Some(arg.ceil()),
                        "round" => Some(arg.round()),
                        _ => None,
                    }
                }
            };
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_math_and_bases() {
        assert_eq!(eval_expr("(2 + 3) * 4"), Some(20.0));
        assert_eq!(eval_expr("2^10"), Some(1024.0));
        assert_eq!(eval_expr("0xff + 1"), Some(256.0));
        assert_eq!(eval_expr("sqrt(144)"), Some(12.0));
        assert_eq!(eval_expr("0b1010"), Some(10.0));
    }
}
