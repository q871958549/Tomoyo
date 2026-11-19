//! Built-in Tier-1 zero-overhead providers for Tomoyo.

pub mod apps;
pub mod calc;
pub mod clipboard;
pub mod everything;
pub mod processes;
pub mod system;
pub mod web;
pub mod windows_switcher;

pub use apps::AppsProvider;
pub use calc::CalcProvider;
pub use clipboard::ClipboardProvider;
pub use everything::EverythingProvider;
pub use processes::ProcessProvider;
pub use system::SystemProvider;
pub use web::WebSearchProvider;
pub use windows_switcher::WindowsSwitcherProvider;
