use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static START_TIME: OnceLock<Instant> = OnceLock::new();
static HIBERNATION_CYCLES: AtomicU64 = AtomicU64::new(0);

/// Real-time OS memory and handle snapshot for self-diagnostics (`:stats`) and leak tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessMemoryStats {
    pub working_set_bytes: u64,
    pub private_bytes: u64,
    pub pagefile_bytes: u64,
    pub gdi_objects: u32,
    pub user_objects: u32,
    pub handle_count: u32,
    pub uptime_secs: u64,
    pub hibernation_cycles: u64,
}

impl ProcessMemoryStats {
    pub fn working_set_mb(self) -> f64 {
        self.working_set_bytes as f64 / (1024.0 * 1024.0)
    }

    pub fn private_mb(self) -> f64 {
        self.private_bytes as f64 / (1024.0 * 1024.0)
    }
}

pub fn init_uptime_clock() {
    let _ = START_TIME.get_or_init(Instant::now);
}

pub fn record_hibernation_cycle() -> u64 {
    HIBERNATION_CYCLES.fetch_add(1, Ordering::Relaxed) + 1
}

#[cfg(windows)]
pub fn current_process_stats() -> ProcessMemoryStats {
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetGuiResources, GetProcessHandleCount, GR_GDIOBJECTS, GR_USEROBJECTS,
    };

    let uptime_secs = START_TIME
        .get_or_init(Instant::now)
        .elapsed()
        .as_secs();

    unsafe {
        let proc = GetCurrentProcess();
        let mut pmc = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        let ok = K32GetProcessMemoryInfo(
            proc,
            &mut pmc as *mut PROCESS_MEMORY_COUNTERS_EX as *mut _,
            pmc.cb,
        )
        .as_bool();

        let mut handles = 0u32;
        let _ = GetProcessHandleCount(proc, &mut handles);
        let gdi = GetGuiResources(proc, GR_GDIOBJECTS);
        let user = GetGuiResources(proc, GR_USEROBJECTS);

        ProcessMemoryStats {
            working_set_bytes: if ok { pmc.WorkingSetSize as u64 } else { 0 },
            private_bytes: if ok { pmc.PrivateUsage as u64 } else { 0 },
            pagefile_bytes: if ok { pmc.PagefileUsage as u64 } else { 0 },
            gdi_objects: gdi,
            user_objects: user,
            handle_count: handles,
            uptime_secs,
            hibernation_cycles: HIBERNATION_CYCLES.load(Ordering::Relaxed),
        }
    }
}

#[cfg(not(windows))]
pub fn current_process_stats() -> ProcessMemoryStats {
    ProcessMemoryStats {
        uptime_secs: START_TIME.get_or_init(Instant::now).elapsed().as_secs(),
        hibernation_cycles: HIBERNATION_CYCLES.load(Ordering::Relaxed),
        ..Default::default()
    }
}

/// Inspects working set bytes of a child/plugin process by PID.
#[cfg(windows)]
pub fn inspect_pid_working_set(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
            .or_else(|_| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid))
            .ok()?;
        let mut pmc = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        let ok = K32GetProcessMemoryInfo(
            handle,
            &mut pmc as *mut PROCESS_MEMORY_COUNTERS_EX as *mut _,
            pmc.cb,
        )
        .as_bool();
        let _ = CloseHandle(handle);
        ok.then_some(pmc.WorkingSetSize as u64)
    }
}

#[cfg(not(windows))]
pub fn inspect_pid_working_set(_pid: u32) -> Option<u64> {
    None
}

/// Hints Windows to release cold working-set pages during launcher hibernation.
#[cfg(windows)]
pub fn trim_working_set() {
    use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    use windows::Win32::System::Threading::GetCurrentProcess;

    unsafe {
        let _ = K32EmptyWorkingSet(GetCurrentProcess());
    }
}

#[cfg(not(windows))]
pub fn trim_working_set() {}
