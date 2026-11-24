use crate::v2::{PluginEventKind, PluginHotkeyDecl, PluginSettingField};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("failed to parse plugin manifest TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid plugin manifest: {0}")]
    Invalid(String),
}

/// Preferred UI presentation mode declared by a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginDisplayMode {
    #[default]
    Auto,
    Inline,
    Window,
}

impl PluginDisplayMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Inline => "inline",
            Self::Window => "window",
        }
    }

    pub fn from_opt_str(raw: Option<&str>) -> Self {
        match raw.map(str::trim).unwrap_or("").to_ascii_lowercase().as_str() {
            "inline" => Self::Inline,
            "window" => Self::Window,
            _ => Self::Auto,
        }
    }
}

/// Plugin manifest stored as `tomoyo-plugin.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Relative asset path (e.g. `"icon.png"`) inside the plugin directory for dedicated logo rendering.
    #[serde(default)]
    pub icon: Option<String>,
    /// Catalog category (`"productivity"`, `"docs"`, `"dev"`, `"ai"`, `"media"`, `"system"`).
    #[serde(default)]
    pub category: Option<String>,
    /// Preferred host display mode (`"auto"`, `"inline"`, or `"window"`).
    #[serde(default)]
    pub display_mode: PluginDisplayMode,
    /// Searchable tags in the scalable Plugin Center.
    #[serde(default)]
    pub tags: Vec<String>,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub triggers: TriggerConfig,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub resources: ResourceLimits,
    /// Protocol v2: declarative global hotkeys managed by Tomoyo's microkernel.
    #[serde(default)]
    pub hotkeys: Vec<PluginHotkeyDecl>,
    /// Protocol v2: opt-in host events delivered via `tomoyo/event` JSON-RPC.
    #[serde(default)]
    pub events: Vec<PluginEventKind>,
    /// Protocol v2: declarative settings schema auto-rendered in Settings Center.
    #[serde(default)]
    pub settings: Vec<PluginSettingField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeConfig {
    /// Out-of-process isolated child speaking newline-delimited JSON-RPC 2.0 on stdio.
    StdioJsonrpc {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Capability-sandboxed, fuel-metered WebAssembly plugin (`.wasm` or `.wat`).
    Wasm {
        module: String,
        /// Per-invocation instruction fuel ceiling (prevents infinite loops).
        #[serde(default = "default_wasm_fuel")]
        max_fuel: u64,
    },
    /// On-demand ephemeral WebView host (spawned on open, terminated on close — 0 idle RAM!).
    Webview {
        entry: String,
        #[serde(default = "default_true")]
        utools_compat: bool,
        #[serde(default = "default_webview_width")]
        width: u32,
        #[serde(default = "default_webview_height")]
        height: u32,
    },
}

fn default_wasm_fuel() -> u64 {
    5_000_000
}
fn default_true() -> bool {
    true
}
fn default_webview_width() -> u32 {
    860
}
fn default_webview_height() -> u32 {
    600
}

impl RuntimeConfig {
    pub fn kind_badge(&self) -> &'static str {
        match self {
            Self::StdioJsonrpc { .. } => "STDIO",
            Self::Wasm { .. } => "WASM",
            Self::Webview { .. } => "WEBVIEW",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Keyword triggers, e.g. `["hi", "hello"]` matched on `hi <args>` or exact `hi`.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Optional regex pattern tested against the raw query.
    #[serde(default)]
    pub regex: Option<String>,
    /// Optional clipboard content kinds that activate this plugin.
    #[serde(default)]
    pub clipboard_types: Vec<ClipboardKind>,
    /// If true, receives global fallback queries (opt-in; keep false for zero idle cost).
    #[serde(default)]
    pub match_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardKind {
    Text,
    Url,
    Path,
    Json,
    Color,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    FilesystemRead,
    FilesystemWrite,
    Network,
    Clipboard,
    ShellExec,
    SystemInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Hard memory limit per plugin (MB). Enforced via Windows Job Object or WASM StoreLimits.
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u32,
    /// Seconds of inactivity before the host automatically reaps/unloads the plugin.
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    /// Per-query RPC timeout in milliseconds.
    #[serde(default = "default_query_timeout_ms")]
    pub query_timeout_ms: u64,
}

fn default_max_memory_mb() -> u32 {
    128
}
fn default_idle_timeout_secs() -> u64 {
    60
}
fn default_query_timeout_ms() -> u64 {
    1500
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_mb: default_max_memory_mb(),
            idle_timeout_secs: default_idle_timeout_secs(),
            query_timeout_ms: default_query_timeout_ms(),
        }
    }
}

impl PluginManifest {
    pub fn from_toml_str(input: &str) -> Result<Self, ManifestError> {
        let manifest: Self = toml::from_str(input)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.id.trim().is_empty() {
            return Err(ManifestError::Invalid("`id` must not be empty".into()));
        }
        if self.name.trim().is_empty() {
            return Err(ManifestError::Invalid("`name` must not be empty".into()));
        }
        if self.resources.max_memory_mb == 0 || self.resources.max_memory_mb > 4096 {
            return Err(ManifestError::Invalid(
                "`resources.max_memory_mb` must be between 1 and 4096".into(),
            ));
        }
        if self.resources.query_timeout_ms == 0 {
            return Err(ManifestError::Invalid(
                "`resources.query_timeout_ms` must be > 0".into(),
            ));
        }
        Ok(())
    }

    pub fn has_permission(&self, perm: Permission) -> bool {
        self.permissions.contains(&perm)
    }
}

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
    fn parses_valid_manifests_all_runtimes() {
        let stdio = r#"
id = "dev.tomoyo.hello"
name = "Hello Plugin"
version = "0.1.0"
[runtime]
kind = "stdio_jsonrpc"
command = "python"
args = ["main.py"]
[triggers]
keywords = ["hi"]
"#;
        assert_eq!(PluginManifest::from_toml_str(stdio).unwrap().runtime.kind_badge(), "STDIO");

        let wasm = r#"
id = "dev.tomoyo.wasm"
name = "Wasm Demo"
version = "0.1.0"
permissions = ["clipboard"]
[runtime]
kind = "wasm"
module = "plugin.wasm"
[triggers]
keywords = ["jwt"]
"#;
        let m = PluginManifest::from_toml_str(wasm).unwrap();
        assert_eq!(m.runtime.kind_badge(), "WASM");
        assert!(m.has_permission(Permission::Clipboard));
        assert!(!m.has_permission(Permission::FilesystemRead));

        let web = r#"
id = "dev.tomoyo.json"
name = "JSON Inspector"
version = "0.1.0"
[runtime]
kind = "webview"
entry = "index.html"
utools_compat = true
[triggers]
keywords = ["json"]
"#;
        let m = PluginManifest::from_toml_str(web).unwrap();
        assert_eq!(m.runtime.kind_badge(), "WEBVIEW");
    }

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
