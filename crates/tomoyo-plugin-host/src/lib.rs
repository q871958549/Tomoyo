//! Unified Multi-Runtime Plugin Host for Tomoyo.
//!
//! Supports 3 clean tiers under one unified protocol:
//! - Tier 2A (`stdio_jsonrpc`): Lazy child process isolated via Windows Job Object + idle reaper.
//! - Tier 2B (`wasm`): Capability-sandboxed, fuel-metered, memory-capped `wasmi` sandbox (0 JIT bloat).
//! - Tier 3  (`webview`): Zero-idle-RAM trigger returning `ActionKind::OpenWebView` for ephemeral host.

pub mod host;
pub mod job;
pub mod process;
pub mod wasm;

pub use host::{default_plugin_dirs, PluginHost, PluginRuntimeInfo};
pub use job::JobGuard;
