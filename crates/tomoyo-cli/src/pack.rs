//! Reproducible `.zip` bundle packager + SHA-256 + `registry/index.json` snippet generator (`tomoyo-cli pack`).

use crate::flag_val;
use crate::hash::{collect_plugin_files, compute_plugin_sha256};
use crate::verify::validate_plugin_dir;
use crc32fast::Hasher as Crc32;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use tomoyo_protocol::PluginManifest;

pub(crate) fn cmd_pack(args: &[String]) -> i32 {
    let Some(dir_str) = args.first().filter(|s| !s.starts_with('-')) else {
        eprintln!("Usage: tomoyo-cli pack <plugin-dir> [--out <bundle.zip>]");
        return 1;
    };
    let (canon, manifest, warnings) = match validate_plugin_dir(Path::new(dir_str)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            return 1;
        }
    };
    for w in warnings {
        println!("[WARN] {w}");
    }

    let slug = manifest
        .id
        .rsplit('.')
        .next()
        .unwrap_or(&manifest.id)
        .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "-");
    let out_path = flag_val(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| canon.join("dist").join(format!("{slug}-v{}.zip", manifest.version)));

    match pack_plugin_dir(&canon, &manifest, &out_path) {
        Ok(res) => {
            println!(
                "[PACK] Packaged `{}` v{} ({} files, {} bytes) -> {}",
                manifest.name,
                manifest.version,
                res.file_count,
                res.zip_bytes,
                res.out_path.display()
            );
            println!("[SHA256] archive (zip) = {}", res.zip_sha256);
            println!("[SHA256] bundle (dir)  = {}", res.dir_sha256);
            println!("\nInstall locally via:\n  tomoyo-cli install {} --sha256 {}", res.out_path.display(), res.zip_sha256);

            let perms: Vec<String> = manifest
                .permissions
                .iter()
                .filter_map(|p| serde_json::to_value(p).ok()?.as_str().map(ToString::to_string))
                .collect();
            let archive_url = format!(
                "https://github.com/YOUR_ORG/YOUR_REPO/releases/download/v{}/{}",
                manifest.version,
                res.out_path.file_name().unwrap_or_default().to_string_lossy()
            );
            let plat_key = match &manifest.runtime {
                tomoyo_protocol::RuntimeConfig::Wasm { .. }
                | tomoyo_protocol::RuntimeConfig::Webview { .. } => "any",
                tomoyo_protocol::RuntimeConfig::StdioJsonrpc { command, .. }
                    if matches!(command.as_str(), "python" | "python3" | "node" | "pwsh" | "bash") =>
                {
                    "any"
                }
                _ => tomoyo_protocol::current_platform_key(),
            };
            let entry_json = serde_json::json!({
                "id": manifest.id,
                "name": manifest.name,
                "version": manifest.version,
                "description": manifest.description,
                "author": if manifest.author.is_empty() { "Community" } else { &manifest.author },
                "license": if manifest.license.is_empty() { "MIT" } else { &manifest.license },
                "runtime_kind": match manifest.runtime {
                    tomoyo_protocol::RuntimeConfig::StdioJsonrpc { .. } => "stdio_jsonrpc",
                    tomoyo_protocol::RuntimeConfig::Wasm { .. } => "wasm",
                    tomoyo_protocol::RuntimeConfig::Webview { .. } => "webview",
                },
                "category": manifest.category.as_deref().unwrap_or("productivity"),
                "display_mode": manifest.display_mode.as_str(),
                "icon": manifest.icon.as_deref().unwrap_or("icon.png"),
                "tags": manifest.tags,
                "keywords": manifest.triggers.keywords,
                "permissions": perms,
                "homepage": manifest.homepage,
                "platforms": {
                    plat_key: {
                        "source": archive_url,
                        "sha256": res.zip_sha256,
                    }
                },
                "source": archive_url,
                "sha256": res.zip_sha256,
            });
            println!(
                "\nReady-to-paste `registry/index.json` snippet (use archive sha256 for HTTPS `.zip`, or bundle sha256 if vendored under `plugins/`):\n{}",
                serde_json::to_string_pretty(&entry_json).unwrap_or_default()
            );
            0
        }
        Err(e) => {
            eprintln!("[FAIL] Packaging failed: {e}");
            1
        }
    }
}

pub(crate) struct PackResult {
    pub out_path: PathBuf,
    pub zip_bytes: usize,
    pub zip_sha256: String,
    pub dir_sha256: String,
    pub file_count: usize,
}

pub(crate) fn pack_plugin_dir(
    dir: &Path,
    manifest: &PluginManifest,
    out_path: &Path,
) -> Result<PackResult, String> {
    let mut files = BTreeMap::new();
    collect_plugin_files(dir, dir, &mut files).map_err(|e| e.to_string())?;
    files.retain(|rel, _| !rel.starts_with("dist/") && !rel.to_ascii_lowercase().ends_with(".zip"));
    if files.is_empty() {
        return Err("No packable files found in plugin directory".into());
    }

    let root_folder = manifest
        .id
        .rsplit('.')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("plugin")
        .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "-");

    let mut zip_buf = Vec::new();
    let mut central = Vec::new();
    let file_count = files.len();

    for (rel, abs) in &files {
        let raw = std::fs::read(abs).map_err(|e| format!("read {}: {e}", abs.display()))?;
        let zip_name = format!("{root_folder}/{rel}");
        write_zip_entry(&mut zip_buf, &mut central, &zip_name, &raw)?;
    }
    finish_zip(&mut zip_buf, &central, file_count as u16);

    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out_path, &zip_buf).map_err(|e| e.to_string())?;

    let mut hasher = Sha256::new();
    hasher.update(&zip_buf);
    let zip_sha256: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let dir_sha256 = compute_plugin_sha256(dir).map_err(|e| e.to_string())?;

    Ok(PackResult {
        out_path: out_path.to_path_buf(),
        zip_bytes: zip_buf.len(),
        zip_sha256,
        dir_sha256,
        file_count,
    })
}

fn write_zip_entry(
    out: &mut Vec<u8>,
    central: &mut Vec<u8>,
    name: &str,
    data: &[u8],
) -> Result<(), String> {
    let mut crc = Crc32::new();
    crc.update(data);
    let crc32 = crc.finalize();

    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(data).map_err(|e| e.to_string())?;
    let deflated = enc.finish().map_err(|e| e.to_string())?;
    let (method, payload): (u16, &[u8]) = if deflated.len() < data.len() {
        (8, &deflated)
    } else {
        (0, data)
    };

    let local_off = out.len() as u32;
    let name_bytes = name.as_bytes();
    let nlen = name_bytes.len() as u16;
    let csize = payload.len() as u32;
    let usize_val = data.len() as u32;

    // Local file header (UTF-8 flag 0x0800)
    out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0x0800u16.to_le_bytes());
    out.extend_from_slice(&method.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0x5421u16.to_le_bytes());
    out.extend_from_slice(&crc32.to_le_bytes());
    out.extend_from_slice(&csize.to_le_bytes());
    out.extend_from_slice(&usize_val.to_le_bytes());
    out.extend_from_slice(&nlen.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name_bytes);
    out.extend_from_slice(payload);

    // Central directory header
    central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    central.extend_from_slice(&20u16.to_le_bytes());
    central.extend_from_slice(&20u16.to_le_bytes());
    central.extend_from_slice(&0x0800u16.to_le_bytes());
    central.extend_from_slice(&method.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0x5421u16.to_le_bytes());
    central.extend_from_slice(&crc32.to_le_bytes());
    central.extend_from_slice(&csize.to_le_bytes());
    central.extend_from_slice(&usize_val.to_le_bytes());
    central.extend_from_slice(&nlen.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u32.to_le_bytes());
    central.extend_from_slice(&local_off.to_le_bytes());
    central.extend_from_slice(name_bytes);
    Ok(())
}

fn finish_zip(out: &mut Vec<u8>, central: &[u8], count: u16) {
    let cd_off = out.len() as u32;
    let cd_sz = central.len() as u32;
    out.extend_from_slice(central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_sz.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_plugin_zip_unpacks_and_verifies_via_pkg_zip() {
        let base = std::env::temp_dir().join(format!("tomoyo_pack_test_{}", std::process::id()));
        let src = base.join("src");
        let dst = base.join("unpacked");
        let zip_path = base.join("bundle.zip");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&src).unwrap();

        let toml_src = "id=\"dev.test.packdemo\"\nname=\"Pack Demo\"\nversion=\"1.2.3\"\n[runtime]\nkind=\"webview\"\nentry=\"index.html\"\n[triggers]\nkeywords=[\"pack\"]\n";
        std::fs::write(src.join("tomoyo-plugin.toml"), toml_src).unwrap();
        std::fs::write(src.join("index.html"), "<html><body>Pack Test 中文</body></html>").unwrap();

        let manifest = PluginManifest::from_toml_str(toml_src).unwrap();
        let res = pack_plugin_dir(&src, &manifest, &zip_path).unwrap();
        let verified_sha =
            tomoyo_core::install_zip_file_verified(&zip_path, &res.zip_sha256, &dst).unwrap();
        assert_eq!(verified_sha, res.zip_sha256);
        assert_eq!(compute_plugin_sha256(&dst).unwrap(), res.dir_sha256);

        let _ = std::fs::remove_dir_all(&base);
    }
}
