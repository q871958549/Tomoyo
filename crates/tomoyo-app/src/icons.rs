//! Bounded non-blocking async Windows Shell `HICON` cache with RAII `DestroyIcon` cleanup.
//!
//! Never calls `SHGetFileInfoW` on the UI `WM_PAINT` thread, eliminating wake-up freezes
//! after hibernation while guaranteeing deterministic zero-handle-leak cleanup on hibernate.

use lru::LruCache;
use std::collections::HashSet;
use std::ffi::c_void;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, PostMessageW, HICON, WM_APP};

pub const WM_APP_ICON_READY: u32 = WM_APP + 5;
const MAX_CACHED_ICONS: usize = 96;
const MAX_PENDING_ICONS: usize = 128;

unsafe extern "C" {
    fn mi_collect(force: bool);
}

enum IconJob {
    Extract(u64, String),
    Purge,
}

pub struct OwnedIcon(pub HICON);

// SAFETY: Win32 `HICON` handles are process-scoped GDI/USER resources; ownership moves
// strictly one-way from the extraction worker into UI-thread `IconCache` for drawing/drop.
unsafe impl Send for OwnedIcon {}
unsafe impl Sync for OwnedIcon {}

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_invalid() {
                let _ = DestroyIcon(self.0);
            }
        }
    }
}

pub struct IconCache {
    cache: LruCache<String, Option<OwnedIcon>>,
    pending: HashSet<String>,
    epoch: Arc<AtomicU64>,
    in_flight: Arc<AtomicUsize>,
    notify_hwnd: Arc<AtomicIsize>,
    notify_queued: Arc<AtomicBool>,
    req_tx: Sender<IconJob>,
    res_rx: Receiver<(u64, String, Option<OwnedIcon>)>,
}

impl IconCache {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<IconJob>();
        let (res_tx, res_rx) = mpsc::channel::<(u64, String, Option<OwnedIcon>)>();
        let epoch = Arc::new(AtomicU64::new(1));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let notify_hwnd = Arc::new(AtomicIsize::new(0));
        let notify_queued = Arc::new(AtomicBool::new(false));

        let w_epoch = Arc::clone(&epoch);
        let w_flight = Arc::clone(&in_flight);
        let w_hwnd = Arc::clone(&notify_hwnd);
        let w_queued = Arc::clone(&notify_queued);

        let _ = std::thread::Builder::new()
            .name("tomoyo-icon-loader".into())
            .spawn(move || {
                let com_ok = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
                while let Ok(job) = req_rx.recv() {
                    match job {
                        IconJob::Extract(job_ep, path) => {
                            if job_ep == w_epoch.load(Ordering::Acquire) {
                                let icon = extract_shell_icon(&path);
                                if job_ep == w_epoch.load(Ordering::Acquire)
                                    && res_tx.send((job_ep, path, icon)).is_ok() {
                                        let raw_hwnd = w_hwnd.load(Ordering::Acquire);
                                        if raw_hwnd != 0 && !w_queued.swap(true, Ordering::AcqRel) {
                                            unsafe {
                                                let _ = PostMessageW(
                                                    Some(HWND(raw_hwnd as *mut c_void)),
                                                    WM_APP_ICON_READY,
                                                    WPARAM(0),
                                                    LPARAM(0),
                                                );
                                            }
                                        }
                                    }
                                // Stale icons drop deterministically here without hitting `res_rx`.
                            }
                        }
                        IconJob::Purge => unsafe {
                            mi_collect(true);
                        },
                    }
                    w_flight.fetch_sub(1, Ordering::Release);
                }
                if com_ok {
                    unsafe { CoUninitialize() };
                }
            });

        Self {
            cache: LruCache::new(NonZeroUsize::new(MAX_CACHED_ICONS).unwrap()),
            pending: HashSet::with_capacity(64),
            epoch,
            in_flight,
            notify_hwnd,
            notify_queued,
            req_tx,
            res_rx,
        }
    }

    pub fn set_notify_hwnd(&self, hwnd: isize) {
        self.notify_hwnd.store(hwnd, Ordering::Release);
    }

    /// Destroys all cached `HICON` handles & invalidates any in-flight extraction jobs.
    pub fn clear(&mut self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.cache.clear();
        self.pending.clear();
        self.notify_queued.store(false, Ordering::Release);
        while self.res_rx.try_recv().is_ok() {}
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        if self.req_tx.send(IconJob::Purge).is_err() {
            self.in_flight.fetch_sub(1, Ordering::Release);
        }
    }

    /// Returns a cached `HICON` immediately without blocking UI, or enqueues async extraction.
    pub fn get_or_enqueue(&mut self, path: &str) -> Option<HICON> {
        if let Some(cached) = self.cache.get(path) {
            return cached.as_ref().map(|o| o.0);
        }
        if !self.pending.contains(path) && self.pending.len() < MAX_PENDING_ICONS {
            let ep = self.epoch.load(Ordering::Acquire);
            self.pending.insert(path.to_string());
            self.in_flight.fetch_add(1, Ordering::AcqRel);
            if self
                .req_tx
                .send(IconJob::Extract(ep, path.to_string()))
                .is_err()
            {
                self.in_flight.fetch_sub(1, Ordering::Release);
                self.pending.remove(path);
            }
        }
        None
    }

    /// Drains finished background extractions onto the UI LRU cache. Returns `true` if updated.
    pub fn drain_ready(&mut self) -> bool {
        self.notify_queued.store(false, Ordering::Release);
        let cur_epoch = self.epoch.load(Ordering::Acquire);
        let mut updated = false;
        while let Ok((ep, path, icon)) = self.res_rx.try_recv() {
            self.pending.remove(&path);
            if ep == cur_epoch {
                let has_icon = icon.is_some();
                self.cache.put(path, icon);
                if has_icon {
                    updated = true;
                }
            }
        }
        updated
    }

    /// Deterministic wait used only by `--self-test` before checking OS handle counts.
    pub fn wait_idle_and_drain(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while self.in_flight.load(Ordering::Acquire) > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let _ = self.drain_ready();
    }
}

fn extract_shell_icon(path: &str) -> Option<OwnedIcon> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut shfi = SHFILEINFOW::default();
        let res = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if res == 0 || shfi.hIcon.is_invalid() {
            return None;
        }
        Some(OwnedIcon(shfi.hIcon))
    }
}
