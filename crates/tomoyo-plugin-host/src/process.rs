use crate::job::JobGuard;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use tomoyo_protocol::{
    HandshakeParams, HandshakeResult, JsonRpcRequest, JsonRpcResponse, METHOD_HANDSHAKE,
    METHOD_SHUTDOWN, PROTOCOL_VERSION,
};

const MAX_STDERR_LINES: usize = 12;

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("failed to spawn plugin process: {0}")]
    Spawn(String),
    #[error("plugin RPC timeout after {0}ms")]
    Timeout(u64),
    #[error("plugin process exited or closed stdout ({0})")]
    Closed(String),
    #[error("plugin JSON-RPC error ({code}): {message}")]
    Rpc { code: i32, message: String },
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub struct LivePluginProcess {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<JsonRpcResponse>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    _job: Option<JobGuard>,
    next_id: u64,
    last_used: Instant,
    pid: u32,
}

impl LivePluginProcess {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        plugin_id: &str,
        plugin_dir: &Path,
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        max_memory_mb: u32,
        handshake_timeout_ms: u64,
        allow_breakaway: bool,
    ) -> Result<Self, ProcessError> {
        let resolved = resolve_command(plugin_dir, command);
        let mut cmd = Command::new(&resolved);
        cmd.args(args)
            .current_dir(plugin_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (k, v) in env {
            cmd.env(k, v);
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    unsafe extern "C" {
                        fn setpgid(pid: i32, pgid: i32) -> i32;
                        fn prctl(option: i32, arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> i32;
                    }
                    const PR_SET_PDEATHSIG: i32 = 1;
                    const SIGKILL: usize = 9;
                    let _ = setpgid(0, 0);
                    let _ = prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0);
                    Ok(())
                });
            }
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| ProcessError::Spawn(format!("{resolved}: {e}")))?;

        let pid = child.id();
        let job = match JobGuard::assign_child_with_breakaway(&child, max_memory_mb, allow_breakaway) {
            Ok(j) => Some(j),
            Err(e) => {
                tracing::warn!(plugin_id = %plugin_id, pid = pid, "Job Object assignment skipped: {e}");
                None
            }
        };

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProcessError::Spawn("missing child stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProcessError::Spawn("missing child stdout".into()))?;
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(MAX_STDERR_LINES)));
        if let Some(stderr) = child.stderr.take() {
            let tail_clone = Arc::clone(&stderr_tail);
            let pid_label = plugin_id.to_string();
            let _ = std::thread::Builder::new()
                .name(format!("tomoyo-err-{pid}"))
                .spawn(move || {
                    let mut reader = BufReader::new(stderr);
                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {
                                let clean: String = line.trim().chars().take(512).collect();
                                if clean.is_empty() {
                                    continue;
                                }
                                tracing::warn!(
                                    target: "plugin_stderr",
                                    plugin_id = %pid_label,
                                    pid = pid,
                                    "{clean}"
                                );
                                if let Ok(mut q) = tail_clone.lock() {
                                    if q.len() >= MAX_STDERR_LINES {
                                        q.pop_front();
                                    }
                                    q.push_back(clean);
                                }
                            }
                        }
                    }
                });
        }

        // Bounded channel prevents memory spikes if a misbehaving plugin spams stdout.
        let (tx, rx) = mpsc::sync_channel::<JsonRpcResponse>(8);
        std::thread::Builder::new()
            .name(format!("tomoyo-plug-{pid}"))
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed)
                                && tx.send(resp).is_err() {
                                    break;
                                }
                        }
                    }
                }
            })
            .map_err(|e| ProcessError::Spawn(format!("reader thread failed: {e}")))?;

        let mut proc = Self {
            child,
            stdin,
            responses: rx,
            stderr_tail,
            _job: job,
            next_id: 1,
            last_used: Instant::now(),
            pid,
        };

        let hs_params = serde_json::to_value(HandshakeParams {
            protocol_version: PROTOCOL_VERSION,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
        })?;
        let hs_val = proc.call(
            METHOD_HANDSHAKE,
            hs_params,
            Duration::from_millis(handshake_timeout_ms.max(2000)),
        )?;
        let hs: HandshakeResult = serde_json::from_value(hs_val)?;
        if hs.protocol_version == 0 || hs.protocol_version > PROTOCOL_VERSION {
            return Err(ProcessError::Spawn(format!(
                "unsupported plugin protocol version: {}",
                hs.protocol_version
            )));
        }

        Ok(proc)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn last_used(&self) -> Instant {
        self.last_used
    }

    pub fn recent_stderr(&self) -> Vec<String> {
        self.stderr_tail
            .lock()
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn describe_close_reason(&mut self) -> String {
        let status = self
            .child
            .try_wait()
            .ok()
            .flatten()
            .map(|s| format!("exit={s}"))
            .unwrap_or_else(|| "eof".into());
        let tail = self.recent_stderr();
        if tail.is_empty() {
            status
        } else {
            let last = tail.iter().rev().take(2).cloned().collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
            format!("{status}; stderr: {last}")
        }
    }

    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, ProcessError> {
        let id = self.next_id;
        self.next_id += 1;
        let req = JsonRpcRequest::new(id, method, params);
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');

        if self.stdin.write_all(line.as_bytes()).and_then(|_| self.stdin.flush()).is_err() {
            return Err(ProcessError::Closed(self.describe_close_reason()));
        }

        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(ProcessError::Timeout(timeout.as_millis() as u64));
            }
            let remain = deadline - now;
            match self.responses.recv_timeout(remain) {
                Ok(resp) => {
                    if resp.id != id {
                        continue;
                    }
                    self.last_used = Instant::now();
                    if let Some(err) = resp.error {
                        return Err(ProcessError::Rpc {
                            code: err.code,
                            message: err.message,
                        });
                    }
                    return Ok(resp.result.unwrap_or(Value::Null));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(ProcessError::Timeout(timeout.as_millis() as u64));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(ProcessError::Closed(self.describe_close_reason()));
                }
            }
        }
    }

    pub fn shutdown_gracefully(&mut self) {
        let _ = self.call(
            METHOD_SHUTDOWN,
            serde_json::json!({}),
            Duration::from_millis(250),
        );
    }
}

impl Drop for LivePluginProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Resolves relative plugin binaries and falls back cleanly to sibling executables
/// (e.g. Cargo `target/debug` / `target/release`) during local development.
/// Always checks `.is_file()` so sibling directories (like `%LOCALAPPDATA%\Programs\Python`)
/// are never mistaken for executable binaries!
fn resolve_command(plugin_dir: &Path, command: &str) -> String {
    let candidate = plugin_dir.join(command);
    if candidate.is_file() {
        return candidate.to_string_lossy().into_owned();
    }
    #[cfg(windows)]
    {
        let with_exe = plugin_dir.join(format!("{command}.exe"));
        if with_exe.is_file() {
            return with_exe.to_string_lossy().into_owned();
        }
    }

    // Check alongside current executable (makes workspace dev & installed plugins work immediately!).
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent() {
            let clean_name = Path::new(command)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| command.to_string());
            let sib = exe_dir.join(&clean_name);
            if sib.is_file() {
                return sib.to_string_lossy().into_owned();
            }
            #[cfg(windows)]
            {
                let sib_exe = exe_dir.join(format!("{clean_name}.exe"));
                if sib_exe.is_file() {
                    return sib_exe.to_string_lossy().into_owned();
                }
                // Only check parent dir when running inside Cargo's `target/.../deps` test folder.
                let is_deps_dir = exe_dir
                    .file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("deps"));
                if is_deps_dir
                    && let Some(parent_dir) = exe_dir.parent() {
                        let parent_sib = parent_dir.join(&clean_name);
                        if parent_sib.is_file() {
                            return parent_sib.to_string_lossy().into_owned();
                        }
                        let parent_sib_exe = parent_dir.join(format!("{clean_name}.exe"));
                        if parent_sib_exe.is_file() {
                            return parent_sib_exe.to_string_lossy().into_owned();
                        }
                    }
            }
        }

    // On Windows, avoid the Microsoft Store `WindowsApps\python.exe` stub when `python` / `python3` is requested.
    #[cfg(windows)]
    if (command.eq_ignore_ascii_case("python") || command.eq_ignore_ascii_case("python3"))
        && let Some(real_py) = find_real_windows_python() {
            return real_py;
        }

    command.to_string()
}

#[cfg(windows)]
fn find_real_windows_python() -> Option<String> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("python.exe");
            let s = candidate.to_string_lossy();
            if candidate.is_file() && !s.to_lowercase().contains("windowsapps") {
                return Some(s.into_owned());
            }
        }
    }
    let py_launcher = Path::new(r"C:\Windows\py.exe");
    if py_launcher.is_file() {
        return Some(py_launcher.to_string_lossy().into_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_command_never_picks_directory() {
        let tmp = std::env::temp_dir().join(format!("tomoyo_rescmd_{}", std::process::id()));
        let sub_dir = tmp.join("python");
        let _ = std::fs::create_dir_all(&sub_dir);
        let resolved = resolve_command(&tmp, "python");
        assert!(
            !Path::new(&resolved).is_dir(),
            "resolve_command must never return a directory: {resolved}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
