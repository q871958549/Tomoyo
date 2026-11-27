//! Pure-Rust bounded backtracking regex matcher with ReDoS step budget.

use crate::parser::{parse_pattern, ClassAtom, Node};

const MAX_STEPS: usize = 40_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatch {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub groups: Vec<Option<String>>,
}

pub struct CompiledRegex {
    root: Node,
    group_count: usize,
    pub case_insensitive: bool,
}

enum Cont<'a> {
    Done,
    Seq(&'a [Node], &'a Cont<'a>),
    EndGroup(usize, usize, &'a Cont<'a>),
    RepeatNext {
        inner: &'a Node,
        min: usize,
        max: usize,
        greedy: bool,
        count: usize,
        prev_pos: usize,
        next: &'a Cont<'a>,
    },
}

impl CompiledRegex {
    pub fn compile(pattern: &str, case_insensitive: bool) -> Result<Self, String> {
        let (root, group_count) = parse_pattern(pattern)?;
        Ok(Self { root, group_count, case_insensitive })
    }

    pub fn find_matches(&self, text: &str, global: bool) -> Result<Vec<RegexMatch>, String> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut pos = 0usize;
        let mut steps = 0usize;
        while pos <= chars.len() && out.len() < 32 {
            let mut caps = vec![None; self.group_count + 1];
            if let Some(end) = self.eval_node(&self.root, &chars, pos, &mut caps, &mut steps, &Cont::Done)? {
                caps[0] = Some((pos, end));
                let groups = caps[1..].iter().map(|g| g.map(|(a, b)| chars[a..b].iter().collect())).collect();
                out.push(RegexMatch {
                    start: pos,
                    end,
                    text: chars[pos..end].iter().collect(),
                    groups,
                });
                if !global { break; }
                pos = if end > pos { end } else { pos + 1 };
            } else {
                pos += 1;
            }
        }
        Ok(out)
    }

    fn eval_cont(
        &self,
        cont: &Cont<'_>,
        chars: &[char],
        pos: usize,
        caps: &mut Vec<Option<(usize, usize)>>,
        steps: &mut usize,
    ) -> Result<Option<usize>, String> {
        match cont {
            Cont::Done => Ok(Some(pos)),
            Cont::Seq([], next) => self.eval_cont(next, chars, pos, caps, steps),
            Cont::Seq([head, tail @ ..], next) => {
                let rest = Cont::Seq(tail, next);
                self.eval_node(head, chars, pos, caps, steps, &rest)
            }
            Cont::EndGroup(gid, start, next) => {
                let prev = caps[*gid];
                caps[*gid] = Some((*start, pos));
                if let Some(end) = self.eval_cont(next, chars, pos, caps, steps)? {
                    return Ok(Some(end));
                }
                caps[*gid] = prev;
                Ok(None)
            }
            Cont::RepeatNext { inner, min, max, greedy, count, prev_pos, next } => {
                if pos == *prev_pos && *count >= *min {
                    return self.eval_cont(next, chars, pos, caps, steps);
                }
                self.eval_repeat(inner, *min, *max, *greedy, *count, chars, pos, caps, steps, next)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_node(
        &self,
        node: &Node,
        chars: &[char],
        pos: usize,
        caps: &mut Vec<Option<(usize, usize)>>,
        steps: &mut usize,
        cont: &Cont<'_>,
    ) -> Result<Option<usize>, String> {
        *steps += 1;
        if *steps > MAX_STEPS {
            return Err("触发 ReDoS 步数上限保护 (40,000 步)".into());
        }
        match node {
            Node::Seq(seq) => self.eval_cont(&Cont::Seq(seq, cont), chars, pos, caps, steps),
            Node::Alt(alts) => {
                for branch in alts {
                    let snap = caps.clone();
                    if let Some(end) = self.eval_node(branch, chars, pos, caps, steps, cont)? {
                        return Ok(Some(end));
                    }
                    *caps = snap;
                }
                Ok(None)
            }
            Node::Group(gid, inner) => {
                let tail = Cont::EndGroup(*gid, pos, cont);
                self.eval_node(inner, chars, pos, caps, steps, &tail)
            }
            Node::NonCap(inner) => self.eval_node(inner, chars, pos, caps, steps, cont),
            Node::Repeat { inner, min, max, greedy } => {
                self.eval_repeat(inner, *min, *max, *greedy, 0, chars, pos, caps, steps, cont)
            }
            Node::AnchorStart => (pos == 0).then(|| self.eval_cont(cont, chars, pos, caps, steps)).transpose().map(Option::flatten),
            Node::AnchorEnd => (pos == chars.len()).then(|| self.eval_cont(cont, chars, pos, caps, steps)).transpose().map(Option::flatten),
            Node::WordBoundary | Node::NonWordBoundary => {
                let prev_w = pos > 0 && is_word_char(chars[pos - 1]);
                let next_w = pos < chars.len() && is_word_char(chars[pos]);
                let ok = if matches!(node, Node::WordBoundary) { prev_w != next_w } else { prev_w == next_w };
                if ok { self.eval_cont(cont, chars, pos, caps, steps) } else { Ok(None) }
            }
            Node::Any => match chars.get(pos) {
                Some(&c) if c != '\n' => self.eval_cont(cont, chars, pos + 1, caps, steps),
                _ => Ok(None),
            },
            Node::Literal(lit) => match chars.get(pos) {
                Some(&c) if self.eq_char(c, *lit) => self.eval_cont(cont, chars, pos + 1, caps, steps),
                _ => Ok(None),
            },
            Node::Builtin(atom) => match chars.get(pos) {
                Some(&c) if self.match_atom(atom, c) => self.eval_cont(cont, chars, pos + 1, caps, steps),
                _ => Ok(None),
            },
            Node::Class { negated, items } => match chars.get(pos) {
                Some(&c) if items.iter().any(|a| self.match_atom(a, c)) ^ *negated => {
                    self.eval_cont(cont, chars, pos + 1, caps, steps)
                }
                _ => Ok(None),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_repeat(
        &self,
        inner: &Node,
        min: usize,
        max: usize,
        greedy: bool,
        count: usize,
        chars: &[char],
        pos: usize,
        caps: &mut Vec<Option<(usize, usize)>>,
        steps: &mut usize,
        cont: &Cont<'_>,
    ) -> Result<Option<usize>, String> {
        let can_stop = count >= min;
        let can_more = count < max;
        if !greedy && can_stop {
            let snap = caps.clone();
            if let Some(end) = self.eval_cont(cont, chars, pos, caps, steps)? {
                return Ok(Some(end));
            }
            *caps = snap;
        }
        if can_more {
            let next_cont = Cont::RepeatNext {
                inner,
                min,
                max,
                greedy,
                count: count + 1,
                prev_pos: pos,
                next: cont,
            };
            let snap = caps.clone();
            if let Some(end) = self.eval_node(inner, chars, pos, caps, steps, &next_cont)? {
                return Ok(Some(end));
            }
            *caps = snap;
        }
        if greedy && can_stop {
            return self.eval_cont(cont, chars, pos, caps, steps);
        }
        Ok(None)
    }

    fn eq_char(&self, a: char, b: char) -> bool {
        a == b || (self.case_insensitive && a.to_ascii_lowercase() == b.to_ascii_lowercase())
    }

    fn match_atom(&self, atom: &ClassAtom, c: char) -> bool {
        match atom {
            ClassAtom::Char(x) => self.eq_char(c, *x),
            ClassAtom::Range(lo, hi) => {
                (c >= *lo && c <= *hi)
                    || (self.case_insensitive && {
                        let lc = c.to_ascii_lowercase();
                        lc >= lo.to_ascii_lowercase() && lc <= hi.to_ascii_lowercase()
                    })
            }
            ClassAtom::Digit => c.is_ascii_digit(),
            ClassAtom::NonDigit => !c.is_ascii_digit(),
            ClassAtom::Word => c.is_ascii_alphanumeric() || c == '_',
            ClassAtom::NonWord => !(c.is_ascii_alphanumeric() || c == '_'),
            ClassAtom::Space => c.is_whitespace(),
            ClassAtom::NonSpace => !c.is_whitespace(),
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn highlight_matches(input: &str, matches: &[RegexMatch]) -> String {
    if matches.is_empty() { return input.to_string(); }
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut cur = 0usize;
    for m in matches {
        if m.start < cur || m.end > chars.len() { continue; }
        out.extend(&chars[cur..m.start]);
        out.push('【');
        out.extend(&chars[m.start..m.end]);
        out.push('】');
        cur = m.end;
    }
    if cur < chars.len() { out.extend(&chars[cur..]); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_groups_alternation_flags_and_lazy_quantifiers() {
        let re = CompiledRegex::compile(r"(\w+)@([\w.]+)", false).unwrap();
        let hits = re.find_matches("hi alice@example.com & bob@tomoyo.dev", true).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].text, "alice@example.com");
        assert_eq!(hits[0].groups, vec![Some("alice".into()), Some("example.com".into())]);
        assert!(highlight_matches("hi alice@example.com & bob@tomoyo.dev", &hits).contains("【alice@example.com】"));

        let nested = CompiledRegex::compile(r"(a+)+a", false).unwrap();
        assert_eq!(nested.find_matches("aaa", false).unwrap()[0].text, "aaa");

        let ci = CompiledRegex::compile(r"^(?:foo|bar)\d+?$", true).unwrap();
        assert_eq!(ci.find_matches("BAR123", false).unwrap()[0].text, "BAR123");
    }

    #[test]
    fn guards_against_catastrophic_backtracking() {
        let evil = CompiledRegex::compile(r"(a+)+b", false).unwrap();
        assert!(evil.find_matches(&"a".repeat(26), false).is_err());
    }
}
