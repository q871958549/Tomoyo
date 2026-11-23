//! Filesystem & binary discovery helpers for `dev.tomoyo.markdown-editor` (`Quiet.md`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tomoyo_sdk::QueryContext;

const MAX_SCAN_DOCS: usize = 160;
const MAX_PREVIEW_BYTES: usize = 8_192;

pub struct MdDoc {
    pub path: PathBuf,
    pub title: String,
    pub snippet: String,
    pub body_preview: String,
    pub modified_secs: u64,
}

/// Locates `quiet-md` binary across custom user setting, Tomoyo tools dir, common installs, and PATH.
pub fn find_quiet_md(custom: &str) -> Option<PathBuf> {
    let custom_t = custom.trim();
    if !custom_t.is_empty() {
        let p = PathBuf::from(custom_t);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut cands = Vec::new();
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    #[cfg(windows)]
    {
        let tools = base.join("Tomoyo").join("tools").join("quiet-md");
        for exe in ["quiet-md.exe", "Quiet.md.exe", "quiet_md.exe"] {
            cands.push(tools.join(exe));
            cands.push(base.join("Programs").join("Quiet.md").join(exe));
            cands.push(base.join("Programs").join("quiet-md").join(exe));
            cands.push(base.join("Quiet.md").join(exe));
            for env_k in ["ProgramFiles", "ProgramFiles(x86)"] {
                if let Ok(pf) = std::env::var(env_k) {
                    cands.push(PathBuf::from(&pf).join("Quiet.md").join(exe));
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let tools = base.join(".local/share/tomoyo/tools/quiet-md");
        cands.push(tools.join("quiet-md.AppImage"));
        cands.push(tools.join("quiet-md"));
        cands.push(base.join(".local/bin/quiet-md"));
        cands.push(PathBuf::from("/usr/bin/quiet-md"));
        cands.push(PathBuf::from("/usr/local/bin/quiet-md"));
    }
    if let Some(hit) = cands.into_iter().find(|p| p.is_file()) {
        return Some(hit);
    }
    let path_env = std::env::var_os("PATH")?;
    let names: &[&str] = if cfg!(windows) {
        &["quiet-md.exe", "quiet_md.exe"]
    } else {
        &["quiet-md", "quiet_md"]
    };
    for dir in std::env::split_paths(&path_env) {
        for n in names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Resolves the standalone Markdown documents directory (separate from Quick Notes).
pub fn docs_dir_from(ctx: &QueryContext) -> PathBuf {
    if let Some(raw) = ctx
        .plugin_settings
        .get("docs_dir")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(raw);
    }
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let docs = PathBuf::from(home).join("Documents").join("Markdown");
        if docs.is_dir() {
            return docs;
        }
    }
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("Tomoyo").join("markdown")
}

pub fn is_md_file(p: &Path) -> bool {
    p.is_file()
        && p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown"))
}

pub fn read_md_doc(path: &Path) -> Option<MdDoc> {
    let meta = fs::metadata(path).ok()?;
    let bytes = fs::read(path).ok()?;
    let slice = &bytes[..bytes.len().min(MAX_PREVIEW_BYTES)];
    let body_preview = String::from_utf8_lossy(slice).into_owned();
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = body_preview
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().trim_start_matches('#').trim().chars().take(64).collect())
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| name.clone());
    let snippet = body_preview
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" · ")
        .chars()
        .take(96)
        .collect();
    let modified_secs = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(MdDoc {
        path: path.to_path_buf(),
        title,
        snippet,
        body_preview,
        modified_secs,
    })
}

pub fn scan_markdown_docs(dir: &Path) -> Vec<MdDoc> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for ent in rd.flatten().take(MAX_SCAN_DOCS) {
        let p = ent.path();
        if is_md_file(&p)
            && let Some(doc) = read_md_doc(&p)
        {
            out.push(doc);
        }
    }
    out.sort_by_key(|d| std::cmp::Reverse(d.modified_secs));
    out
}

pub fn create_md_file(dir: &Path, title: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let slug: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&c) { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(48)
        .collect();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let file_name = if slug.is_empty() { format!("doc-{ts}.md") } else { format!("{slug}.md") };
    let path = if dir.join(&file_name).exists() {
        dir.join(format!("{}-{ts}.md", slug.trim_end_matches(".md")))
    } else {
        dir.join(file_name)
    };
    let body = format!("# {title}\n\n");
    fs::write(&path, body).map_err(|e| format!("写入 .md 文件失败: {e}"))?;
    Ok(path)
}
