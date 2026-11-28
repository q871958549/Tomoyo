//! Prompt routing, natural-language subcommand parsing & desktop-concise system prompts.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiMode {
    Ask,
    Summarize,
    Polish,
    Commit,
    Diagnose,
    Regex,
    Filter,
    Shell,
}

impl AiMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Summarize => "sum",
            Self::Polish => "polish",
            Self::Commit => "commit",
            Self::Diagnose => "fix",
            Self::Regex => "regex",
            Self::Filter => "filter",
            Self::Shell => "cmd",
        }
    }

    pub fn from_action_or_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "sum" | "summarize" | "ai_summarize" => Self::Summarize,
            "polish" | "rewrite" | "ai_polish" => Self::Polish,
            "commit" => Self::Commit,
            "fix" | "err" | "debug" | "diagnose" | "ai_diagnose" => Self::Diagnose,
            "regex" | "re" => Self::Regex,
            "filter" | "ev" => Self::Filter,
            "cmd" | "sh" | "shell" => Self::Shell,
            _ => Self::Ask,
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Ask => "ASK",
            Self::Summarize => "SUM",
            Self::Polish => "EDIT",
            Self::Commit => "GIT",
            Self::Diagnose => "FIX",
            Self::Regex => "REGEX",
            Self::Filter => "FILTER",
            Self::Shell => "CMD",
        }
    }

    pub fn title_zh(self) -> &'static str {
        match self {
            Self::Ask => "快速问答",
            Self::Summarize => "提炼要点",
            Self::Polish => "润色改写",
            Self::Commit => "生成 Commit Message",
            Self::Diagnose => "报错诊断",
            Self::Regex => "自然语言 → 正则表达式",
            Self::Filter => "自然语言 → 文件筛选表达式",
            Self::Shell => "自然语言 → 终端命令",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAiInput {
    pub mode: AiMode,
    pub text: String,
    pub from_clipboard: bool,
}

pub fn parse_ai_input(args: &str, clipboard: Option<&str>) -> ParsedAiInput {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return ParsedAiInput {
            mode: AiMode::Ask,
            text: String::new(),
            from_clipboard: false,
        };
    }

    let (first, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((f, r)) => (f.trim(), r.trim()),
        None => (trimmed, ""),
    };

    let matched_mode = match first.to_ascii_lowercase().as_str() {
        "sum" | "summary" | "tldr" => Some(AiMode::Summarize),
        "polish" | "rewrite" | "edit" => Some(AiMode::Polish),
        "commit" | "git" => Some(AiMode::Commit),
        "fix" | "err" | "error" | "debug" => Some(AiMode::Diagnose),
        "regex" | "re" => Some(AiMode::Regex),
        "filter" | "ev" => Some(AiMode::Filter),
        "cmd" | "sh" | "shell" | "cli" => Some(AiMode::Shell),
        _ => match first {
            "总结" | "摘要" | "提炼" => Some(AiMode::Summarize),
            "润色" | "改写" => Some(AiMode::Polish),
            "报错" | "诊断" | "排错" => Some(AiMode::Diagnose),
            "正则" => Some(AiMode::Regex),
            "筛选" | "搜索" => Some(AiMode::Filter),
            "命令" | "脚本" => Some(AiMode::Shell),
            _ => None,
        },
    };

    if let Some(mode) = matched_mode {
        if !rest.is_empty() {
            return ParsedAiInput {
                mode,
                text: rest.to_string(),
                from_clipboard: false,
            };
        }
        let clip = clipboard.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
        return ParsedAiInput {
            mode,
            text: clip.to_string(),
            from_clipboard: !clip.is_empty(),
        };
    }

    ParsedAiInput {
        mode: AiMode::Ask,
        text: trimmed.to_string(),
        from_clipboard: false,
    }
}

pub fn build_messages(mode: AiMode, input: &str, reply_lang: &str) -> (String, String) {
    let lang_hint = if reply_lang.eq_ignore_ascii_case("en") {
        "Respond concisely in English."
    } else {
        "请用简洁清晰的简体中文回答。"
    };
    let os_hint = if cfg!(windows) {
        "Target OS: Windows PowerShell 7."
    } else {
        "Target OS: Linux bash/zsh."
    };

    let sys = match mode {
        AiMode::Ask => format!(
            "You are Tomoyo's concise local desktop assistant. Give direct, actionable answers without filler. {lang_hint}"
        ),
        AiMode::Summarize => format!(
            "Extract the key conclusions and action items as short bullet points (max 5 bullets, no preamble). {lang_hint}"
        ),
        AiMode::Polish => format!(
            "Rewrite and polish the user's text for clarity and tone while preserving exact meaning. Output only the improved text. {lang_hint}"
        ),
        AiMode::Commit => format!(
            "Generate a Conventional Commit message (feat/fix/refactor/docs/chore) from the input diff or description. Output subject line + optional concise body only. {lang_hint}"
        ),
        AiMode::Diagnose => format!(
            "Diagnose the error/stack trace. Output: 1) Root cause (1-2 sentences), 2) Exact fix steps or command. {os_hint} {lang_hint}"
        ),
        AiMode::Regex => format!(
            "Generate a standard regex matching the user's requirement. Line 1 MUST be ONLY `/pattern/flags` (ready to copy into Tomoyo `re`), followed on next lines by a brief explanation. {lang_hint}"
        ),
        AiMode::Filter => format!(
            "Convert the user's request into a single Everything / Tomoyo `f` filter expression (supporting `ext:`, `dm:today`, `size:>10mb`, quotes). Line 1 MUST be ONLY the filter expression, followed by a brief note. {lang_hint}"
        ),
        AiMode::Shell => format!(
            "Provide a safe one-line terminal command for the request. {os_hint} Line 1 MUST be ONLY the command (no markdown fences), followed by a 1-sentence warning if destructive. {lang_hint}"
        ),
    };

    (sys, input.trim().to_string())
}

pub fn extract_first_code_or_line(reply: &str) -> String {
    for raw in reply.lines() {
        let s = raw.trim();
        if s.is_empty() || s.starts_with("```") {
            continue;
        }
        return s.trim_matches('`').trim().to_string();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes_and_clipboard_fallback() {
        let p1 = parse_ai_input("sum hello world", None);
        assert_eq!(p1.mode, AiMode::Summarize);
        assert_eq!(p1.text, "hello world");
        assert!(!p1.from_clipboard);

        let p2 = parse_ai_input("fix", Some("error[E0308]: mismatched types"));
        assert_eq!(p2.mode, AiMode::Diagnose);
        assert!(p2.from_clipboard);
        assert!(p2.text.contains("E0308"));

        let p3 = parse_ai_input("正则 匹配所有邮箱", None);
        assert_eq!(p3.mode, AiMode::Regex);
        assert_eq!(p3.text, "匹配所有邮箱");

        let p4 = parse_ai_input("Rust lifetime elision", None);
        assert_eq!(p4.mode, AiMode::Ask);
    }

    #[test]
    fn builds_concise_prompts_and_extracts_first_line() {
        let (sys, usr) = build_messages(AiMode::Regex, "ipv4", "zh");
        assert!(sys.contains("/pattern/flags"));
        assert_eq!(usr, "ipv4");
        assert_eq!(
            extract_first_code_or_line("```\n`/\\d+/g`\n解释\n```"),
            "/\\d+/g"
        );
    }
}
