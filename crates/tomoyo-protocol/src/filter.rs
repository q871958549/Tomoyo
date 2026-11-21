use serde::{Deserialize, Serialize};

/// Active result-type filter chip selected in the launcher bar or via `@tag` syntax.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchFilter {
    #[default]
    All,
    App,
    Folder,
    Pdf,
    Excel,
    Word,
    Ppt,
    Image,
    Archive,
    /// User-defined extensible filter tab (index into `TomoyoConfig::custom_filters`).
    Custom(u8),
}

fn default_tab_enabled() -> bool {
    true
}

/// User-extensible search filter tab shown in the launcher filter bar & invocable via `@tag`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomFilterTab {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub everything_expr: String,
    #[serde(default = "default_tab_enabled")]
    pub enabled: bool,
}

impl CustomFilterTab {
    /// Builds the Everything search expression for this tab (`everything_expr` or `ext:a;b;c`).
    pub fn effective_everything_expr(&self) -> Option<String> {
        let custom = self.everything_expr.trim();
        if !custom.is_empty() {
            return Some(custom.to_string());
        }
        if !self.extensions.is_empty() {
            return Some(format!("ext:{}", self.extensions.join(";")));
        }
        None
    }

    pub fn matches_extension(&self, ext_lower: &str) -> bool {
        self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext_lower))
    }

    pub fn matches_inline_tag(&self, token_lower: &str) -> bool {
        let Some(bare) = token_lower.strip_prefix('@') else { return false; };
        let t = self.tag.trim().trim_start_matches('@');
        (!t.is_empty() && t.eq_ignore_ascii_case(bare))
            || self.id.eq_ignore_ascii_case(bare)
            || self.label.eq_ignore_ascii_case(bare)
    }
}

/// Resolved visible tab entry rendered in the launcher filter bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleFilterTab {
    pub filter: SearchFilter,
    pub label: String,
    pub extensions: Vec<String>,
}

/// Builds the ordered list of visible filter chips (built-ins minus hidden + enabled custom tabs).
pub fn build_visible_filter_tabs(
    zh: bool,
    hidden_builtins: &[String],
    custom_tabs: &[CustomFilterTab],
) -> Vec<VisibleFilterTab> {
    let mut out = Vec::with_capacity(SearchFilter::ALL.len() + custom_tabs.len());
    for &f in &SearchFilter::ALL {
        if f != SearchFilter::All
            && hidden_builtins.iter().any(|h| h.eq_ignore_ascii_case(f.id_key()))
        {
            continue;
        }
        out.push(VisibleFilterTab {
            filter: f,
            label: f.label(zh).to_string(),
            extensions: f.builtin_extensions().iter().map(|s| (*s).to_string()).collect(),
        });
    }
    for (idx, tab) in custom_tabs.iter().enumerate().take(16) {
        if tab.enabled && !tab.label.trim().is_empty() && tab.effective_everything_expr().is_some() {
            out.push(VisibleFilterTab {
                filter: SearchFilter::Custom(idx as u8),
                label: tab.label.trim().to_string(),
                extensions: tab.extensions.clone(),
            });
        }
    }
    out
}

impl SearchFilter {
    pub const ALL: [SearchFilter; 9] = [
        Self::All,
        Self::App,
        Self::Folder,
        Self::Pdf,
        Self::Excel,
        Self::Word,
        Self::Ppt,
        Self::Image,
        Self::Archive,
    ];

    pub fn id_key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::App => "app",
            Self::Folder => "folder",
            Self::Pdf => "pdf",
            Self::Excel => "excel",
            Self::Word => "word",
            Self::Ppt => "ppt",
            Self::Image => "image",
            Self::Archive => "archive",
            Self::Custom(_) => "custom",
        }
    }

    pub fn label(self, zh: bool) -> &'static str {
        match (self, zh) {
            (Self::All, true) => "全部",
            (Self::All, false) => "All",
            (Self::App, true) => "应用",
            (Self::App, false) => "Apps",
            (Self::Folder, true) => "文件夹",
            (Self::Folder, false) => "Folders",
            (Self::Pdf, _) => "PDF",
            (Self::Excel, _) => "Excel",
            (Self::Word, _) => "Word",
            (Self::Ppt, _) => "PPT",
            (Self::Image, true) => "图片",
            (Self::Image, false) => "Images",
            (Self::Archive, true) => "压缩",
            (Self::Archive, false) => "Zip",
            (Self::Custom(_), true) => "自定义",
            (Self::Custom(_), false) => "Custom",
        }
    }

    pub fn builtin_extensions(self) -> &'static [&'static str] {
        match self {
            Self::Pdf => &["pdf"],
            Self::Excel => &["xls", "xlsx", "xlsm", "xlsb", "csv"],
            Self::Word => &["doc", "docx", "rtf", "wps"],
            Self::Ppt => &["ppt", "pptx", "ppsx", "dps"],
            Self::Image => &["png", "jpg", "jpeg", "webp", "bmp", "gif", "svg", "ico"],
            Self::Archive => &["zip", "7z", "rar", "tar", "gz", "xz"],
            _ => &[],
        }
    }

    pub fn is_file_only(self) -> bool {
        matches!(
            self,
            Self::Folder
                | Self::Pdf
                | Self::Excel
                | Self::Word
                | Self::Ppt
                | Self::Image
                | Self::Archive
                | Self::Custom(_)
        )
    }

    /// Everything search expression appended automatically when this built-in filter is active.
    pub fn everything_expr(self) -> Option<&'static str> {
        match self {
            Self::Folder => Some("folder:"),
            Self::Pdf => Some("ext:pdf"),
            Self::Excel => Some("ext:xlsx;xls;xlsm;xlsb;csv"),
            Self::Word => Some("ext:docx;doc;rtf;wps"),
            Self::Ppt => Some("ext:pptx;ppt;ppsx;dps"),
            Self::Image => Some("ext:png;jpg;jpeg;webp;gif;bmp;svg;ico"),
            Self::Archive => Some("ext:zip;7z;rar;tar;gz"),
            Self::All | Self::App | Self::Custom(_) => None,
        }
    }

    pub fn cycle_visible(self, visible: &[VisibleFilterTab], forward: bool) -> Self {
        if visible.is_empty() {
            return Self::All;
        }
        let idx = visible.iter().position(|t| t.filter == self).unwrap_or(0);
        let len = visible.len();
        let next = if forward { (idx + 1) % len } else { (idx + len - 1) % len };
        visible[next].filter
    }

    pub fn cycle(self, forward: bool) -> Self {
        let idx = Self::ALL.iter().position(|&f| f == self).unwrap_or(0);
        let len = Self::ALL.len();
        let next = if forward { (idx + 1) % len } else { (idx + len - 1) % len };
        Self::ALL[next]
    }

    pub fn from_index(idx: usize) -> Option<Self> {
        Self::ALL.get(idx).copied()
    }

    pub fn parse_inline_tag(token: &str) -> Option<Self> {
        let t = token.trim().to_ascii_lowercase();
        let tag = t.strip_prefix('@')?;
        match tag {
            "all" | "全部" => Some(Self::All),
            "app" | "apps" | "应用" => Some(Self::App),
            "dir" | "folder" | "folders" | "文件夹" => Some(Self::Folder),
            "pdf" => Some(Self::Pdf),
            "xls" | "xlsx" | "excel" | "表格" => Some(Self::Excel),
            "doc" | "docx" | "word" | "文档" => Some(Self::Word),
            "ppt" | "pptx" | "演示" => Some(Self::Ppt),
            "img" | "image" | "images" | "pic" | "图片" => Some(Self::Image),
            "zip" | "rar" | "7z" | "archive" | "压缩" => Some(Self::Archive),
            _ => None,
        }
    }

    pub fn parse_inline_tag_with_custom(token: &str, custom: &[CustomFilterTab]) -> Option<Self> {
        if let Some(b) = Self::parse_inline_tag(token) {
            return Some(b);
        }
        let low = token.trim().to_ascii_lowercase();
        custom
            .iter()
            .enumerate()
            .take(16)
            .find(|(_, tab)| tab.enabled && tab.matches_inline_tag(&low))
            .map(|(i, _)| Self::Custom(i as u8))
    }
}

/// Maps a lowercase file extension to a compact 3-letter badge for icons/placeholders.
pub fn extension_short_tag(ext_lower: &str) -> &'static str {
    match ext_lower {
        "pdf" => "PDF",
        "xlsx" | "xls" | "xlsm" | "xlsb" | "csv" => "XLS",
        "docx" | "doc" | "rtf" | "wps" => "DOC",
        "pptx" | "ppt" | "ppsx" | "dps" => "PPT",
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" | "ico" => "IMG",
        "zip" | "7z" | "rar" | "tar" | "gz" => "ZIP",
        "exe" | "lnk" | "msi" | "bat" | "cmd" => "APP",
        _ => "FILE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_and_parses_builtin_and_custom_tags() {
        assert_eq!(SearchFilter::All.cycle(true), SearchFilter::App);
        assert_eq!(SearchFilter::All.cycle(false), SearchFilter::Archive);
        assert_eq!(SearchFilter::parse_inline_tag("@pdf"), Some(SearchFilter::Pdf));
        let custom = vec![CustomFilterTab {
            id: "code".into(),
            label: "代码".into(),
            tag: "@code".into(),
            extensions: vec!["rs".into(), "ts".into()],
            everything_expr: String::new(),
            enabled: true,
        }];
        assert_eq!(
            SearchFilter::parse_inline_tag_with_custom("@code", &custom),
            Some(SearchFilter::Custom(0))
        );
        let vis = build_visible_filter_tabs(true, &["ppt".into()], &custom);
        assert!(!vis.iter().any(|t| t.filter == SearchFilter::Ppt));
        assert!(vis.iter().any(|t| t.filter == SearchFilter::Custom(0) && t.label == "代码"));
    }
}
