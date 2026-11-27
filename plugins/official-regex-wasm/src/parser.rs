//! Regex AST & recursive-descent parser for pure-WASM Regex Lab.

const MAX_GROUPS: usize = 9;

#[derive(Debug, Clone)]
pub enum ClassAtom {
    Char(char),
    Range(char, char),
    Digit,
    NonDigit,
    Word,
    NonWord,
    Space,
    NonSpace,
}

#[derive(Debug, Clone)]
pub enum Node {
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Group(usize, Box<Node>),
    NonCap(Box<Node>),
    Repeat { inner: Box<Node>, min: usize, max: usize, greedy: bool },
    AnchorStart,
    AnchorEnd,
    WordBoundary,
    NonWordBoundary,
    Any,
    Literal(char),
    Class { negated: bool, items: Vec<ClassAtom> },
    Builtin(ClassAtom),
}

pub fn parse_pattern(pattern: &str) -> Result<(Node, usize), String> {
    if pattern.is_empty() || pattern.chars().count() > 240 {
        return Err("正则表达式为空或超过 240 字符上限".into());
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut p = Parser { chars: &chars, pos: 0, group_count: 0 };
    let root = p.parse_alt()?;
    if p.pos != chars.len() {
        return Err("存在多余未匹配的括号或符号".into());
    }
    Ok((root, p.group_count))
}

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
    group_count: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> { self.chars.get(self.pos).copied() }
    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }

    fn parse_alt(&mut self) -> Result<Node, String> {
        let mut alts = vec![self.parse_seq()?];
        while self.peek() == Some('|') {
            self.pos += 1;
            alts.push(self.parse_seq()?);
        }
        Ok(if alts.len() == 1 { alts.pop().unwrap() } else { Node::Alt(alts) })
    }

    fn parse_seq(&mut self) -> Result<Node, String> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == ')' || c == '|' { break; }
            nodes.push(self.parse_quantified()?);
        }
        Ok(if nodes.len() == 1 { nodes.pop().unwrap() } else { Node::Seq(nodes) })
    }

    fn parse_quantified(&mut self) -> Result<Node, String> {
        let atom = self.parse_atom()?;
        let (min, max) = match self.peek() {
            Some('*') => { self.pos += 1; (0, usize::MAX) }
            Some('+') => { self.pos += 1; (1, usize::MAX) }
            Some('?') => { self.pos += 1; (0, 1) }
            Some('{') => {
                let save = self.pos;
                if let Some(bounds) = self.try_parse_braces() {
                    bounds
                } else {
                    self.pos = save;
                    return Ok(atom);
                }
            }
            _ => return Ok(atom),
        };
        let greedy = if self.peek() == Some('?') { self.pos += 1; false } else { true };
        Ok(Node::Repeat { inner: Box::new(atom), min, max, greedy })
    }

    fn try_parse_braces(&mut self) -> Option<(usize, usize)> {
        self.pos += 1;
        let min = self.read_uint()?;
        let max = match self.peek() {
            Some('}') => { self.pos += 1; min }
            Some(',') => {
                self.pos += 1;
                if self.peek() == Some('}') {
                    self.pos += 1;
                    usize::MAX
                } else {
                    let hi = self.read_uint()?;
                    if self.next() != Some('}') || hi < min { return None; }
                    hi
                }
            }
            _ => return None,
        };
        Some((min, max.min(512)))
    }

    fn read_uint(&mut self) -> Option<usize> {
        let start = self.pos;
        let mut val = 0usize;
        while let Some(c) = self.peek()
            && c.is_ascii_digit()
        {
            val = val.saturating_mul(10).saturating_add((c as u8 - b'0') as usize);
            self.pos += 1;
        }
        (self.pos > start).then_some(val)
    }

    fn parse_atom(&mut self) -> Result<Node, String> {
        match self.next().ok_or("表达式意外结束")? {
            '^' => Ok(Node::AnchorStart),
            '$' => Ok(Node::AnchorEnd),
            '.' => Ok(Node::Any),
            '(' => {
                let non_cap = self.chars.get(self.pos..self.pos + 2) == Some(&['?', ':']);
                let gid = if non_cap {
                    self.pos += 2;
                    None
                } else if self.group_count < MAX_GROUPS {
                    self.group_count += 1;
                    Some(self.group_count)
                } else {
                    None
                };
                let inner = self.parse_alt()?;
                if self.next() != Some(')') { return Err("缺少右括号 `)`".into()); }
                Ok(match gid {
                    Some(id) => Node::Group(id, Box::new(inner)),
                    None => Node::NonCap(Box::new(inner)),
                })
            }
            '[' => self.parse_class(),
            '\\' => self.parse_escape(),
            '*' | '+' | '?' => Err("量词前缺少可重复元素".into()),
            c => Ok(Node::Literal(c)),
        }
    }

    fn parse_escape(&mut self) -> Result<Node, String> {
        let c = self.next().ok_or("转义符 `\\` 末尾缺失字符")?;
        Ok(match c {
            'b' => Node::WordBoundary,
            'B' => Node::NonWordBoundary,
            'd' => Node::Builtin(ClassAtom::Digit),
            'D' => Node::Builtin(ClassAtom::NonDigit),
            'w' => Node::Builtin(ClassAtom::Word),
            'W' => Node::Builtin(ClassAtom::NonWord),
            's' => Node::Builtin(ClassAtom::Space),
            'S' => Node::Builtin(ClassAtom::NonSpace),
            'n' => Node::Literal('\n'),
            'r' => Node::Literal('\r'),
            't' => Node::Literal('\t'),
            other => Node::Literal(other),
        })
    }

    fn parse_class(&mut self) -> Result<Node, String> {
        let negated = self.peek() == Some('^');
        if negated { self.pos += 1; }
        let mut items = Vec::new();
        while let Some(c) = self.next() {
            if c == ']' && !items.is_empty() {
                return Ok(Node::Class { negated, items });
            }
            let atom = if c == '\\' {
                match self.next().ok_or("字符类转义未闭合")? {
                    'd' => ClassAtom::Digit,
                    'D' => ClassAtom::NonDigit,
                    'w' => ClassAtom::Word,
                    'W' => ClassAtom::NonWord,
                    's' => ClassAtom::Space,
                    'S' => ClassAtom::NonSpace,
                    'n' => ClassAtom::Char('\n'),
                    'r' => ClassAtom::Char('\r'),
                    't' => ClassAtom::Char('\t'),
                    x => ClassAtom::Char(x),
                }
            } else {
                ClassAtom::Char(c)
            };
            if let ClassAtom::Char(start_c) = atom
                && self.peek() == Some('-')
                && !matches!(self.chars.get(self.pos + 1), None | Some(']'))
            {
                self.pos += 1;
                let end_c = match self.next().ok_or("字符区间未闭合")? {
                    '\\' => self.next().ok_or("字符区间转义未闭合")?,
                    x => x,
                };
                items.push(ClassAtom::Range(start_c.min(end_c), start_c.max(end_c)));
            } else {
                items.push(atom);
            }
        }
        Err("字符类 `[` 缺少闭合 `]`".into())
    }
}
