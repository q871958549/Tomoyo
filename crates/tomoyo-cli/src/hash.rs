//! Deterministic plugin bundle SHA-256 hashing & safe file copy helpers.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Deterministic SHA-256 over all non-build plugin files (sorted relative paths + bytes).
pub fn compute_plugin_sha256(dir: &Path) -> Result<String, std::io::Error> {
    let mut files = BTreeMap::<String, PathBuf>::new();
    collect_plugin_files(dir, dir, &mut files)?;

    let mut hasher = Sha256::new();
    for (rel, path) in files {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        let data = std::fs::read(&path)?;
        hasher.update((data.len() as u64).to_le_bytes());
        hasher.update(&data);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn collect_plugin_files(
    root: &Path,
    cur: &Path,
    out: &mut BTreeMap<String, PathBuf>,
) -> Result<(), std::io::Error> {
    for ent in std::fs::read_dir(cur)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name();
        let name_str = name.to_string_lossy();
        if matches!(
            name_str.as_ref(),
            "target" | "node_modules" | "__pycache__" | ".git" | "Cargo.lock" | "dist"
        ) || name_str.to_ascii_lowercase().ends_with(".zip") {
            continue;
        }
        if path.is_dir() {
            collect_plugin_files(root, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(rel, path);
        }
    }
    Ok(())
}

pub fn copy_plugin_files(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    let mut files = BTreeMap::new();
    collect_plugin_files(src, src, &mut files)?;
    std::fs::create_dir_all(dst)?;
    for (rel, path) in files {
        let target = dst.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&path, &target)?;
    }
    Ok(())
}
