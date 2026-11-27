//! Shared translation config schema & OS-encrypted secret vault (DPAPI / Secret Service).
//!
//! All multi-engine HTTP translation execution and history/wordbook persistence now live
//! strictly inside the standalone `dev.tomoyo.translate` Protocol v3 plugin (`plugins/official-translate`),
//! keeping Tomoyo Core 100% free of duplicated business engines.

pub mod config;
pub mod secrets;

pub use config::{
    sanitize_translate_config, TranslateConfig, ALL_TRANSLATE_ENGINES, ALLOWED_TARGET_LANGS,
};
pub use secrets::{
    delete_secret, get_secret, has_secret, is_valid_secret_key, list_stored_secret_keys,
    load_plugin_secrets, plugin_secret_key, set_secret,
};
