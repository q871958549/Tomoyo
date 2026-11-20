//! Tomoyo Open Plugin Protocol (v1).
//!
//! Zero OS dependencies — shared between the Tomoyo core host, built-in providers,
//! and third-party plugins written in Rust or any JSON-RPC 2.0 capable language.

pub mod filter;
pub mod manifest;
pub mod rpc;
pub mod types;

pub use filter::*;
pub use manifest::*;
pub use rpc::*;
pub use types::*;

/// Current wire protocol version.
pub const PROTOCOL_VERSION: u32 = 1;
