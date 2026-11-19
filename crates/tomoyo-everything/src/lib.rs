//! Pure-Rust native IPC client for Voidtools Everything (1.4 & 1.5a).
//!
//! Speaks the `WM_COPYDATA` `EVERYTHING_IPC_QUERY2W` protocol directly with zero
//! external DLL dependencies and strict timeouts so the launcher can never hang.

use chrono::{DateTime, Local, TimeZone, Utc};
use thiserror::Error;

pub mod ipc;
pub mod wire;

#[derive(Debug, Error)]
pub enum EverythingError {
    #[error("Everything is not running (IPC window not found)")]
    NotRunning,
    #[error("Everything IPC query timed out after {0}ms")]
    Timeout(u32),
    #[error("Everything IPC error: {0}")]
    Ipc(String),
    #[error("Everything is only supported on Windows")]
    UnsupportedPlatform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EverythingVersion {
    V14,
    V15Alpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    NameAsc = 1,
    NameDesc = 2,
    PathAsc = 3,
    PathDesc = 4,
    SizeAsc = 5,
    SizeDesc = 6,
    DateModifiedAsc = 13,
    DateModifiedDesc = 14,
    RunCountDesc = 20,
}

#[derive(Debug, Clone)]
pub struct EverythingQueryOptions {
    pub max_results: u32,
    pub offset: u32,
    pub match_case: bool,
    pub match_whole_word: bool,
    pub match_path: bool,
    pub regex: bool,
    pub sort: SortOrder,
    pub timeout_ms: u32,
}

impl Default for EverythingQueryOptions {
    fn default() -> Self {
        Self {
            max_results: 30,
            offset: 0,
            match_case: false,
            match_whole_word: false,
            match_path: false,
            regex: false,
            sort: SortOrder::DateModifiedDesc,
            timeout_ms: 450,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EverythingEntry {
    pub name: String,
    pub path: String,
    pub full_path: String,
    pub is_folder: bool,
    pub is_drive: bool,
    pub size: Option<u64>,
    pub date_modified_filetime: Option<u64>,
    pub attributes: Option<u32>,
}

impl EverythingEntry {
    /// Formats file size in human-readable units (e.g. `14.2 KB`, `3.1 MB`).
    pub fn formatted_size(&self) -> Option<String> {
        let bytes = self.size?;
        Some(format_bytes(bytes))
    }

    /// Converts Windows FILETIME (100ns ticks since 1601-01-01 UTC) to local datetime.
    pub fn modified_local(&self) -> Option<DateTime<Local>> {
        let ft = self.date_modified_filetime?;
        filetime_to_local(ft)
    }
}

#[derive(Debug, Clone)]
pub struct EverythingResponse {
    pub total_folders: u32,
    pub total_files: u32,
    pub total_items: u32,
    pub entries: Vec<EverythingEntry>,
}

/// Checks whether an Everything 1.4 or 1.5a IPC instance is currently reachable.
pub fn detect_running() -> Option<EverythingVersion> {
    ipc::detect_running()
}

/// Executes a query against the running Everything instance.
pub fn query(
    search: &str,
    opts: &EverythingQueryOptions,
) -> Result<EverythingResponse, EverythingError> {
    ipc::query(search, opts)
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut val = bytes as f64;
    let mut idx = 0;
    while val >= 1024.0 && idx < UNITS.len() - 1 {
        val /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{bytes} B")
    } else if val >= 100.0 {
        format!("{val:.0} {}", UNITS[idx])
    } else {
        format!("{val:.1} {}", UNITS[idx])
    }
}

pub fn filetime_to_local(ft: u64) -> Option<DateTime<Local>> {
    // Difference between 1601-01-01 and 1970-01-01 in 100ns ticks.
    const WINDOWS_TO_UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    if ft < WINDOWS_TO_UNIX_EPOCH_TICKS || ft == u64::MAX {
        return None;
    }
    let unix_ticks = ft - WINDOWS_TO_UNIX_EPOCH_TICKS;
    let secs = (unix_ticks / 10_000_000) as i64;
    let nanos = ((unix_ticks % 10_000_000) * 100) as u32;
    let utc = Utc.timestamp_opt(secs, nanos).single()?;
    Some(utc.with_timezone(&Local))
}
