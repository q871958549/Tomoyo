//! Thin tracing-subscriber adapter & Win32 fatal dialog backed by `tomoyo_core::diagnostics`.

use std::io::{self, Write};
use tomoyo_core::{
    append_crash_record, init_process_diagnostics, logs_dir, write_process_log_bytes,
    DEFAULT_MAIN_LOG_BYTES,
};

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
        write_process_log_bytes(buf, self.mirror_stderr)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.mirror_stderr {
            let _ = io::stderr().flush();
        }
        Ok(())
    }
}

/// Initializes main-process 1 MB × 3 rolling log + panic hook + Win32 SEH fault capture.
pub fn init_logging_and_crash_guards(mirror_stderr: bool) -> RollingMakeWriter {
    init_process_diagnostics("tomoyo-app", "tomoyo", DEFAULT_MAIN_LOG_BYTES, true);
    RollingMakeWriter { mirror_stderr }
}

#[cfg(windows)]
pub fn show_fatal_error_dialog(message: &str) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    append_crash_record(&format!("FATAL STARTUP ERROR: {message}"));
    let log_dir = logs_dir();
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
