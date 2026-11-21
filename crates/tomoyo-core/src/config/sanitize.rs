//! Defensive validation & clamping for `TomoyoConfig`.
//!
//! Enforces strict domain bounds, blocks dangerous OS/root scan directories,
//! rejects non-HTTP(S) web search templates, and prevents crashes or resource exhaustion
//! even when `config.toml` or WebView IPC payloads are malformed or adversarial.

use super::{
    default_plugin_registries, default_web_searches, parse_hotkey, TomoyoConfig, WebSearchEngine,
};
use std::collections::HashSet;

pub const MAX_SCAN_DEPTH: usize = 4;
pub const MAX_EXTRA_DIRS: usize = 16;
pub const MAX_WEB_SEARCHES: usize = 24;
pub const MAX_EXTRA_FILTER_CHARS: usize = 160;
pub const MAX_FAVORITES: usize = 12;
pub const MAX_EXCLUDED_APPS: usize = 32;

/// Normalizes and validates a privacy-excluded Windows process filename (e.g., `keepass` -> `keepass.exe`).
pub fn normalize_exe_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return None;
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return None;
    }
    let mut low = trimmed.to_ascii_lowercase();
    if !low.ends_with(".exe") {
        low.push_str(".exe");
    }
    let stem = low.strip_suffix(".exe")?;
    if stem.is_empty()
        || !stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b' '))
    {
        return None;
    }
    Some(low)
}

const RESERVED_KEYWORDS: &[&str] = &[
    "f", "ev", "cb", "tr", "fy", "cap", "shot", "jt", "pin", "jtocr", "jtfy", "w", "kill", "ps",
    "snip", "=", "b64", "b64d",
];

/// Validates a candidate custom scan directory path.
/// Blocks drive roots (`C:\`), UNC shares (`\\host\share`), relative/traversal paths (`..`),
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
    let normalized = trimmed.replace('/', "\\");
    if normalized.split('\\').any(|seg| seg == "..") {
        return Err("路径不允许包含上级跳转 (..)");
    }

    // Must be a Windows absolute drive path (`X:\...`) on Windows or `/` path on Unix.
    let bytes = normalized.as_bytes();
    let is_win_abs = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes[2] == b'\\';
    let is_unix_abs = normalized.starts_with('/');
    if !is_win_abs && !is_unix_abs {
        return Err("请通过浏览框选择完整绝对目录路径");
    }

    let clean = normalized.trim_end_matches('\\').to_string();
    // Reject bare drive root (`C:`, `D:`) or Unix `/`
    if clean.len() <= 2 || clean == "/" {
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

/// Validates a GitHub repository slug (`owner/repo`).
pub fn is_valid_github_repo(repo: &str) -> bool {
    let trimmed = repo.trim();
    let Some((owner, name)) = trimmed.split_once('/') else {
        return false;
    };
    if owner.is_empty()
        || name.is_empty()
        || owner.len() > 64
        || name.len() > 100
        || name.contains('/')
        || owner.contains("..")
        || name.contains("..")
    {
        return false;
    }
    let valid_part = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            && !s.starts_with('.')
            && !s.ends_with('.')
    };
    valid_part(owner) && valid_part(name)
}

/// Strict HTTP(S) URL validator (blocks `file://`, `cmd:`, `javascript:`, `.exe` paths).
pub fn is_safe_http_url(url: &str) -> bool {
    let t = url.trim();
    if t.len() < 10 || t.len() > 512 || t.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let low = t.to_ascii_lowercase();
    let rest = if let Some(r) = low.strip_prefix("https://") {
        r
    } else if let Some(r) = low.strip_prefix("http://") {
        r
    } else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    !host.is_empty() && (host.contains('.') || host.starts_with("localhost"))
}

/// Validates a web search engine entry (`keyword`, `name`, `https?://...{q}...`).
pub fn validate_web_search(engine: &WebSearchEngine) -> Result<WebSearchEngine, &'static str> {
    let kw = engine.keyword.trim().to_ascii_lowercase();
    if kw.is_empty()
        || kw.len() > 16
        || !kw
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err("关键词仅允许 1–16 位小写字母、数字、下划线或连字符");
    }
    if RESERVED_KEYWORDS.contains(&kw.as_str()) {
        return Err("关键词与内置命令冲突");
    }
    let name: String = engine
        .name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(48)
        .collect();
    if name.is_empty() {
        return Err("搜索引擎名称不能为空");
    }
    let url = engine.url.trim().to_string();
    if !is_safe_http_url(&url) {
        return Err("URL 模板必须为合法的 https:// 或 http:// 地址");
    }
    if !url.contains("{q}") {
        return Err("URL 模板必须包含 {q} 查询占位符");
    }
    Ok(WebSearchEngine {
        keyword: kw,
        name,
        url,
    })
}

/// Sanitizes every field in `TomoyoConfig` in-place, returning a list of adjustment notices.
pub fn sanitize_config(cfg: &mut TomoyoConfig) -> Vec<String> {
    let mut notes = Vec::new();

    if !matches!(cfg.language.as_str(), "zh" | "en") {
        cfg.language = if cfg.language.to_ascii_lowercase().starts_with("en") {
            "en".into()
        } else {
            "zh".into()
        };
    }
    if !matches!(cfg.theme.as_str(), "light" | "dark" | "midnight") {
        cfg.theme = "dark".into();
        notes.push("主题已回退为默认预设".into());
    }
    if parse_hotkey(&cfg.hotkey).is_none() {
        cfg.hotkey = "Alt+Space".into();
        notes.push("快捷键格式无效，已恢复 Alt+Space".into());
    }

    let hib = cfg.hibernate_delay_secs.clamp(5, 120);
    if hib != cfg.hibernate_delay_secs {
        cfg.hibernate_delay_secs = hib;
        notes.push(format!("休眠延迟已约束至 {hib} s"));
    }

    cfg.window.width_dip = cfg.window.width_dip.clamp(520, 1280);
    cfg.window.max_visible_rows = cfg.window.max_visible_rows.clamp(4, 16);
    cfg.window.preview_width_dip = cfg.window.preview_width_dip.clamp(220, 560);
    cfg.window.custom_x = cfg.window.custom_x.map(|v| v.clamp(-16_384, 16_384));
    cfg.window.custom_y = cfg.window.custom_y.map(|v| v.clamp(-16_384, 16_384));

    let depth = cfg.apps.extra_scan_depth.clamp(1, MAX_SCAN_DEPTH);
    if depth != cfg.apps.extra_scan_depth {
        notes.push(format!(
            "自定义目录扫描深度 {} 超出安全范围，已自动修正为 {} 层",
            cfg.apps.extra_scan_depth, depth
        ));
        cfg.apps.extra_scan_depth = depth;
    }

    let mut seen_dirs = HashSet::new();
    let mut clean_dirs = Vec::new();
    for raw in std::mem::take(&mut cfg.apps.extra_dirs) {
        match validate_scan_dir(&raw) {
            Ok(p) => {
                if seen_dirs.insert(p.to_ascii_lowercase()) && clean_dirs.len() < MAX_EXTRA_DIRS {
                    clean_dirs.push(p);
                }
            }
            Err(reason) => {
                if !raw.trim().is_empty() {
                    notes.push(format!("已忽略非法扫描目录 `{}`（{reason}）", raw.trim()));
                }
            }
        }
    }
    cfg.apps.extra_dirs = clean_dirs;

    let ev_g = cfg.everything.max_global_results.clamp(0, 10);
    if ev_g != cfg.everything.max_global_results {
        cfg.everything.max_global_results = ev_g;
        notes.push(format!("Everything 全局条数已约束为 {ev_g}"));
    }
    let ev_d = cfg.everything.max_dedicated_results.clamp(5, 50);
    if ev_d != cfg.everything.max_dedicated_results {
        cfg.everything.max_dedicated_results = ev_d;
        notes.push(format!("Everything 专属模式条数已约束为 {ev_d}"));
    }
    cfg.everything.extra_filter = cfg
        .everything
        .extra_filter
        .chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\'')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_EXTRA_FILTER_CHARS)
        .collect();
    cfg.everything.favorites.truncate(MAX_FAVORITES);
    cfg.everything.favorites.retain(|f| {
        f.tag.starts_with('@')
            && f.tag.len() >= 2
            && f.tag.len() <= 18
            && !f.expr.trim().is_empty()
            && f.expr.len() <= 120
            && !f.expr.chars().any(|c| c.is_control())
    });

    let clip_max = cfg.clipboard.max_persisted_items.clamp(50, 2000);
    if clip_max != cfg.clipboard.max_persisted_items {
        cfg.clipboard.max_persisted_items = clip_max;
        notes.push(format!("剪贴板记录上限已约束为 {clip_max}"));
    }
    let clip_hk = cfg.clipboard.hotkey.trim().to_string();
    if clip_hk.is_empty() {
        cfg.clipboard.hotkey.clear();
    } else if parse_hotkey(&clip_hk).is_none() || clip_hk.eq_ignore_ascii_case(&cfg.hotkey) {
        let fallback = if cfg.hotkey.eq_ignore_ascii_case("Alt+V") {
            String::new()
        } else {
            "Alt+V".into()
        };
        notes.push(format!("剪贴板快捷键 `{clip_hk}` 无效或与主快捷键冲突，已重置"));
        cfg.clipboard.hotkey = fallback;
    } else {
        cfg.clipboard.hotkey = clip_hk;
    }
    let mut seen_apps = HashSet::new();
    let mut clean_apps = Vec::new();
    for raw in std::mem::take(&mut cfg.clipboard.excluded_apps) {
        if let Some(exe) = normalize_exe_name(&raw) {
            if seen_apps.insert(exe.clone()) && clean_apps.len() < MAX_EXCLUDED_APPS {
                clean_apps.push(exe);
            }
        }
    }
    cfg.clipboard.excluded_apps = clean_apps;
    crate::translate::sanitize_translate_config(
        &mut cfg.translate,
        &cfg.hotkey,
        &cfg.clipboard.hotkey,
        &mut notes,
    );
    super::sanitize_capture_config(
        &mut cfg.capture,
        &cfg.hotkey,
        &cfg.clipboard.hotkey,
        &cfg.translate.hotkey,
        &mut notes,
    );

    cfg.update.check_interval_hours = cfg.update.check_interval_hours.clamp(6, 720);
    if !is_valid_github_repo(&cfg.update.repo) {
        notes.push("GitHub 仓库格式非法，已恢复默认官方源".into());
        cfg.update.repo = "q871958549/Tomoyo".into();
    }

    let mut clean_regs = Vec::new();
    for reg in std::mem::take(&mut cfg.plugin_registries) {
        let t = reg.trim();
        let ok = (t == "registry/index.json")
            || (t.starts_with("https://") && is_safe_http_url(t))
            || (t.ends_with(".json") && !t.contains("..") && !t.chars().any(|c| c.is_control()));
        if ok && clean_regs.len() < 8 {
            clean_regs.push(t.to_string());
        }
    }
    if clean_regs.is_empty() {
        clean_regs = default_plugin_registries();
    }
    cfg.plugin_registries = clean_regs;

    let mut seen_kw = HashSet::new();
    let mut clean_web = Vec::new();
    for eng in std::mem::take(&mut cfg.web_searches) {
        match validate_web_search(&eng) {
            Ok(valid) => {
                if seen_kw.insert(valid.keyword.clone()) && clean_web.len() < MAX_WEB_SEARCHES {
                    clean_web.push(valid);
                }
            }
            Err(reason) => {
                notes.push(format!("已移除无效网页搜索 `{}`（{reason}）", eng.keyword.trim()));
            }
        }
    }
    if clean_web.is_empty() {
        clean_web = default_web_searches();
    }
    cfg.web_searches = clean_web;

    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_adversarial_and_out_of_range_config_inputs() {
        let mut cfg = TomoyoConfig::default();
        cfg.apps.extra_scan_depth = 12_000;
        cfg.apps.extra_dirs = vec![
            r"C:\".into(),
            r"C:\Windows\System32".into(),
            r"\\evil\share".into(),
            r"E:\Tools\..\Windows".into(),
            r"E:\Tools\".into(),
            r"e:\tools".into(),
        ];
        cfg.everything.max_global_results = 99_999;
        cfg.everything.max_dedicated_results = 99_999;
        cfg.clipboard.max_persisted_items = 9_999_999;
        cfg.clipboard.hotkey = "Alt+Space".into();
        cfg.clipboard.excluded_apps = vec![
            "KeePass".into(),
            "keepass.exe".into(),
            "..\\evil.exe".into(),
            "Bitwarden.EXE".into(),
        ];
        cfg.window.width_dip = 99_999;
        cfg.capture.hotkey = "Alt+T".into();
        cfg.capture.pin_opacity = 5;
        cfg.capture.save_dir = r"\\evil\share".into();
        cfg.update.repo = "https://evil.com/../../etc/passwd".into();
        cfg.web_searches.push(WebSearchEngine {
            keyword: "bad".into(),
            name: "Exploit".into(),
            url: "file:///C:/Windows/System32/cmd.exe?{q}".into(),
        });
        cfg.web_searches.push(WebSearchEngine {
            keyword: "f".into(),
            name: "Reserved Conflict".into(),
            url: "https://example.com/?q={q}".into(),
        });

        let notes = sanitize_config(&mut cfg);
        assert!(!notes.is_empty());
        assert_eq!(cfg.apps.extra_scan_depth, 4);
        assert_eq!(cfg.apps.extra_dirs, vec![r"E:\Tools".to_string()]);
        assert_eq!(cfg.everything.max_global_results, 10);
        assert_eq!(cfg.everything.max_dedicated_results, 50);
        assert_eq!(cfg.clipboard.max_persisted_items, 2000);
        assert_eq!(cfg.clipboard.hotkey, "Alt+V");
        assert_eq!(cfg.clipboard.excluded_apps, vec!["keepass.exe", "bitwarden.exe"]);
        assert_eq!(cfg.window.width_dip, 1280);
        assert_eq!(cfg.capture.hotkey, "Alt+S");
        assert_eq!(cfg.capture.pin_opacity, 30);
        assert!(cfg.capture.save_dir.is_empty());
        assert_eq!(cfg.update.repo, "q871958549/Tomoyo");
        assert!(!cfg.web_searches.iter().any(|w| w.keyword == "bad" || w.keyword == "f"));
    }
}
