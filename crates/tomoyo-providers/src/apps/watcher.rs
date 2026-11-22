//! Kernel-blocked (`FindFirstChangeNotificationW`) incremental directory watcher for installed apps.
//!
//! Spends 0% CPU when idle by sleeping inside `WaitForMultipleObjects` on directory handles and a
//! manual-reset stop event, automatically refreshing the in-memory app index whenever shortcuts change.

#[cfg(windows)]
mod win_impl {
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::{
        FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
        FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject, INFINITE,
    };

    struct ChangeHandle(HANDLE);
    impl Drop for ChangeHandle {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = FindCloseChangeNotification(self.0);
                }
            }
        }
    }

    pub struct AppDirWatcher {
        stop_event: HANDLE,
        worker: Option<JoinHandle<()>>,
    }

    // SAFETY: Win32 Event handle is thread-safe and signaled only on Drop.
    unsafe impl Send for AppDirWatcher {}
    unsafe impl Sync for AppDirWatcher {}

    impl AppDirWatcher {
        pub fn start<F>(dirs: Vec<PathBuf>, on_change: F) -> Option<Self>
        where
            F: Fn() + Send + Sync + 'static,
        {
            if dirs.is_empty() {
                return None;
            }
            let stop_event = unsafe { CreateEventW(None, true, false, None).ok()? };
            let stop_raw = stop_event.0 as usize;
            let cb = Arc::new(on_change);
            let worker = std::thread::Builder::new()
                .name("tomoyo-apps-watcher".into())
                .spawn(move || {
                    let stop = HANDLE(stop_raw as *mut c_void);
                    let mut guards: Vec<ChangeHandle> = Vec::new();
                    let flags = FILE_NOTIFY_CHANGE_FILE_NAME
                        | FILE_NOTIFY_CHANGE_LAST_WRITE
                        | FILE_NOTIFY_CHANGE_DIR_NAME;
                    for dir in dirs.into_iter().take(32) {
                        if !dir.is_dir() {
                            continue;
                        }
                        let wide: Vec<u16> = dir
                            .to_string_lossy()
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        if let Ok(h) = unsafe { FindFirstChangeNotificationW(PCWSTR(wide.as_ptr()), true, flags) }
                            && !h.is_invalid() {
                                guards.push(ChangeHandle(h));
                            }
                    }
                    if guards.is_empty() {
                        return;
                    }
                    let mut handles: Vec<HANDLE> = Vec::with_capacity(guards.len() + 1);
                    handles.push(stop);
                    for g in &guards {
                        handles.push(g.0);
                    }
                    loop {
                        let wait = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                        let idx = wait.0.saturating_sub(WAIT_OBJECT_0.0) as usize;
                        if idx == 0 || idx >= handles.len() {
                            break;
                        }
                        // Coalesce burst writes cleanly without busy polling.
                        if unsafe { WaitForSingleObject(stop, 1000) } == WAIT_OBJECT_0 {
                            break;
                        }
                        for g in &guards {
                            unsafe {
                                let _ = FindNextChangeNotification(g.0);
                            }
                        }
                        cb();
                    }
                })
                .ok();

            Some(Self { stop_event, worker })
        }
    }

    impl Drop for AppDirWatcher {
        fn drop(&mut self) {
            if !self.stop_event.is_invalid() {
                unsafe {
                    let _ = SetEvent(self.stop_event);
                }
            }
            if let Some(handle) = self.worker.take() {
                let _ = handle.join();
            }
            if !self.stop_event.is_invalid() {
                unsafe {
                    let _ = CloseHandle(self.stop_event);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod win_impl {
    use std::ffi::{c_char, c_int, c_short, c_void, CString};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread::JoinHandle;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PollFd {
        fd: c_int,
        events: c_short,
        revents: c_short,
    }

    const POLLIN: c_short = 0x0001;
    const IN_NONBLOCK: c_int = 0o0004000;
    const IN_CLOEXEC: c_int = 0o2000000;
    const IN_MODIFY: u32 = 0x0000_0002;
    const IN_MOVED_FROM: u32 = 0x0000_0040;
    const IN_MOVED_TO: u32 = 0x0000_0080;
    const IN_CREATE: u32 = 0x0000_0100;
    const IN_DELETE: u32 = 0x0000_0200;

    unsafe extern "C" {
        fn inotify_init1(flags: c_int) -> c_int;
        fn inotify_add_watch(fd: c_int, pathname: *const c_char, mask: u32) -> c_int;
        fn pipe2(fds: *mut c_int, flags: c_int) -> c_int;
        fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int;
        fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
        fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
        fn close(fd: c_int) -> c_int;
    }

    pub struct AppDirWatcher {
        stop_write_fd: c_int,
        worker: Option<JoinHandle<()>>,
    }

    impl AppDirWatcher {
        pub fn start<F>(dirs: Vec<PathBuf>, on_change: F) -> Option<Self>
        where
            F: Fn() + Send + Sync + 'static,
        {
            if dirs.is_empty() {
                return None;
            }
            let inotify_fd = unsafe { inotify_init1(IN_NONBLOCK | IN_CLOEXEC) };
            if inotify_fd < 0 {
                return None;
            }
            let mask = IN_CREATE | IN_DELETE | IN_MOVED_FROM | IN_MOVED_TO | IN_MODIFY;
            let mut watched = 0usize;
            for dir in dirs.into_iter().take(48) {
                if dir.is_dir()
                    && let Ok(c_dir) = CString::new(dir.to_string_lossy().as_bytes())
                    && unsafe { inotify_add_watch(inotify_fd, c_dir.as_ptr(), mask) } >= 0
                {
                    watched += 1;
                }
            }
            if watched == 0 {
                unsafe { close(inotify_fd); }
                return None;
            }
            let mut pipe_fds = [-1 as c_int; 2];
            if unsafe { pipe2(pipe_fds.as_mut_ptr(), IN_NONBLOCK | IN_CLOEXEC) } != 0 {
                unsafe { close(inotify_fd); }
                return None;
            }
            let (stop_read, stop_write) = (pipe_fds[0], pipe_fds[1]);
            let cb = Arc::new(on_change);
            let worker = std::thread::Builder::new()
                .name("tomoyo-apps-inotify".into())
                .spawn(move || {
                    let mut fds = [
                        PollFd { fd: stop_read, events: POLLIN, revents: 0 },
                        PollFd { fd: inotify_fd, events: POLLIN, revents: 0 },
                    ];
                    let mut buf = [0u8; 4096];
                    loop {
                        fds[0].revents = 0;
                        fds[1].revents = 0;
                        let n = unsafe { poll(fds.as_mut_ptr(), fds.len(), -1) };
                        if n <= 0 || (fds[0].revents & POLLIN) != 0 {
                            break;
                        }
                        if (fds[1].revents & POLLIN) != 0 {
                            let mut stop_poll = [PollFd { fd: stop_read, events: POLLIN, revents: 0 }];
                            let stopped = unsafe { poll(stop_poll.as_mut_ptr(), 1, 800) } > 0;
                            while unsafe { read(inotify_fd, buf.as_mut_ptr().cast::<c_void>(), buf.len()) } > 0 {}
                            if stopped {
                                break;
                            }
                            cb();
                        }
                    }
                    unsafe {
                        close(inotify_fd);
                        close(stop_read);
                    }
                })
                .ok();
            Some(Self { stop_write_fd: stop_write, worker })
        }
    }

    impl Drop for AppDirWatcher {
        fn drop(&mut self) {
            if self.stop_write_fd >= 0 {
                let b: u8 = 1;
                unsafe {
                    let _ = write(self.stop_write_fd, (&b as *const u8).cast::<c_void>(), 1);
                    close(self.stop_write_fd);
                }
                self.stop_write_fd = -1;
            }
            if let Some(h) = self.worker.take() {
                let _ = h.join();
            }
        }
    }
}

#[cfg(all(not(windows), not(target_os = "linux")))]
mod win_impl {
    use std::path::PathBuf;
    pub struct AppDirWatcher;
    impl AppDirWatcher {
        pub fn start<F>(_dirs: Vec<PathBuf>, _on_change: F) -> Option<Self>
        where
            F: Fn() + Send + Sync + 'static,
        {
            None
        }
    }
}

pub use win_impl::AppDirWatcher;
