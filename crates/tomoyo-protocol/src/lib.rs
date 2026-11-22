//! Open Plugin Protocol (v2, backward-compatible with v1) for **Tomoyo Launcher**.
//!
//! Zero OS dependencies — shared between the Tomoyo Launcher core host
//! (`github.com/q871958549/Tomoyo`), built-in providers, and third-party plugins
//! authored in Rust or any JSON-RPC 2.0 / WebAssembly capable language.
//!
//! *Disambiguation: Tomoyo Launcher is an open-source Rust desktop launcher for Linux
//! and Windows, unrelated to the TOMOYO Linux kernel LSM module.*

pub mod filter;
pub mod manifest;
pub mod rpc;
pub mod types;
pub mod v2;

pub use filter::*;
pub use manifest::*;
pub use rpc::*;
pub use types::*;
pub use v2::*;

/// Current wire protocol version (v2 adds declarative hotkeys, events & settings schema; v1 compatible).
pub const PROTOCOL_VERSION: u32 = 2;
