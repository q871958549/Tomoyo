//! Zero-bloat pure-Rust PKZIP parser, CRC-32 & SHA-256 verified community plugin installer.
//!
//! Uses `flate2` + `crc32fast` already compiled into the workspace dependency graph, strictly
//! enforcing path-traversal (`ZipSlip`) sanitization, size caps (<= 32 MB), and single-root
//! GitHub Release archive flattening (`tomoyo-plugin.toml`).

use crate::http::http_download_to_file;
use crate::updater::compute_file_sha256;
use flate2::read::DeflateDecoder;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_ZIP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_UNPACKED_TOTAL: usize = 96 * 1024 * 1024;
const MAX_ENTRY_COUNT: usize = 512;

fn write_entries_to_dir(entries: BTreeMap<PathBuf, Vec<u8>>, dest_dir: &Path) -> Result<(), String> {
    if dest_dir.exists() {
        let _ = std::fs::remove_dir_all(dest_dir);
    }
    std::fs::create_dir_all(dest_dir).map_err(|e| format!("创建目标目录失败: {e}"))?;
    for (rel, data) in entries {
        let target = dest_dir.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建子目录失败: {e}"))?;
        }
        std::fs::write(&target, data).map_err(|e| format!("写入解压文件失败: {e}"))?;
    }
    Ok(())
}

/// Safely unpacks a general `.zip` archive (e.g. portable self-update bundle) with ZipSlip guards.
pub fn unpack_zip_to_dir(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let bytes = std::fs::read(zip_path).map_err(|e| format!("读取压缩包失败: {e}"))?;
    let unpacked = extract_zip_entries(&bytes)?;
    write_entries_to_dir(unpacked, dest_dir)
}

/// Verifies `expected_sha256` (if non-empty) against a `.zip` file, unpacks safely, and validates
/// `tomoyo-plugin.toml` before atomically installing into `dest_dir`.
pub fn install_zip_file_verified(
    zip_path: &Path,
    expected_sha256: &str,
    dest_dir: &Path,
) -> Result<String, String> {
    let actual_sha = compute_file_sha256(zip_path)?;
    if !expected_sha256.trim().is_empty() && !expected_sha256.trim().eq_ignore_ascii_case(&actual_sha) {
        return Err(format!(
            "安全校验失败：ZIP SHA-256 不匹配（期望 {}，实际 {}）",
            &expected_sha256[..expected_sha256.len().min(12)],
            &actual_sha[..12]
        ));
    }
    let bytes = std::fs::read(zip_path).map_err(|e| format!("读取压缩包失败: {e}"))?;
    let unpacked = extract_zip_entries(&bytes)?;
    let normalized = flatten_plugin_root(unpacked)?;
    write_entries_to_dir(normalized, dest_dir)?;
    Ok(actual_sha)
}

/// Downloads a remote community plugin `.zip` URL via native WinHTTP, enforces SHA-256 verification,
/// and unpacks into `dest_dir` (`expected_sha256` is mandatory for remote packages unless explicitly allowed).
pub fn download_and_install_remote_zip(
    url: &str,
    expected_sha256: &str,
    require_sha256: bool,
    dest_dir: &Path,
) -> Result<String, String> {
    if require_sha256 && expected_sha256.trim().is_empty() {
        return Err("安全阻断：远程社区插件包必须提供 SHA-256 校验摘要".into());
    }
    let tmp_dir = std::env::temp_dir().join("tomoyo-plugin-dl");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let tmp_zip = tmp_dir.join(format!("pkg_{}.zip", std::process::id()));
    let dl_res = http_download_to_file(url, &tmp_zip, 30_000, MAX_ZIP_BYTES);
    let res = match dl_res {
        Ok(_) => install_zip_file_verified(&tmp_zip, expected_sha256, dest_dir),
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_file(&tmp_zip);
    res
}

fn sanitize_zip_rel_path(raw: &str) -> Result<Option<PathBuf>, String> {
    if raw.ends_with('/') || raw.ends_with('\\') {
        return Ok(None);
    }
    let norm = raw.replace('\\', "/");
    let mut clean = PathBuf::new();
    for comp in Path::new(&norm).components() {
        match comp {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("ZIP 含非法越权路径: `{raw}`"));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        Ok(None)
    } else {
        Ok(Some(clean))
    }
}

fn flatten_plugin_root(entries: BTreeMap<PathBuf, Vec<u8>>) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    if entries.contains_key(Path::new("tomoyo-plugin.toml")) {
        return Ok(entries);
    }
    let top_dirs: Vec<String> = entries
        .keys()
        .filter_map(|p| p.components().next())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(first) = top_dirs.first()
        && top_dirs.iter().all(|d| d == first) {
            let mut stripped = BTreeMap::new();
            for (path, data) in entries {
                if let Ok(rest) = path.strip_prefix(first)
                    && !rest.as_os_str().is_empty() {
                        stripped.insert(rest.to_path_buf(), data);
                    }
            }
            if stripped.contains_key(Path::new("tomoyo-plugin.toml")) {
                return Ok(stripped);
            }
        }
    Err("压缩包根目录缺少 `tomoyo-plugin.toml` 清单文件".into())
}

pub fn extract_zip_entries(zip_bytes: &[u8]) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let eocd = find_eocd(zip_bytes).ok_or_else(|| "无效 ZIP：未找到 Central Directory 尾标记".to_string())?;
    let entry_count = read_u16(zip_bytes, eocd + 10)? as usize;
    let cd_size = read_u32(zip_bytes, eocd + 12)? as usize;
    let cd_offset = read_u32(zip_bytes, eocd + 16)? as usize;
    if entry_count > MAX_ENTRY_COUNT {
        return Err(format!("ZIP 文件数过多 ({entry_count} > {MAX_ENTRY_COUNT})"));
    }
    if cd_offset.checked_add(cd_size).is_none_or(|end| end > eocd) {
        return Err("无效 ZIP：Central Directory 越界".into());
    }

    let mut pos = cd_offset;
    let cd_end = cd_offset + cd_size;
    let mut total_unpacked = 0usize;
    let mut out = BTreeMap::new();

    for _ in 0..entry_count {
        if pos + 46 > cd_end || read_u32(zip_bytes, pos)? != 0x0201_4b50 {
            return Err("无效 ZIP：Central Directory 条目签名错误".into());
        }
        let method = read_u16(zip_bytes, pos + 10)?;
        let expected_crc = read_u32(zip_bytes, pos + 16)?;
        let comp_size = read_u32(zip_bytes, pos + 20)? as usize;
        let uncomp_size = read_u32(zip_bytes, pos + 24)? as usize;
        let name_len = read_u16(zip_bytes, pos + 28)? as usize;
        let extra_len = read_u16(zip_bytes, pos + 30)? as usize;
        let comment_len = read_u16(zip_bytes, pos + 32)? as usize;
        let local_hdr_off = read_u32(zip_bytes, pos + 42)? as usize;

        let name_start = pos + 46;
        let next_pos = name_start + name_len + extra_len + comment_len;
        if next_pos > cd_end {
            return Err("无效 ZIP：条目元数据越界".into());
        }
        let raw_name = std::str::from_utf8(&zip_bytes[name_start..name_start + name_len])
            .map_err(|_| "无效 ZIP：文件名非 UTF-8".to_string())?;
        pos = next_pos;

        let Some(clean_rel) = sanitize_zip_rel_path(raw_name)? else {
            continue;
        };
        if total_unpacked.saturating_add(uncomp_size) > MAX_UNPACKED_TOTAL {
            return Err("安全阻断：ZIP 解压后总大小超过 48 MB 限制".into());
        }
        if local_hdr_off + 30 > cd_offset || read_u32(zip_bytes, local_hdr_off)? != 0x0403_4b50 {
            return Err(format!("无效 ZIP：本地文件头损坏 (`{raw_name}`)"));
        }
        let lh_name_len = read_u16(zip_bytes, local_hdr_off + 26)? as usize;
        let lh_extra_len = read_u16(zip_bytes, local_hdr_off + 28)? as usize;
        let data_start = local_hdr_off + 30 + lh_name_len + lh_extra_len;
        let data_end = data_start
            .checked_add(comp_size)
            .ok_or_else(|| "无效 ZIP：压缩数据偏移溢出".to_string())?;
        if data_end > cd_offset {
            return Err(format!("无效 ZIP：压缩数据越界 (`{raw_name}`)"));
        }
        let comp_slice = &zip_bytes[data_start..data_end];
        let unpacked = match method {
            0 => comp_slice.to_vec(),
            8 => {
                let mut dec = DeflateDecoder::new(comp_slice);
                let mut buf = Vec::with_capacity(uncomp_size.min(1024 * 1024));
                dec.read_to_end(&mut buf)
                    .map_err(|e| format!("Deflate 解压 `{raw_name}` 失败: {e}"))?;
                buf
            }
            other => return Err(format!("不支持的 ZIP 压缩算法 ({other})")),
        };
        if crc32fast::hash(&unpacked) != expected_crc {
            return Err(format!("ZIP CRC-32 校验失败: `{raw_name}`"));
        }
        total_unpacked = total_unpacked.saturating_add(unpacked.len());
        out.insert(clean_rel, unpacked);
    }
    Ok(out)
}

fn find_eocd(data: &[u8]) -> Option<usize> {
    if data.len() < 22 {
        return None;
    }
    let min_off = data.len().saturating_sub(22 + 65535);
    let mut i = data.len() - 22;
    loop {
        if data[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            let comment_len = u16::from_le_bytes([data[i + 20], data[i + 21]]) as usize;
            if i + 22 + comment_len == data.len() {
                return Some(i);
            }
        }
        if i == min_off {
            break;
        }
        i -= 1;
    }
    None
}

fn read_u16(b: &[u8], off: usize) -> Result<u16, String> {
    let s = b.get(off..off + 2).ok_or_else(|| "ZIP 读取越界".to_string())?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn read_u32(b: &[u8], off: usize) -> Result<u32, String> {
    let s = b.get(off..off + 4).ok_or_else(|| "ZIP 读取越界".to_string())?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn build_test_zip(items: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, data, deflate) in items {
            let lh_off = out.len() as u32;
            let crc = crc32fast::hash(data);
            let comp = if *deflate {
                let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
                enc.write_all(data).unwrap();
                enc.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let nb = name.as_bytes();
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(comp.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(nb);
            out.extend_from_slice(&comp);

            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&method.to_le_bytes());
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&crc.to_le_bytes());
            cd.extend_from_slice(&(comp.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&lh_off.to_le_bytes());
            cd.extend_from_slice(nb);
        }
        let cd_off = out.len() as u32;
        let cd_size = cd.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(items.len() as u16).to_le_bytes());
        out.extend_from_slice(&(items.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn unpacks_stored_and_deflated_archives_and_strips_single_root() {
        let manifest = b"id = \"dev.test.zip\"\nname = \"ZipTest\"\nversion = \"0.1.0\"\n";
        let zip = build_test_zip(&[
            ("my-plugin-v1/tomoyo-plugin.toml", manifest, true),
            ("my-plugin-v1/main.py", b"print('ok')\n", false),
        ]);
        let entries = flatten_plugin_root(extract_zip_entries(&zip).unwrap()).unwrap();
        assert_eq!(entries.get(Path::new("tomoyo-plugin.toml")).unwrap(), manifest);
        assert_eq!(entries.get(Path::new("main.py")).unwrap(), b"print('ok')\n");
    }

    #[test]
    fn rejects_zip_slip_traversal() {
        let zip = build_test_zip(&[("../../evil.txt", b"boom", false)]);
        let err = extract_zip_entries(&zip).unwrap_err();
        assert!(err.contains("非法越权路径"));
    }
}
