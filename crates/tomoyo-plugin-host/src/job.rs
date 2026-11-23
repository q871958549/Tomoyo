//! OS-level Job Object isolation for Windows plugin child processes.

#[cfg(windows)]
pub struct JobGuard {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for JobGuard {}
#[cfg(windows)]
unsafe impl Sync for JobGuard {}

#[cfg(windows)]
impl JobGuard {
    /// Creates a Windows Job Object with `KILL_ON_JOB_CLOSE` and a hard per-process memory cap,
    /// then assigns `child` into the job (forbidding breakaway by default for untrusted plugins).
    pub fn assign_child(child: &std::process::Child, max_memory_mb: u32) -> Result<Self, String> {
        Self::assign_child_impl(child, max_memory_mb, false)
    }

    /// Creates a Job Object for trusted WebView children that still kills the UI process tree
    /// on close, while permitting detached self-updaters via `CREATE_BREAKAWAY_FROM_JOB`.
    pub fn assign_webview_child(
        child: &std::process::Child,
        max_memory_mb: u32,
    ) -> Result<Self, String> {
        Self::assign_child_impl(child, max_memory_mb, true)
    }

    fn assign_child_impl(
        child: &std::process::Child,
        max_memory_mb: u32,
        allow_breakaway: bool,
    ) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        };

        unsafe {
            let job = CreateJobObjectW(None, None)
                .map_err(|e| format!("CreateJobObjectW failed: {e}"))?;

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            let mut flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            if allow_breakaway {
                flags |= JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            }
            info.BasicLimitInformation.LimitFlags = flags;
            info.ProcessMemoryLimit = (max_memory_mb as usize).saturating_mul(1024 * 1024);

            if let Err(e) = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) {
                let _ = CloseHandle(job);
                return Err(format!("SetInformationJobObject failed: {e}"));
            }

            let proc_handle = HANDLE(child.as_raw_handle());
            if let Err(e) = AssignProcessToJobObject(job, proc_handle) {
                let _ = CloseHandle(job);
                return Err(format!("AssignProcessToJobObject failed: {e}"));
            }

            Ok(Self { handle: job })
        }
    }
}

#[cfg(windows)]
impl Drop for JobGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_invalid() {
                let _ = windows::Win32::Foundation::CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub struct JobGuard {
    pgid: i32,
}

#[cfg(target_os = "linux")]
impl JobGuard {
    pub fn assign_child(child: &std::process::Child, _max_memory_mb: u32) -> Result<Self, String> {
        Ok(Self { pgid: child.id() as i32 })
    }
}

#[cfg(target_os = "linux")]
impl Drop for JobGuard {
    fn drop(&mut self) {
        if self.pgid > 0 {
            unsafe extern "C" {
                fn kill(pid: i32, sig: i32) -> i32;
            }
            const SIGKILL: i32 = 9;
            unsafe {
                let _ = kill(-self.pgid, SIGKILL);
            }
        }
    }
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub struct JobGuard;

#[cfg(all(not(windows), not(target_os = "linux")))]
impl JobGuard {
    pub fn assign_child(_child: &std::process::Child, _max_memory_mb: u32) -> Result<Self, String> {
        Ok(Self)
    }
}
