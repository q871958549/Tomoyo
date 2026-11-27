//! Cross-machine authenticated encrypted credential vault (`tomoyo-cli secrets list|export|import`).
//!
//! Uses pure-Rust PBKDF2-HMAC-SHA256 (200,000 iterations) + Encrypt-then-MAC (HMAC-SHA256-CTR +
//! constant-time HMAC-SHA256 tag) with zero extra crate dependencies.

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tomoyo_core::{
    data_dir, get_secret, is_valid_secret_key, list_stored_secret_keys, plugin_secret_key,
    set_secret,
};
use tomoyo_plugin_host::discovery::scan_plugin_dirs;
use uuid::Uuid;

const VAULT_DOMAIN: &[u8] = b"TOMOYO-VAULT-V1";
const PBKDF2_ITERS: u32 = 200_000;
const MIN_PASSPHRASE_CHARS: usize = 8;

#[derive(Debug, Serialize, Deserialize)]
struct VaultEnvelope {
    version: u32,
    kdf: String,
    iterations: u32,
    salt_b64: String,
    nonce_b64: String,
    ciphertext_b64: String,
    mac_b64: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VaultPayload {
    exported_at_unix: i64,
    secrets: BTreeMap<String, String>,
}

pub fn cmd_secrets(args: &[String]) -> i32 {
    let Some(sub) = args.first().map(String::as_str) else {
        print_usage();
        return 1;
    };
    match sub {
        "list" => {
            let keys = collect_known_secret_keys();
            if keys.is_empty() {
                println!("No stored secrets found.");
            } else {
                println!("Stored secret keys ({}):", keys.len());
                for k in keys {
                    println!("  - {k}");
                }
            }
            0
        }
        "export" => cmd_export(&args[1..]),
        "import" => cmd_import(&args[1..]),
        _ => {
            print_usage();
            1
        }
    }
}

fn print_usage() {
    eprintln!(
        "Usage:\n  tomoyo-cli secrets list\n  tomoyo-cli secrets export --out <vault.json> [--passphrase <pw>]\n  tomoyo-cli secrets import --in <vault.json> [--passphrase <pw>] [--dry-run]"
    );
}

fn resolve_passphrase(args: &[String]) -> Result<String, String> {
    let pw = crate::flag_val(args, "--passphrase")
        .map(ToString::to_string)
        .or_else(|| std::env::var("TOMOYO_VAULT_PASSPHRASE").ok())
        .ok_or_else(|| "Missing --passphrase (or TOMOYO_VAULT_PASSPHRASE env var)".to_string())?;
    if pw.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(format!("Passphrase must be at least {MIN_PASSPHRASE_CHARS} characters"));
    }
    Ok(pw)
}

fn collect_known_secret_keys() -> Vec<String> {
    let mut extra = Vec::new();
    let plugin_dirs = [PathBuf::from("plugins"), data_dir().join("plugins")];
    for pl in scan_plugin_dirs(&plugin_dirs) {
        for field in &pl.manifest.settings {
            if matches!(field.kind, tomoyo_protocol::SettingFieldKind::Secret { .. })
                && !field.key.trim().is_empty()
            {
                extra.push(plugin_secret_key(&pl.manifest.id, &field.key));
            }
        }
    }
    list_stored_secret_keys(&extra)
}

fn cmd_export(args: &[String]) -> i32 {
    let Some(out_path) = crate::flag_val(args, "--out").map(PathBuf::from) else {
        eprintln!("Missing --out <vault.json>");
        return 1;
    };
    let pw = match resolve_passphrase(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    let mut map = BTreeMap::new();
    for key in collect_known_secret_keys() {
        if let Some(val) = get_secret(&key) {
            map.insert(key, val);
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let payload = VaultPayload { exported_at_unix: now, secrets: map };
    let Ok(plain) = serde_json::to_vec(&payload) else {
        eprintln!("Failed to serialize secret payload");
        return 1;
    };
    let env = seal_vault(&plain, &pw, PBKDF2_ITERS);
    if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(json) = serde_json::to_string_pretty(&env) else { return 1; };
    if let Err(e) = std::fs::write(&out_path, json) {
        eprintln!("Failed to write {}: {e}", out_path.display());
        return 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(0o600));
    }
    println!("[OK] Exported {} encrypted credential(s) -> {}", payload.secrets.len(), out_path.display());
    0
}

fn cmd_import(args: &[String]) -> i32 {
    let Some(in_path) = crate::flag_val(args, "--in").map(PathBuf::from) else {
        eprintln!("Missing --in <vault.json>");
        return 1;
    };
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let pw = match resolve_passphrase(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    let raw = match std::fs::read_to_string(&in_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to read {}: {e}", in_path.display());
            return 1;
        }
    };
    let env: VaultEnvelope = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Invalid vault file: {e}");
            return 1;
        }
    };
    let plain = match open_vault(&env, &pw) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Vault decryption failed: {e}");
            return 1;
        }
    };
    let payload: VaultPayload = match serde_json::from_slice(&plain) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Corrupt vault payload JSON: {e}");
            return 1;
        }
    };
    let mut imported = 0usize;
    for (k, v) in &payload.secrets {
        if !is_valid_secret_key(k) {
            eprintln!("Skipped invalid key: {k}");
            continue;
        }
        if dry_run {
            println!("  [dry-run] {k}");
            imported += 1;
        } else if let Err(e) = set_secret(k, v) {
            eprintln!("Failed storing `{k}`: {e}");
            return 1;
        } else {
            imported += 1;
        }
    }
    println!(
        "[OK] {} {} credential(s) from {}",
        if dry_run { "Verified" } else { "Imported" },
        imported,
        in_path.display()
    );
    0
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k_pad = [0u8; 64];
    if key.len() > 64 {
        let digest = Sha256::digest(key);
        k_pad[..32].copy_from_slice(&digest);
    } else {
        k_pad[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k_pad[i];
        opad[i] ^= k_pad[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

pub(crate) fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, dk: &mut [u8]) {
    let mut block_idx = 1u32;
    let mut written = 0usize;
    while written < dk.len() {
        let mut msg = Vec::with_capacity(salt.len() + 4);
        msg.extend_from_slice(salt);
        msg.extend_from_slice(&block_idx.to_be_bytes());
        let mut u = hmac_sha256(password, &msg);
        let mut t = u;
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for (dst, src) in t.iter_mut().zip(u.iter()) {
                *dst ^= *src;
            }
        }
        let take = (dk.len() - written).min(32);
        dk[written..written + take].copy_from_slice(&t[..take]);
        written += take;
        block_idx += 1;
    }
}

fn derive_subkeys(passphrase: &str, salt: &[u8], iters: u32) -> ([u8; 32], [u8; 32]) {
    let mut dk = [0u8; 32];
    pbkdf2_hmac_sha256(passphrase.as_bytes(), salt, iters, &mut dk);
    let enc_key = hmac_sha256(&dk, b"tomoyo-vault-enc-v1");
    let mac_key = hmac_sha256(&dk, b"tomoyo-vault-mac-v1");
    (enc_key, mac_key)
}

fn xor_keystream(enc_key: &[u8; 32], nonce: &[u8], input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    for (ctr, chunk) in input.chunks(32).enumerate() {
        let mut block_in = Vec::with_capacity(nonce.len() + 4);
        block_in.extend_from_slice(nonce);
        block_in.extend_from_slice(&(ctr as u32).to_be_bytes());
        let ks = hmac_sha256(enc_key, &block_in);
        for (a, b) in chunk.iter().zip(ks.iter()) {
            out.push(a ^ b);
        }
    }
    out
}

fn compute_mac(mac_key: &[u8; 32], iters: u32, salt: &[u8], nonce: &[u8], ct: &[u8]) -> [u8; 32] {
    let mut msg = Vec::with_capacity(VAULT_DOMAIN.len() + 4 + salt.len() + nonce.len() + ct.len());
    msg.extend_from_slice(VAULT_DOMAIN);
    msg.extend_from_slice(&iters.to_be_bytes());
    msg.extend_from_slice(salt);
    msg.extend_from_slice(nonce);
    msg.extend_from_slice(ct);
    hmac_sha256(mac_key, &msg)
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn seal_vault(plain: &[u8], passphrase: &str, iters: u32) -> VaultEnvelope {
    let b64 = base64::engine::general_purpose::STANDARD;
    let salt = *Uuid::new_v4().as_bytes();
    let nonce = *Uuid::new_v4().as_bytes();
    let (enc_key, mac_key) = derive_subkeys(passphrase, &salt, iters);
    let ct = xor_keystream(&enc_key, &nonce, plain);
    let mac = compute_mac(&mac_key, iters, &salt, &nonce, &ct);
    VaultEnvelope {
        version: 1,
        kdf: "pbkdf2-hmac-sha256".into(),
        iterations: iters,
        salt_b64: b64.encode(salt),
        nonce_b64: b64.encode(nonce),
        ciphertext_b64: b64.encode(ct),
        mac_b64: b64.encode(mac),
    }
}

fn open_vault(env: &VaultEnvelope, passphrase: &str) -> Result<Vec<u8>, String> {
    if env.version != 1 || env.iterations < 10_000 {
        return Err("Unsupported vault version or weak KDF iteration count".into());
    }
    let b64 = base64::engine::general_purpose::STANDARD;
    let salt = b64.decode(&env.salt_b64).map_err(|_| "Bad salt")?;
    let nonce = b64.decode(&env.nonce_b64).map_err(|_| "Bad nonce")?;
    let ct = b64.decode(&env.ciphertext_b64).map_err(|_| "Bad ciphertext")?;
    let mac = b64.decode(&env.mac_b64).map_err(|_| "Bad MAC")?;
    let (enc_key, mac_key) = derive_subkeys(passphrase, &salt, env.iterations);
    let expected = compute_mac(&mac_key, env.iterations, &salt, &nonce, &ct);
    if !ct_eq(&mac, &expected) {
        return Err("Authentication failed (wrong passphrase or tampered vault)".into());
    }
    Ok(xor_keystream(&enc_key, &nonce, &ct))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_hmac_sha256_matches_rfc6070_vector() {
        let mut dk = [0u8; 32];
        pbkdf2_hmac_sha256(b"password", b"salt", 2, &mut dk);
        let hex: String = dk.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43");
    }

    #[test]
    fn authenticated_vault_roundtrips_and_rejects_wrong_passphrase_or_tamper() {
        let raw = br#"{"exported_at_unix":1,"secrets":{"plugin:dev.tomoyo.translate:deepl_api_key":"sk-123"}}"#;
        let mut env = seal_vault(raw, "correct-horse-battery", 10_000);
        assert_eq!(open_vault(&env, "correct-horse-battery").unwrap(), raw);
        assert!(open_vault(&env, "wrong-passphrase").is_err());

        let mut ct = base64::engine::general_purpose::STANDARD.decode(&env.ciphertext_b64).unwrap();
        ct[0] ^= 0xff;
        env.ciphertext_b64 = base64::engine::general_purpose::STANDARD.encode(&ct);
        assert!(open_vault(&env, "correct-horse-battery").is_err());
    }
}
