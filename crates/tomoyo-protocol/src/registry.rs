//! Decentralized open plugin registry schema (v2.1+) with multi-platform artifact resolution.

use crate::PluginDisplayMode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Registry shard pointer for scalable catalogs holding 10,000+ community plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryShardEntry {
    pub id: String,
    #[serde(default)]
    pub category: String,
    pub source: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub sha256: String,
}

/// Decentralized open plugin registry index (`registry/index.json`) — zero login wall, SHA-256 verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRegistryIndex {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub total_count: Option<usize>,
    #[serde(default)]
    pub shards: Vec<RegistryShardEntry>,
    #[serde(default)]
    pub plugins: Vec<RegistryPluginEntry>,
}

/// Platform-specific downloadable artifact entry for Registry Schema v2.1+.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformPackage {
    pub source: String,
    pub sha256: String,
}

/// Canonical OS + CPU architecture key used by Registry `platforms` resolution.
pub fn current_platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", "aarch64") => "macos-aarch64",
        ("macos", "x86_64") => "macos-x86_64",
        _ => "any",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPluginEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub runtime_kind: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub display_mode: PluginDisplayMode,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Legacy or universal relative path / HTTPS download URL for plugin directory/bundle.
    #[serde(default)]
    pub source: String,
    /// Required SHA-256 hex digest matching `source` (when `platforms` is absent).
    #[serde(default)]
    pub sha256: String,
    /// Multi-platform artifact map keyed by `windows-x86_64`, `linux-x86_64`, `macos-aarch64`, or `any`.
    #[serde(default)]
    pub platforms: BTreeMap<String, PlatformPackage>,
    /// Optional minimum compatible Tomoyo core version (e.g. `"2.7.0"`).
    #[serde(default)]
    pub min_tomoyo_version: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub downloads: u64,
}

impl RegistryPluginEntry {
    /// Resolves `(source, sha256)` for `platform_key`, falling back to `"any"` or legacy fields.
    /// Returns `None` when `platforms` is explicitly populated but excludes `platform_key`.
    pub fn resolve_for_platform(&self, platform_key: &str) -> Option<(&str, &str)> {
        if !self.platforms.is_empty() {
            return self
                .platforms
                .get(platform_key)
                .or_else(|| self.platforms.get("any"))
                .filter(|p| !p.source.trim().is_empty())
                .map(|p| (p.source.as_str(), p.sha256.as_str()));
        }
        (!self.source.trim().is_empty()).then_some((self.source.as_str(), self.sha256.as_str()))
    }

    /// Lists declared platform keys (or `["any"]` for legacy single-source entries).
    pub fn supported_platforms(&self) -> Vec<&str> {
        if self.platforms.is_empty() {
            vec!["any"]
        } else {
            self.platforms.keys().map(String::as_str).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_entry_resolves_platform_specific_any_and_legacy_packages() {
        let multi: RegistryPluginEntry = serde_json::from_value(serde_json::json!({
            "id": "dev.tomoyo.capture",
            "name": "Capture",
            "version": "3.0.0",
            "description": "Native capture",
            "author": "Tomoyo",
            "license": "MIT",
            "runtime_kind": "stdio_jsonrpc",
            "min_tomoyo_version": "2.7.0",
            "platforms": {
                "windows-x86_64": { "source": "https://example.com/win.zip", "sha256": "a".repeat(64) },
                "linux-x86_64": { "source": "https://example.com/lin.zip", "sha256": "b".repeat(64) }
            }
        }))
        .unwrap();
        assert_eq!(multi.resolve_for_platform("windows-x86_64").map(|p| p.0), Some("https://example.com/win.zip"));
        assert_eq!(multi.resolve_for_platform("linux-x86_64").map(|p| p.0), Some("https://example.com/lin.zip"));
        assert!(multi.resolve_for_platform("macos-aarch64").is_none());
        assert_eq!(multi.supported_platforms(), vec!["linux-x86_64", "windows-x86_64"]);

        let legacy: RegistryPluginEntry = serde_json::from_value(serde_json::json!({
            "id": "dev.tomoyo.wasm",
            "name": "Wasm",
            "version": "1.0.0",
            "description": "Portable wasm",
            "author": "Tomoyo",
            "license": "MIT",
            "runtime_kind": "wasm",
            "source": "plugins/example-wasm",
            "sha256": "c".repeat(64)
        }))
        .unwrap();
        assert_eq!(legacy.resolve_for_platform("macos-aarch64").map(|p| p.0), Some("plugins/example-wasm"));
        assert_eq!(legacy.supported_platforms(), vec!["any"]);
    }
}
