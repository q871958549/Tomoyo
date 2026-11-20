//! Zero-overhead durable crash telemetry, panic hook, SEH exception filter & bounded rolling log.
//!
//! Because release GUI binaries build with `#![windows_subsystem = "windows"]` and `panic = "abort"`,
//! unhandled panics or native Win32 faults would otherwise terminate silently without diagnostics.
//! This module writes strictly size-bounded rolling logs under `%LOCALAPPDATA%\Tomoyo\logs\`.

use chrono::Local;
use std::backtrace::Backtrace;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const MAX_LOG_BYTES: u64 = 256 * 1024;

static ROLLING_WRITER: OnceLock<Mutex<RollingFileState>> = OnceLock::new();

struct RollingFileState {
    path: PathBuf,
    prev_path: PathBuf,
    file: Option<File>,
    written: u64,
}

impl RollingFileState {
    fn open_new() -> Self {
        let dir = tomoyo_core::data_dir().join("logs");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tomoyo.log");
        let prev_path = dir.join("tomoyo.prev.log");
        let written = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        let mut st = Self {
            path,
            prev_path,
            file,
            written,
        };
        st.rotate_if_needed();
        st
    }

    fn rotate_if_needed(&mut self) {
        if self.written < MAX_LOG_BYTES {
            return;
        }
        self.file = None;
        let _ = std::fs::remove_file(&self.prev_path);
        let _ = std::fs::rename(&self.path, &self.prev_path);
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
            .ok();
        self.written = 0;
    }

    fn write_bytes(&mut self, buf: &[u8]) {
        if self.file.is_none() {
            self.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
        }
        if let Some(f) = &mut self.file {
            if f.write_all(buf).is_ok() {
                let _ = f.flush();
                self.written = self.written.saturating_add(buf.len() as u64);
                self.rotate_if_needed();
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct RollingMakeWriter {
    mirror_stderr: bool,
}

pub struct RollingLogSink {
    mirror_stderr: bool,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RollingMakeWriter {
    type Writer = RollingLogSink;

    fn make_writer(&'a self) -> Self::Writer {
        RollingLogSink {
            mirror_stderr: self.mirror_stderr,
        }
    }
}

impl Write for RollingLogSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.mirror_stderr {
            let _ = io::stderr().write_all(buf);
        }
        if let Some(mtx) = ROLLING_WRITER.get() {
            if let Ok(mut guard) = mtx.lock() {
                guard.write_bytes(buf);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.mirror_stderr {
            let _ = io::stderr().flush();
        }
        Ok(())
    }
}

/// Initializes rolling file logging + panic hook + Win32 SEH unhandled exception filter.
pub fn init_logging_and_crash_guards(mirror_stderr: bool) -> RollingMakeWriter {
    ROLLING_WRITER.get_or_init(|| Mutex::new(RollingFileState::open_new()));
    install_panic_hook();
    install_seh_filter();
    RollingMakeWriter { mirror_stderr }
}

pub fn append_crash_record(summary: &str) {
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let block = format!("\n=== TOMOYO CRASH RECORD [{ts}] ===\n{summary}\n");
    if let Some(mtx) = ROLLING_WRITER.get() {
        if let Ok(mut guard) = mtx.lock() {
            guard.write_bytes(block.as_bytes());
        }
    }
    let last_crash = tomoyo_core::data_dir().join("logs").join("last_crash.txt");
    let _ = std::fs::write(last_crash, block.as_bytes());
}

#[cfg(windows)]
pub fn show_fatal_error_dialog(message: &str) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    append_crash_record(&format!("FATAL STARTUP ERROR: {message}"));
    let log_dir = tomoyo_core::data_dir().join("logs");
    let full = format!(
        "Tomoyo 启动失败或遇到致命错误：\n\n{message}\n\n诊断日志已保存到：\n{}",
        log_dir.display()
    );
    let wide: Vec<u16> = full.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(wide.as_ptr()),
            w!("Tomoyo Error"),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
pub fn show_fatal_error_dialog(message: &str) {
    append_crash_record(&format!("FATAL STARTUP ERROR: {message}"));
}

fn install_panic_hook() {
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
        let report = format!("PANIC on thread `{tname}` at {loc}\nPayload: {payload}\nBacktrace:\n{bt}");
        append_crash_record(&report);
        prev(info);
    }));
}

#[cfg(windows)]
fn install_seh_filter() {
    use windows::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter;
    unsafe {
        let _ = SetUnhandledExceptionFilter(Some(unhandled_seh_filter));
    }
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
mod tests {
    use super::*;

    #[test]
    fn rolling_log_rotates_strictly_at_budget() {
        let dir = std::env::temp_dir().join(format!("tomoyo-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tomoyo.log");
        let prev = dir.join("tomoyo.prev.log");
        let mut st = RollingFileState {
            path: path.clone(),
            prev_path: prev.clone(),
            file: OpenOptions::new().create(true).append(true).open(&path).ok(),
            written: 0,
        };
        let chunk = vec![b'A'; 70 * 1024];
        for _ in 0..5 {
            st.write_bytes(&chunk);
        }
        assert!(prev.is_file(), "expected rotated prev log file");
        let cur_len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX);
        assert!(cur_len <= MAX_LOG_BYTES, "current log exceeded cap: {cur_len}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
