//! Unified multi-process rolling logger, panic/SEH crash guard, correlation trace IDs,
//! and bounded diagnostic bundle builder for Tomoyo (`tomoyo.exe`, `tomoyo-webview-host.exe`, etc.).
//!
//! Writes strictly size-bounded rotating logs under `%LOCALAPPDATA%\Tomoyo\logs\`:
//! - `tomoyo.log` (+ `.1.log` / `.2.log`, 1 MB each)
//! - `webview.log` (+ `.1.log`, 512 KB each)
//! - `last_crash.txt` (durable panic / Win32 SEH fault snapshot with stack backtrace)

use crate::{current_process_stats, data_dir, CURRENT_VERSION};
use chrono::{FixedOffset, Utc};
use std::backtrace::Backtrace;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

pub const DEFAULT_MAIN_LOG_BYTES: u64 = 1024 * 1024;
pub const DEFAULT_SUB_LOG_BYTES: u64 = 512 * 1024;
const MAX_TAIL_READ_BYTES: u64 = 64 * 1024;

static TRACE_SEQ: AtomicU64 = AtomicU64::new(1);
static PROCESS_LOG: OnceLock<Mutex<RollingLogState>> = OnceLock::new();
static PROCESS_COMPONENT: OnceLock<&'static str> = OnceLock::new();

/// Formats the current wall-clock time in Beijing Standard Time (`UTC+08:00`).
pub fn format_beijing_now() -> String {
    let bj = FixedOffset::east_opt(8 * 3600).unwrap();
    Utc::now()
        .with_timezone(&bj)
        .format("%Y-%m-%d %H:%M:%S%.3f +08:00")
        .to_string()
}

/// Generates a compact, greppable per-operation correlation trace ID (e.g. `PLG-00012`).
pub fn next_trace_id(prefix: &str) -> String {
    let seq = TRACE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{seq:05}")
}

/// Returns `%LOCALAPPDATA%\Tomoyo\logs` (creating it best-effort).
pub fn logs_dir() -> PathBuf {
    let dir = data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub struct RollingLogState {
    path: PathBuf,
    backup1: PathBuf,
    backup2: PathBuf,
    max_bytes: u64,
    keep_two_backups: bool,
    file: Option<File>,
    written: u64,
}

impl RollingLogState {
    pub fn open(dir: &Path, stem: &str, max_bytes: u64, keep_two_backups: bool) -> Self {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(format!("{stem}.log"));
        let backup1 = dir.join(format!("{stem}.1.log"));
        let backup2 = dir.join(format!("{stem}.2.log"));
        let written = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        let mut st = Self {
            path,
            backup1,
            backup2,
            max_bytes: max_bytes.max(16 * 1024),
            keep_two_backups,
            file,
            written,
        };
        st.rotate_if_needed();
        st
    }

    fn rotate_if_needed(&mut self) {
        if self.written < self.max_bytes {
            return;
        }
        self.file = None;
        if self.keep_two_backups {
            let _ = std::fs::remove_file(&self.backup2);
            let _ = std::fs::rename(&self.backup1, &self.backup2);
        } else {
            let _ = std::fs::remove_file(&self.backup1);
        }
        let _ = std::fs::rename(&self.path, &self.backup1);
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
            .ok();
        self.written = 0;
    }

    pub fn write_bytes(&mut self, buf: &[u8]) {
        if self.file.is_none() {
            self.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
        }
        if let Some(f) = &mut self.file
            && f.write_all(buf).is_ok() {
                let _ = f.flush();
                self.written = self.written.saturating_add(buf.len() as u64);
                self.rotate_if_needed();
            }
    }
}

/// Initializes process-wide rolling file logging, Rust panic hook, and Win32 SEH fault capture.
pub fn init_process_diagnostics(
    component: &'static str,
    file_stem: &str,
    max_bytes: u64,
    keep_two_backups: bool,
) {
    let _ = PROCESS_COMPONENT.set(component);
    let _ = PROCESS_LOG.get_or_init(|| {
        let dir = logs_dir();
        Mutex::new(RollingLogState::open(&dir, file_stem, max_bytes, keep_two_backups))
    });
    install_panic_hook();
    install_seh_filter();
}

/// Writes raw log bytes to the active process rolling log (and optionally mirrors to stderr).
pub fn write_process_log_bytes(buf: &[u8], mirror_stderr: bool) -> io::Result<usize> {
    if mirror_stderr {
        let _ = io::stderr().write_all(buf);
    }
    if let Some(mtx) = PROCESS_LOG.get()
        && let Ok(mut guard) = mtx.lock() {
            guard.write_bytes(buf);
        }
    Ok(buf.len())
}

/// Appends a durable crash/fatal record to both the process rolling log and `last_crash.txt`.
pub fn append_crash_record(summary: &str) {
    let ts = format_beijing_now();
    let comp = PROCESS_COMPONENT.get().copied().unwrap_or("tomoyo");
    let pid = std::process::id();
    let block = format!(
        "\n=== TOMOYO CRASH RECORD [{ts}] component={comp} ver=v{CURRENT_VERSION} pid={pid} ===\n{summary}\n"
    );
    if let Some(mtx) = PROCESS_LOG.get()
        && let Ok(mut guard) = mtx.lock() {
            guard.write_bytes(block.as_bytes());
        }
    let last_crash = logs_dir().join("last_crash.txt");
    let _ = std::fs::write(last_crash, block.as_bytes());
}

/// Reads at most the last `64 KB` of a named log file inside `%LOCALAPPDATA%\Tomoyo\logs\`
/// (strictly single-segment filename) and returns up to `max_lines` lines.
pub fn read_log_tail(file_name: &str, max_lines: usize, warn_or_error_only: bool) -> String {
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains("..")
    {
        return String::new();
    }
    let path = logs_dir().join(file_name);
    let Ok(mut f) = File::open(&path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > MAX_TAIL_READ_BYTES {
        let _ = f.seek(SeekFrom::Start(len - MAX_TAIL_READ_BYTES));
    }
    let mut raw = Vec::new();
    if f.read_to_end(&mut raw).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&raw);
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|line| {
            if !warn_or_error_only {
                return true;
            }
            line.contains(" WARN ")
                || line.contains(" ERROR ")
                || line.contains("CRASH RECORD")
                || line.contains("PANIC")
                || line.contains("SEH")
        })
        .collect();
    if lines.len() > max_lines {
        lines = lines.split_off(lines.len() - max_lines);
    }
    lines.join("\n")
}

/// Counts recent WARN and ERROR lines in `tomoyo.log` (bounded tail scan).
pub fn count_recent_log_issues() -> (usize, usize, bool) {
    let tail = read_log_tail("tomoyo.log", 400, true);
    let mut warns = 0usize;
    let mut errs = 0usize;
    for line in tail.lines() {
        if line.contains(" ERROR ") || line.contains("PANIC") || line.contains("CRASH RECORD") {
            errs += 1;
        } else if line.contains(" WARN ") {
            warns += 1;
        }
    }
    let has_crash = logs_dir().join("last_crash.txt").is_file();
    (warns, errs, has_crash)
}

/// Archives `last_crash.txt` and active logs into `%LOCALAPPDATA%\Tomoyo\logs\archive\` and clears active warnings.
pub fn archive_diagnostic_issues() -> Result<usize, String> {
    archive_diagnostic_issues_in(&logs_dir())
}

pub(crate) fn archive_diagnostic_issues_in(dir: &Path) -> Result<usize, String> {
    let stamp = Utc::now()
        .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
        .format("%Y%m%d_%H%M%S")
        .to_string();
    let archive_dir = dir.join("archive").join(stamp);
    std::fs::create_dir_all(&archive_dir).map_err(|e| format!("Failed to create archive dir: {e}"))?;

    let mut archived = 0usize;
    let crash_path = dir.join("last_crash.txt");
    if crash_path.is_file() {
        let dest = archive_dir.join("last_crash.txt");
        if std::fs::rename(&crash_path, &dest).is_err() {
            std::fs::copy(&crash_path, &dest).map_err(|e| format!("Failed to copy crash snapshot: {e}"))?;
            let _ = std::fs::remove_file(&crash_path);
        }
        archived += 1;
    }

    for name in ["tomoyo.log", "webview.log", "update.log"] {
        let src = dir.join(name);
        if !src.is_file() {
            continue;
        }
        let len = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
        if len > 0 {
            let _ = std::fs::copy(&src, archive_dir.join(name));
            archived += 1;
        }
        if let Ok(f) = OpenOptions::new().write(true).open(&src) {
            let _ = f.set_len(0);
        }
    }
    if archived == 0 {
        let _ = std::fs::remove_dir(&archive_dir);
    }
    Ok(archived)
}

/// Builds a concise, copyable diagnostic bundle for troubleshooting & bug reports.
pub fn build_diagnostic_bundle() -> String {
    let stats = current_process_stats();
    let ts = format_beijing_now();
    let log_dir = logs_dir();
    let main_issues = read_log_tail("tomoyo.log", 45, true);
    let main_recent = read_log_tail("tomoyo.log", 20, false);
    let webview_tail = read_log_tail("webview.log", 25, false);
    let update_tail = read_log_tail("update.log", 20, false);
    let crash_tail = read_log_tail("last_crash.txt", 35, false);

    let mut out = format!(
        "=== Tomoyo Diagnostic Report ({ts}) ===\n\
         Version: v{CURRENT_VERSION} | PID: {}\n\
         Memory: WS={:.2} MB | Private={:.2} MB | GDI={} | USER={} | Handles={}\n\
         Logs Dir: {}\n",
        std::process::id(),
        stats.working_set_mb(),
        stats.private_mb(),
        stats.gdi_objects,
        stats.user_objects,
        stats.handle_count,
        log_dir.display()
    );

    if !crash_tail.is_empty() {
        out.push_str("\n--- last_crash.txt ---\n");
        out.push_str(&crash_tail);
        out.push('\n');
    }
    out.push_str("\n--- tomoyo.log (recent WARN/ERROR) ---\n");
    if main_issues.is_empty() {
        out.push_str("(no recent warnings or errors)\n");
    } else {
        out.push_str(&main_issues);
        out.push('\n');
    }
    out.push_str("\n--- tomoyo.log (latest 20 lines) ---\n");
    out.push_str(&main_recent);
    out.push('\n');
    if !webview_tail.is_empty() {
        out.push_str("\n--- webview.log (latest 25 lines) ---\n");
        out.push_str(&webview_tail);
        out.push('\n');
    }
    if !update_tail.is_empty() {
        out.push_str("\n--- update.log (latest 20 lines) ---\n");
        out.push_str(&update_tail);
        out.push('\n');
    }
    out
}

fn install_panic_hook() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            let tname = thread.name().unwrap_or("unnamed");
            let loc = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".to_string());
            let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            let bt = Backtrace::force_capture();
            let report = format!(
                "PANIC on thread `{tname}` at {loc}\nPayload: {payload}\nBacktrace:\n{bt}"
            );
            append_crash_record(&report);
            prev(info);
        }));
    });
}

#[cfg(windows)]
fn install_seh_filter() {
    use windows::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter;
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| unsafe {
        let _ = SetUnhandledExceptionFilter(Some(unhandled_seh_filter));
    });
}

#[cfg(not(windows))]
fn install_seh_filter() {}

#[cfg(windows)]
unsafe extern "system" fn unhandled_seh_filter(
    info: *const windows::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    if !info.is_null() {
        let rec = unsafe { (*info).ExceptionRecord };
        if !rec.is_null() {
            let code = unsafe { (*rec).ExceptionCode.0 as u32 };
            let addr = unsafe { (*rec).ExceptionAddress as usize };
            let flags = unsafe { (*rec).ExceptionFlags };
            let bt = Backtrace::force_capture();
            let report = format!(
                "UNHANDLED WIN32 SEH EXCEPTION\nCode: 0x{code:08X} | Flags: 0x{flags:08X} | Address: 0x{addr:016X}\nBacktrace:\n{bt}"
            );
            append_crash_record(&report);
        }
    }
    0 // EXCEPTION_CONTINUE_SEARCH
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
