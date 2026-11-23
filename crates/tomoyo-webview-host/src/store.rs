//! Decentralized SHA-256 verified In-App Plugin Store operations (fetch, install, uninstall).

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Loads plugin registry entries via scalable catalog index (page 1 default).
pub fn fetch_store_catalog_json(sources: &[String]) -> String {
    crate::store_index::query_store_page_json(&crate::store_index::StoreQuery {
        sources: sources.to_vec(),
        page: 1,
        page_size: 24,
        ..Default::default()
    })
}

pub fn install_plugin_from_source(
    plugin_id: &str,
    source: &str,
    expected_sha256: &str,
) -> Result<String, String> {
    if plugin_id.is_empty() || plugin_id.contains("..") || plugin_id.contains(['/', '\\']) {
        return Err("无效的插件 ID".into());
    }
    let slug = plugin_id.replace('.', "-");
    let target_dir = tomoyo_core::data_dir().join("plugins").join(slug);

    if source.starts_with("https://") || source.starts_with("http://") {
        tomoyo_core::download_and_install_remote_zip(source, expected_sha256, true, &target_dir)?;
        return Ok(target_dir.to_string_lossy().into_owned());
    }

    let src_dir = candidate_local_paths(source)
        .into_iter()
        .find(|p| p.is_dir())
        .ok_or_else(|| format!("未在本地找到插件源码目录 `{source}`（可使用 tomoyo-cli install 安装远程包）"))?;

    let actual_sha = compute_plugin_sha256(&src_dir).map_err(|e| format!("SHA-256 计算失败: {e}"))?;
    if !expected_sha256.is_empty() && !expected_sha256.eq_ignore_ascii_case(&actual_sha) {
        return Err(format!(
            "安全阻断：SHA-256 不匹配！期望 {}，实际 {}",
            &expected_sha256[..expected_sha256.len().min(12)],
            &actual_sha[..12]
        ));
    }

    copy_plugin_files(&src_dir, &target_dir).map_err(|e| format!("复制文件失败: {e}"))?;
    Ok(target_dir.to_string_lossy().into_owned())
}

pub fn uninstall_user_plugin(plugin_id: &str) -> Result<(), String> {
    if plugin_id.is_empty() || plugin_id.contains("..") || plugin_id.contains(['/', '\\']) {
        return Err("无效的插件 ID".into());
    }
    let slug = plugin_id.replace('.', "-");
    let target_dir = tomoyo_core::data_dir().join("plugins").join(slug);
    if !target_dir.is_dir() {
        return Err("该插件属于随包内置示例插件，无法从用户目录卸载".into());
    }
    std::fs::remove_dir_all(&target_dir).map_err(|e| format!("删除目录失败: {e}"))?;
    Ok(())
}

pub(crate) fn candidate_local_paths(rel: &str) -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from(rel), tomoyo_core::data_dir().join(rel)];
    #[cfg(target_os = "linux")]
    {
        out.push(PathBuf::from("/usr/share/tomoyo").join(rel));
        out.push(PathBuf::from("/usr/local/share/tomoyo").join(rel));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.parent();
        for _ in 0..4 {
            if let Some(dir) = cur {
                out.push(dir.join(rel));
                cur = dir.parent();
            } else {
                break;
            }
        }
    }
    out
}

fn compute_plugin_sha256(dir: &Path) -> Result<String, std::io::Error> {
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

fn collect_plugin_files(
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
            "target" | "node_modules" | "__pycache__" | ".git" | "Cargo.lock"
        ) {
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

fn copy_plugin_files(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
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
