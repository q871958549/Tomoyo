//! Offline incremental BM25 + CJK bigram semantic search across local notes, snippets & docs.

use crate::mcp_client::{sync_root_dir, tomoyo_data_dir};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const MAX_FILES: usize = 360;
const MAX_FILE_BYTES: u64 = 128 * 1024;
const SKIP_DIRS: &[&str] = &[".git", ".archive", "target", "node_modules", "dist", "__pycache__"];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedDoc {
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
        if let Some(hit) = prev_map.get(&p_str).filter(|d| d.modified_secs == mtime) {
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

fn collect_files(cur: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if out.len() >= MAX_FILES || depth > 5 { return; }
    if cur.is_file() {
        if is_text_doc(cur) { out.push(cur.to_path_buf()); }
        return;
    }
    let Ok(rd) = std::fs::read_dir(cur) else { return; };
    for ent in rd.flatten() {
        if out.len() >= MAX_FILES { break; }
        let path = ent.path();
        let name = ent.file_name();
        let name_s = name.to_string_lossy();
        if SKIP_DIRS.iter().any(|&s| s.eq_ignore_ascii_case(&name_s)) { continue; }
        if path.is_dir() {
            collect_files(&path, depth + 1, out);
        } else if is_text_doc(&path) {
            out.push(path);
        }
    }
}

fn is_text_doc(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "txt" | "toml" | "rs" | "ts" | "py"))
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
    let tokens = tokenize(&format!("{title}\n{clean_body}"));
    let len = tokens.len().max(1);
    let mut tf = HashMap::new();
    for tok in tokens {
        *tf.entry(tok).or_insert(0) += 1;
    }
    CachedDoc { path: p_str.into(), title, modified_secs, preview, tf, len }
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
}
