//! Domain style prompt presets for LLM translation engines (OpenAI-compatible & Ollama).

pub fn normalize_style(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "academic" => "academic",
        "code" => "code",
        "business" => "business",
        "polish" => "polish",
        _ => "standard",
    }
}

pub fn style_label_zh(style: &str) -> &'static str {
    match normalize_style(style) {
        "academic" => "学术",
        "code" => "代码",
        "business" => "商务",
        "polish" => "润色",
        _ => "标准",
    }
}

pub fn build_llm_system_prompt(dst_name: &str, style: &str) -> String {
    match normalize_style(style) {
        "academic" => format!(
            "You are an expert academic translator. Translate the user's text into formal, publication-grade {dst_name}. Preserve technical terminology, citations, and objective scholarly tone. Output only the translation."
        ),
        "code" => format!(
            "You are a senior software engineer translating technical docs and code comments into {dst_name}. Preserve code identifiers, API symbols, CLI flags, inline backticks, and Markdown structure verbatim. Output only the translation."
        ),
        "business" => format!(
            "You are a professional business communicator. Translate the user's text into polite, concise, executive-ready {dst_name} suitable for workplace emails and reports. Output only the translation."
        ),
        "polish" => format!(
            "You are a native-level editor in {dst_name}. Translate if needed and rewrite the user's text for maximum clarity, natural phrasing, and grammatical elegance while preserving the exact meaning. Output only the polished text."
        ),
        _ => format!(
            "You are a concise, accurate translator. Translate the user's text into {dst_name}. Output only the translation without extra commentary."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_styles_and_builds_distinct_prompts() {
        assert_eq!(normalize_style("CODE"), "code");
        assert_eq!(normalize_style("unknown"), "standard");
        assert_eq!(style_label_zh("academic"), "学术");
        let code_p = build_llm_system_prompt("Simplified Chinese", "code");
        assert!(code_p.contains("CLI flags") && code_p.contains("Simplified Chinese"));
        let acad_p = build_llm_system_prompt("English", "academic");
        assert!(acad_p.contains("publication-grade"));
    }
}
