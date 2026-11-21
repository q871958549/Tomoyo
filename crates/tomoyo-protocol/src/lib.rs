//! Tomoyo Open Plugin Protocol (v1).
//!
//! Zero OS dependencies — shared between the Tomoyo core host, built-in providers,
//! and third-party plugins written in Rust or any JSON-RPC 2.0 capable language.

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
