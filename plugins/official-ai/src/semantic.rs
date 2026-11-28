//! Offline incremental BM25 + CJK bigram semantic search across local notes, snippets & docs.

use crate::mcp_client::{sync_root_dir, tomoyo_data_dir};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const MAX_FILES: usize = 300;
const MAX_FILE_BYTES: u64 = 96 * 1024;
const SKIP_DIRS: &[&str] = &[
    ".git", ".archive", "target", "node_modules", "dist", "build", "out", "coverage", ".venv",
    "venv", "__pycache__",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedDoc {
    #[serde(default)]
    v: u8,
    path: String,
    title: String,
    modified_secs: u64,
    preview: String,
    tf: HashMap<String, u32>,
    len: usize,
}

#[derive(Debug, Clone)]
pub struct SemanticHit {
    pub path: String,
    pub title: String,
    pub preview: String,
    pub score: f64,
}

pub fn search_local_knowledge(query: &str, extra_dirs: &[String], limit: usize) -> Vec<SemanticHit> {
    let roots = collect_roots(extra_dirs);
    search_at_roots(query, &roots, &tomoyo_data_dir().join("cache/semantic_index.json"), limit)
}

pub fn search_at_roots(query: &str, roots: &[PathBuf], cache_path: &Path, limit: usize) -> Vec<SemanticHit> {
    let q_terms = tokenize(query);
    if q_terms.is_empty() { return Vec::new(); }

    let prev_map = load_cache(cache_path);
    let mut files = Vec::new();
    for root in roots {
        collect_files(root, 0, &mut files);
        if files.len() >= MAX_FILES { break; }
    }

    let mut docs = Vec::with_capacity(files.len());
    for path in files.into_iter().take(MAX_FILES) {
        let p_str = path.to_string_lossy().into_owned();
        let Ok(meta) = std::fs::metadata(&path) else { continue; };
        if meta.len() > MAX_FILE_BYTES { continue; }
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        if let Some(hit) = prev_map.get(&p_str).filter(|d| d.modified_secs == mtime && d.v >= 2) {
            docs.push(hit.clone());
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else { continue; };
        docs.push(index_text(&p_str, &path, mtime, &raw));
    }
    save_cache(cache_path, &docs);

    if docs.is_empty() { return Vec::new(); }
    let avg_dl = (docs.iter().map(|d| d.len).sum::<usize>() as f64 / docs.len() as f64).max(1.0);
    let n_docs = docs.len() as f64;
    let unique_q: HashSet<String> = q_terms.into_iter().collect();

    let mut df: HashMap<&str, usize> = HashMap::new();
    for term in &unique_q {
        let count = docs.iter().filter(|d| d.tf.contains_key(term.as_str())).count();
        df.insert(term.as_str(), count);
    }

    let k1 = 1.5;
    let b = 0.75;
    let mut scored: Vec<SemanticHit> = docs
        .into_iter()
        .filter_map(|doc| {
            let mut score = 0.0;
            for term in &unique_q {
                let tf = *doc.tf.get(term.as_str()).unwrap_or(&0) as f64;
                if tf == 0.0 { continue; }
                let n_q = *df.get(term.as_str()).unwrap_or(&0) as f64;
                let idf = ((n_docs - n_q + 0.5) / (n_q + 0.5) + 1.0).ln();
                let denom = tf + k1 * (1.0 - b + b * (doc.len as f64 / avg_dl));
                score += idf * (tf * (k1 + 1.0) / denom);
            }
            if score > 0.0 {
                Some(SemanticHit { path: doc.path, title: doc.title, preview: doc.preview, score })
            } else {
                None
            }
        })
        .collect();
    scored.sort_by(|a, b| b.score.total_cmp(&a.score));
    scored.truncate(limit.max(1));
    scored
}

fn collect_roots(extra_dirs: &[String]) -> Vec<PathBuf> {
    let root = sync_root_dir();
    let mut out = vec![root.join("notes"), root.join("snippets.toml")];
    for d in extra_dirs {
        let p = PathBuf::from(d);
        if (p.is_dir() || p.is_file()) && !out.contains(&p) { out.push(p); }
    }
    out
}

fn is_repo_root(dir: &Path) -> bool {
    dir.join(".git").exists() || dir.join("Cargo.toml").is_file() || dir.join("package.json").is_file()
}

fn push_doc_file(path: PathBuf, out: &mut Vec<PathBuf>) {
    if out.len() < MAX_FILES && is_text_doc(&path) && !out.contains(&path) {
        out.push(path);
    }
}

fn collect_files(cur: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if out.len() >= MAX_FILES || depth > 5 { return; }
    if cur.is_file() {
        push_doc_file(cur.to_path_buf(), out);
        return;
    }
    let Ok(rd) = std::fs::read_dir(cur) else { return; };
    let mut dirs = Vec::new();
    let mut docs_dir = None;
    let repo_root = is_repo_root(cur);
    let mut deferred_files = Vec::new();
    for ent in rd.flatten() {
        if out.len() >= MAX_FILES { break; }
        let path = ent.path();
        let name_s = ent.file_name().to_string_lossy().into_owned();
        if SKIP_DIRS.iter().any(|&s| s.eq_ignore_ascii_case(&name_s)) { continue; }
        if path.is_dir() {
            if repo_root && name_s.eq_ignore_ascii_case("docs") { docs_dir = Some(path); }
            else { dirs.push(path); }
        } else if is_text_doc(&path) {
            let low = name_s.to_ascii_lowercase();
            if repo_root && (low.starts_with("readme") || low == "changelog.md" || low == "architecture.md") {
                push_doc_file(path, out);
            } else {
                deferred_files.push(path);
            }
        }
    }
    if let Some(d) = docs_dir { collect_files(&d, depth + 1, out); }
    for f in deferred_files { push_doc_file(f, out); }
    for d in dirs {
        if out.len() >= MAX_FILES { break; }
        collect_files(&d, depth + 1, out);
    }
}

fn is_text_doc(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "txt" | "toml" | "rs" | "ts" | "py"))
}

fn add_weighted_tokens(tf: &mut HashMap<String, u32>, total_len: &mut usize, text: &str, weight: u32) {
    let toks = tokenize(text);
    *total_len += toks.len() * weight as usize;
    for tok in toks {
        *tf.entry(tok).or_insert(0) += weight;
    }
}

fn index_text(p_str: &str, path: &Path, modified_secs: u64, raw: &str) -> CachedDoc {
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("doc");
    let clean_body = raw.trim();
    let title = clean_body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("---"))
        .map(|l| l.trim_start_matches('#').trim())
        .filter(|l| !l.is_empty())
        .unwrap_or(fname)
        .chars()
        .take(64)
        .collect::<String>();
    let preview: String = clean_body.replace(['\r', '\n'], " ").chars().take(180).collect();
    let mut tf = HashMap::new();
    let mut total_len = 0usize;
    add_weighted_tokens(&mut tf, &mut total_len, &title, 3);
    for line in clean_body.lines() {
        let tr = line.trim_start();
        let w = if tr.starts_with("### ") { 2 }
            else if tr.starts_with("## ") || tr.starts_with("# ") { 3 }
            else { 1 };
        add_weighted_tokens(&mut tf, &mut total_len, line.trim_start_matches('#'), w);
    }
    CachedDoc { v: 2, path: p_str.into(), title, modified_secs, preview, tf, len: total_len.max(1) }
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut ascii_buf = String::new();
    let mut cjk_buf: Vec<char> = Vec::new();

    let flush_ascii = |buf: &mut String, out: &mut Vec<String>| {
        if buf.len() >= 2 { out.push(buf.clone()); }
        buf.clear();
    };
    let flush_cjk = |buf: &mut Vec<char>, out: &mut Vec<String>| {
        for &c in buf.iter() { out.push(c.to_string()); }
        for w in buf.windows(2) { out.push(w.iter().collect()); }
        buf.clear();
    };

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            flush_cjk(&mut cjk_buf, &mut out);
            ascii_buf.push(ch.to_ascii_lowercase());
        } else if ('\u{4E00}'..='\u{9FFF}').contains(&ch) {
            flush_ascii(&mut ascii_buf, &mut out);
            cjk_buf.push(ch);
        } else {
            flush_ascii(&mut ascii_buf, &mut out);
            flush_cjk(&mut cjk_buf, &mut out);
        }
    }
    flush_ascii(&mut ascii_buf, &mut out);
    flush_cjk(&mut cjk_buf, &mut out);
    out
}

fn load_cache(path: &Path) -> HashMap<String, CachedDoc> {
    let Ok(raw) = std::fs::read_to_string(path) else { return HashMap::new(); };
    let Ok(list) = serde_json::from_str::<Vec<CachedDoc>>(&raw) else { return HashMap::new(); };
    list.into_iter().map(|d| (d.path.clone(), d)).collect()
}

fn save_cache(path: &Path, docs: &[CachedDoc]) {
    if let Some(p) = path.parent() { let _ = std::fs::create_dir_all(p); }
    let _ = std::fs::write(path, serde_json::to_string(docs).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_cjk_and_english_notes_with_incremental_cache() {
        let dir = std::env::temp_dir().join(format!("tomoyo_sem_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kwin.md"), "# Wayland 窗口位置记忆\n使用 Object.assign 修复 KWin 6.8 拖拽坐标。\n").unwrap();
        std::fs::write(dir.join("clip.md"), "# 剪贴板防竞态\n延迟 120ms 防抖读取文本。\n").unwrap();
        let cache = dir.join("cache.json");

        let hits = search_at_roots("Wayland 窗口坐标", std::slice::from_ref(&dir), &cache, 5);
        assert!(!hits.is_empty());
        assert!(hits[0].title.contains("Wayland"));
        assert!(cache.is_file());

        let hits2 = search_at_roots("Wayland 窗口坐标", std::slice::from_ref(&dir), &cache, 5);
        assert_eq!(hits2[0].path, hits[0].path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn heading_weighted_ranking_prefers_markdown_heading_over_body_mention() {
        let dir = std::env::temp_dir().join(format!("tomoyo_sem_head_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("body_only.md"), "# 通用开发日志\n这里随手提到一次 portal_shortcut 协议背景说明。\n").unwrap();
        std::fs::write(dir.join("heading_match.md"), "# 系统规范\n## portal_shortcut 热键绑定机制\n详细步骤省略。\n").unwrap();
        let cache = dir.join("cache.json");
        let hits = search_at_roots("portal_shortcut", std::slice::from_ref(&dir), &cache, 5);
        assert_eq!(hits.len(), 2);
        assert!(hits[0].path.ends_with("heading_match.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repo_root_prioritizes_readme_and_docs_while_skipping_build_dirs() {
        let dir = std::env::temp_dir().join(format!("tomoyo_sem_repo_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        std::fs::write(dir.join("README.md"), "# TomoyoRepo\nArchitecture overview for mcp_workbench.\n").unwrap();
        std::fs::write(dir.join("docs/guide.md"), "# Guide\nHow mcp_workbench works cleanly.\n").unwrap();
        std::fs::write(dir.join("dist/junk.md"), "# Ignored\nmcp_workbench compiled output\n").unwrap();
        let cache = dir.join("cache.json");
        let hits = search_at_roots("mcp_workbench", std::slice::from_ref(&dir), &cache, 10);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|h| !h.path.contains("dist")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
