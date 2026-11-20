//! Built-in Tier-1 zero-overhead providers for Tomoyo.

pub mod apps;
pub mod calc;
pub mod clipboard;
pub mod everything;
pub mod everything_history;
mod everything_rank;
pub mod processes;
pub mod snippets;
pub mod system;
pub mod web;
pub mod windows_switcher;

pub use apps::AppsProvider;
pub use calc::CalcProvider;
pub use clipboard::{
    normalize_single_line, sanitize_note, sanitize_tag, ClipboardEntry, ClipboardPayload,
    ClipboardProvider, ClipboardStore, ClipboardSubtype, ParsedClipQuery,
};
pub use everything::EverythingProvider;
pub use everything_history::record_everything_query;
pub use processes::ProcessProvider;
pub use snippets::SnippetsProvider;
pub use system::SystemProvider;
pub use web::WebSearchProvider;
pub use windows_switcher::WindowsSwitcherProvider;
