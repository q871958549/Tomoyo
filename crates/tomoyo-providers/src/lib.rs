//! Built-in Tier-1 zero-overhead providers for Tomoyo.

pub mod apps;
pub mod calc;
pub mod everything;
pub mod everything_history;
mod everything_items;
#[cfg(target_os = "linux")]
pub mod everything_linux;
mod everything_rank;
mod file_actions;
pub mod flows;
mod ports;
pub mod processes;
pub mod snippets;
pub mod system;
pub mod web;
pub mod windows_switcher;

pub use apps::AppsProvider;
pub use calc::CalcProvider;
pub use everything::EverythingProvider;
pub use everything_history::record_everything_query;
pub use flows::FlowsProvider;
pub use processes::ProcessProvider;
pub use snippets::SnippetsProvider;
pub use system::SystemProvider;
pub use web::WebSearchProvider;
pub use windows_switcher::WindowsSwitcherProvider;
