//! Native Windows DPAPI (`CryptProtectData` / `CryptUnprotectData`) local credential storage.
//!
//! API keys (DeepL, OpenAI-compatible) are encrypted using the current Windows user account
//! credentials and persisted as Base64 blobs in `%LOCALAPPDATA%\Tomoyo\secrets.json` —
//! never written in plaintext to `config.toml` or diagnostic logs.

use crate::data_dir;
use base64::Engine;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const ALLOWED_SECRET_KEYS: &[&str] = &["deepl_api_key", "openai_api_key"];

pub fn secrets_file_path() -> PathBuf {
    data_dir().join("secrets.json")
}

pub fn set_secret(key: &str, plaintext: &str) -> Result<(), String> {
    set_secret_at(&secrets_file_path(), key, plaintext)
}

pub fn get_secret(key: &str) -> Option<String> {
    get_secret_at(&secrets_file_path(), key)
}

pub fn has_secret(key: &str) -> bool {
    has_secret_at(&secrets_file_path(), key)
}

pub fn delete_secret(key: &str) -> Result<(), String> {
    delete_secret_at(&secrets_file_path(), key)
}

pub fn set_secret_at(path: &Path, key: &str, plaintext: &str) -> Result<(), String> {
    if !ALLOWED_SECRET_KEYS.contains(&key) {
        return Err("不支持的密钥字段".into());
    }
    let clean = plaintext.trim();
    if clean.is_empty() {
        return delete_secret_at(path, key);
    }
    if clean.len() > 512 || clean.chars().any(|c| c.is_control()) {
        return Err("密钥长度超限或包含非法字符".into());
    }
    let protected = dpapi_protect(clean.as_bytes())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(protected);
    let mut map = load_map(path);
    map.insert(key.to_string(), b64);
    save_map(path, &map)
}

pub fn get_secret_at(path: &Path, key: &str) -> Option<String> {
    if !ALLOWED_SECRET_KEYS.contains(&key) {
        return None;
    }
    let map = load_map(path);
    let b64 = map.get(key)?;
    let cipher = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let plain = dpapi_unprotect(&cipher).ok()?;
    String::from_utf8(plain).ok()
}

pub fn has_secret_at(path: &Path, key: &str) -> bool {
    ALLOWED_SECRET_KEYS.contains(&key) && load_map(path).contains_key(key)
}

pub fn delete_secret_at(path: &Path, key: &str) -> Result<(), String> {
    let mut map = load_map(path);
    if map.remove(key).is_some() {
        save_map(path, &map)?;
    }
    Ok(())
}

fn load_map(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_map(path: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = serde_json::to_string_pretty(map).map_err(|e| format!("serialize secrets: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write secrets: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("commit secrets: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| format!("DPAPI 加密失败: {e}"))?;
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let out = slice.to_vec();
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(output.pbData as _)));
        Ok(out)
    }
}

#[cfg(windows)]
fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: cipher.len() as u32,
        pbData: cipher.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| format!("DPAPI 解密失败: {e}"))?;
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let out = slice.to_vec();
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(output.pbData as _)));
        Ok(out)
    }
}

#[cfg(not(windows))]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plain.iter().map(|b| b ^ 0x5A).collect())
}

#[cfg(not(windows))]
fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    Ok(cipher.iter().map(|b| b ^ 0x5A).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_roundtrip_never_writes_plaintext_on_disk() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_dpapi_test_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        set_secret_at(&tmp, "deepl_api_key", "sk-secret-demo-key-98765:fx").unwrap();
        assert!(has_secret_at(&tmp, "deepl_api_key"));
        let disk = std::fs::read_to_string(&tmp).unwrap();
        assert!(!disk.contains("sk-secret-demo-key-98765"), "secret must be encrypted on disk");
        assert_eq!(get_secret_at(&tmp, "deepl_api_key").as_deref(), Some("sk-secret-demo-key-98765:fx"));
        delete_secret_at(&tmp, "deepl_api_key").unwrap();
        assert!(!has_secret_at(&tmp, "deepl_api_key"));
        let _ = std::fs::remove_file(&tmp);
    }
}
