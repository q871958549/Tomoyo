//! Bounded non-blocking Hi-DPI Windows Shell RGBA icon cache (zero `HICON` handle retention).
//!
//! Extracts native 64..128 px icons via `IShellItemImageFactory` / `SHGetImageList(SHIL_EXTRALARGE)`
//! on a dedicated COM-STA worker thread, resamples with premultiplied Lanczos-2 to the exact
//! physical pixel grid (`32 DIP * dpi_scale`), destroys all OS handles immediately, and blits 1:1.

use crate::icon_extract::{extract_shell_icon_rgba, DecodedRgba};
use lru::LruCache;
use std::collections::HashSet;
use std::ffi::c_void;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_APP_ICON_READY: u32 = WM_APP + 5;
const MAX_CACHED_ICONS: usize = 96;
const MAX_PENDING_ICONS: usize = 128;

unsafe extern "C" {
    fn mi_collect(force: bool);
}

enum IconJob {
    Extract(u64, String, u32),
    Purge,
}

pub struct IconCache {
    cache: LruCache<String, Option<DecodedRgba>>,
    pending: HashSet<String>,
    epoch: Arc<AtomicU64>,
    in_flight: Arc<AtomicUsize>,
    notify_hwnd: Arc<AtomicIsize>,
    notify_queued: Arc<AtomicBool>,
    req_tx: Sender<IconJob>,
    res_rx: Receiver<(u64, String, Option<DecodedRgba>)>,
}

impl IconCache {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<IconJob>();
        let (res_tx, res_rx) = mpsc::channel::<(u64, String, Option<DecodedRgba>)>();
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
                        IconJob::Extract(job_ep, key, target_px) => {
                            if job_ep == w_epoch.load(Ordering::Acquire) {
                                let (_, path) = key.split_once('|').unwrap_or(("", &key));
                                let icon = extract_shell_icon_rgba(path, target_px);
                                if job_ep == w_epoch.load(Ordering::Acquire)
                                    && res_tx.send((job_ep, key, icon)).is_ok()
                                {
                                    let raw = w_hwnd.load(Ordering::Acquire);
                                    if raw != 0 && !w_queued.swap(true, Ordering::AcqRel) {
                                        unsafe {
                                            let _ = PostMessageW(
                                                Some(HWND(raw as *mut c_void)),
                                                WM_APP_ICON_READY,
                                                WPARAM(0),
                                                LPARAM(0),
                                            );
                                        }
                                    }
                                }
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

    pub fn clear(&mut self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.cache.clear();
        self.pending.clear();
        self.notify_queued.store(false, Ordering::Release);
        while self.res_rx.try_recv().is_ok() {}
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        if self.req_tx.send(IconJob::Purge).is_err() {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
        }
    }

    pub fn get_cached(&mut self, path: &str, target_px: u32) -> Option<&DecodedRgba> {
        let sz = target_px.clamp(16, 128);
        let key = format!("{sz}|{path}");
        self.cache.get(&key).and_then(|o| o.as_ref())
    }

    pub fn get_or_enqueue(&mut self, path: &str, target_px: u32) -> Option<&DecodedRgba> {
        let sz = target_px.clamp(16, 128);
        let key = format!("{sz}|{path}");
        if self.cache.contains(&key) {
            return self.cache.get(&key).and_then(|o| o.as_ref());
        }
        if !self.pending.contains(&key) && self.pending.len() < MAX_PENDING_ICONS {
            let ep = self.epoch.load(Ordering::Acquire);
            self.pending.insert(key.clone());
            self.in_flight.fetch_add(1, Ordering::AcqRel);
            if self.req_tx.send(IconJob::Extract(ep, key.clone(), sz)).is_err() {
                self.in_flight.fetch_sub(1, Ordering::AcqRel);
                self.pending.remove(&key);
            }
        }
        None
    }

    pub fn drain_ready(&mut self) -> bool {
        self.notify_queued.store(false, Ordering::Release);
        let cur_epoch = self.epoch.load(Ordering::Acquire);
        let mut updated = false;
        while let Ok((ep, key, icon)) = self.res_rx.try_recv() {
            self.pending.remove(&key);
            if ep == cur_epoch {
                let has_icon = icon.is_some();
                self.cache.put(key, icon);
                if has_icon {
                    updated = true;
                }
            }
        }
        updated
    }

    pub fn wait_idle_and_drain(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(450);
        while self.in_flight.load(Ordering::Acquire) > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let _ = self.drain_ready();
    }
}
