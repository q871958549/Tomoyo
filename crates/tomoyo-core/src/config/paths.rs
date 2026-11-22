//! Cross-platform scan-directory & excluded-process-name validation for `TomoyoConfig`.

/// Normalizes and validates a privacy-excluded process filename.
/// On Windows, ensures `.exe` suffix (`keepass` -> `keepass.exe`); on Linux/Unix,
/// accepts bare binary/process names (`keepassxc`, `bitwarden`) as well as `.exe`.
pub fn normalize_exe_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return None;
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return None;
    }
    #[allow(unused_mut)]
    let mut low = trimmed.to_ascii_lowercase();
    #[cfg(windows)]
    if !low.ends_with(".exe") {
        low.push_str(".exe");
    }
    let stem = low.strip_suffix(".exe").unwrap_or(&low);
    if stem.is_empty()
        || !stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b' '))
    {
        return None;
    }
    Some(low)
}

/// Validates a candidate custom scan or save directory path.
/// Blocks drive roots (`C:\`), Linux OS roots (`/`, `/usr`, `/etc`, ...),
/// UNC shares (`\\host\share`), relative/traversal paths (`..`),
/// and critical Windows system roots (`C:\Windows`, `ProgramData`, `$Recycle.Bin`).
pub fn validate_scan_dir(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return Err("路径不能为空");
    }
    if trimmed.len() > 260 || trimmed.chars().any(|c| c.is_control()) {
        return Err("路径长度超限或包含非法控制字符");
    }
    if trimmed.starts_with(r"\\") || trimmed.starts_with("//") {
        return Err("不支持网络 UNC 共享路径");
    }

    if trimmed.starts_with('/') {
        return validate_unix_scan_dir(trimmed);
    }

    let normalized = trimmed.replace('/', "\\");
    if normalized.split('\\').any(|seg| seg == "..") {
        return Err("路径不允许包含上级跳转 (..)");
    }

    let bytes = normalized.as_bytes();
    let is_win_abs = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes[2] == b'\\';
    if !is_win_abs {
        return Err("请通过浏览框选择完整绝对目录路径");
    }

    let clean = normalized.trim_end_matches('\\').to_string();
    if clean.len() <= 2 {
        return Err("禁止扫描整个磁盘根目录（易引发卡顿与异常句柄占用）");
    }

    let low = clean.to_ascii_lowercase();
    let blocked_prefixes = [
        r"c:\windows",
        r"c:\$recycle.bin",
        r"c:\system volume information",
        r"c:\programdata",
        r"c:\recovery",
        r"c:\perflogs",
    ];
    if blocked_prefixes
        .iter()
        .any(|p| low == *p || low.starts_with(&format!(r"{p}\")))
    {
        return Err("禁止将 Windows 系统核心目录加入扫描列表");
    }
    if let Ok(sys_root) = std::env::var("SystemRoot") {
        let sr = sys_root.trim_end_matches('\\').to_ascii_lowercase();
        if !sr.is_empty() && (low == sr || low.starts_with(&format!(r"{sr}\"))) {
            return Err("禁止扫描 Windows 系统目录");
        }
    }
    Ok(clean)
}

fn validate_unix_scan_dir(trimmed: &str) -> Result<String, &'static str> {
    if trimmed.contains('\\') || trimmed.split('/').any(|seg| seg == "..") {
        return Err("路径不允许包含上级跳转 (..) 或反斜杠");
    }
    let clean = trimmed.trim_end_matches('/');
    if clean.is_empty() {
        return Err("禁止扫描整个磁盘根目录（易引发卡顿与异常句柄占用）");
    }
    let blocked = [
        "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/proc", "/root", "/run", "/sbin",
        "/sys", "/tmp", "/usr", "/var",
    ];
    if blocked
        .iter()
        .any(|p| clean == *p || clean.starts_with(&format!("{p}/")))
    {
        return Err("禁止将 Linux 系统核心目录加入扫描列表");
    }
    Ok(clean.to_string())
}

pub fn default_hotkey_excluded_apps() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        vec![
            "rustdesk".into(),
            "remmina".into(),
            "moonlight".into(),
            "parsec".into(),
            "anydesk".into(),
            "sunloginclient".into(),
        ]
    }
    #[cfg(not(target_os = "linux"))]
    {
        vec![
            "raylink.exe".into(),
            "mstsc.exe".into(),
            "parsec.exe".into(),
            "moonlight.exe".into(),
            "todesk.exe".into(),
            "sunloginclient.exe".into(),
            "anydesk.exe".into(),
        ]
    }
}

pub fn default_excluded_apps() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        vec![
            "keepassxc".into(),
            "bitwarden".into(),
            "1password".into(),
            "enpass".into(),
        ]
    }
    #[cfg(not(target_os = "linux"))]
    {
        vec![
            "keepass.exe".into(),
            "keepassxc.exe".into(),
            "bitwarden.exe".into(),
            "1password.exe".into(),
            "enpass.exe".into(),
        ]
    }
}

pub fn default_web_searches() -> Vec<super::WebSearchEngine> {
    let mk = |k: &str, n: &str, u: &str| super::WebSearchEngine {
        keyword: k.into(),
        name: n.into(),
        url: u.into(),
    };
    vec![
        mk("g", "Google Search", "https://www.google.com/search?q={q}"),
        mk("gh", "GitHub Search", "https://github.com/search?q={q}"),
        mk("crates", "crates.io (Rust)", "https://crates.io/search?q={q}"),
        mk("docs", "docs.rs (Rust Docs)", "https://docs.rs/releases/search?query={q}"),
        mk("bd", "Baidu 百度搜索", "https://www.baidu.com/s?wd={q}"),
        mk("zh", "Zhihu 知乎搜索", "https://www.zhihu.com/search?type=content&q={q}"),
        mk("mdn", "MDN Web Docs", "https://developer.mozilla.org/en-US/search?q={q}"),
    ]
}
