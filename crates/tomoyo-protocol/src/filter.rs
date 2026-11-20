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
        }
    }

    pub fn is_file_only(self) -> bool {
        matches!(
            self,
            Self::Folder | Self::Pdf | Self::Excel | Self::Word | Self::Ppt | Self::Image | Self::Archive
        )
    }

    /// Everything search expression appended automatically when this filter is active.
    pub fn everything_expr(self) -> Option<&'static str> {
        match self {
            Self::Folder => Some("folder:"),
            Self::Pdf => Some("ext:pdf"),
            Self::Excel => Some("ext:xlsx;xls;xlsm;xlsb;csv"),
            Self::Word => Some("ext:docx;doc;rtf;wps"),
            Self::Ppt => Some("ext:pptx;ppt;ppsx;dps"),
            Self::Image => Some("ext:png;jpg;jpeg;webp;gif;bmp;svg;ico"),
            Self::Archive => Some("ext:zip;7z;rar;tar;gz"),
            Self::All | Self::App => None,
        }
    }

    pub fn cycle(self, forward: bool) -> Self {
        let idx = Self::ALL.iter().position(|&f| f == self).unwrap_or(0);
        let len = Self::ALL.len();
        let next = if forward {
            (idx + 1) % len
        } else {
            (idx + len - 1) % len
        };
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
    fn cycles_and_parses_inline_tags() {
        assert_eq!(SearchFilter::All.cycle(true), SearchFilter::App);
        assert_eq!(SearchFilter::All.cycle(false), SearchFilter::Archive);
        assert_eq!(SearchFilter::parse_inline_tag("@pdf"), Some(SearchFilter::Pdf));
        assert_eq!(SearchFilter::parse_inline_tag("@xlsx"), Some(SearchFilter::Excel));
        assert_eq!(SearchFilter::parse_inline_tag("@dir"), Some(SearchFilter::Folder));
        assert_eq!(SearchFilter::parse_inline_tag("pdf"), None);
    }
}
