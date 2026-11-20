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
                    for dir in dirs.into_iter().take(8) {
                        if !dir.is_dir() {
                            continue;
                        }
                        let wide: Vec<u16> = dir
                            .to_string_lossy()
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        if let Ok(h) = unsafe { FindFirstChangeNotificationW(PCWSTR(wide.as_ptr()), true, flags) } {
                            if !h.is_invalid() {
                                guards.push(ChangeHandle(h));
                            }
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

#[cfg(not(windows))]
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
