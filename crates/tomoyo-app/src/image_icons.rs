//! Bounded non-blocking RGBA cache for plugin & custom icon image files (`IconSpec::ImageFile`).

use lru::LruCache;
use std::collections::HashSet;
use std::ffi::c_void;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::icons::WM_APP_ICON_READY;

const MAX_IMAGE_ICONS: usize = 96;
const MAX_PENDING_IMAGE_ICONS: usize = 96;

unsafe extern "C" {
    fn mi_collect(force: bool);
}

pub type DecodedIcon = (u32, u32, Vec<u8>);

enum ImageIconJob {
    Decode(u64, String, u32),
    Purge,
}

pub struct ImageIconCache {
    cache: LruCache<String, Option<DecodedIcon>>,
    pending: HashSet<String>,
    epoch: Arc<AtomicU64>,
    in_flight: Arc<AtomicUsize>,
    notify_hwnd: Arc<AtomicIsize>,
    notify_queued: Arc<AtomicBool>,
    req_tx: Sender<ImageIconJob>,
    res_rx: Receiver<(u64, String, Option<DecodedIcon>)>,
}

impl ImageIconCache {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<ImageIconJob>();
        let (res_tx, res_rx) = mpsc::channel::<(u64, String, Option<DecodedIcon>)>();
        let epoch = Arc::new(AtomicU64::new(1));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let notify_hwnd = Arc::new(AtomicIsize::new(0));
        let notify_queued = Arc::new(AtomicBool::new(false));

        let w_epoch = Arc::clone(&epoch);
        let w_flight = Arc::clone(&in_flight);
        let w_hwnd = Arc::clone(&notify_hwnd);
        let w_queued = Arc::clone(&notify_queued);

        let _ = std::thread::Builder::new()
            .name("tomoyo-img-icon".into())
            .spawn(move || {
                while let Ok(job) = req_rx.recv() {
                    match job {
                        ImageIconJob::Decode(job_ep, key, target_px) => {
                            if job_ep == w_epoch.load(Ordering::Acquire) {
                                let (_, path) = key.split_once('|').unwrap_or(("", &key));
                                let icon = tomoyo_core::wic_thumb::decode_image_thumbnail_wic(
                                    Path::new(path),
                                    target_px,
                                    target_px,
                                )
                                .map(|(_, _, dw, dh, rgba)| (dw, dh, rgba));
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
                        ImageIconJob::Purge => unsafe {
                            windows::Win32::System::Com::CoFreeUnusedLibrariesEx(0, None);
                            mi_collect(true);
                        }
                    }
                    w_flight.fetch_sub(1, Ordering::Release);
                }
            });

        Self {
            cache: LruCache::new(NonZeroUsize::new(MAX_IMAGE_ICONS).unwrap()),
            pending: HashSet::with_capacity(32),
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
        if self.req_tx.send(ImageIconJob::Purge).is_err() {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
        }
    }

    pub fn get_cached(&mut self, path: &str, target_px: u32) -> Option<&DecodedIcon> {
        let key = format!("{target_px}|{path}");
        self.cache.get(&key).and_then(|o| o.as_ref())
    }

    pub fn get_or_enqueue(&mut self, path: &str, target_px: u32) -> Option<&DecodedIcon> {
        let sz = target_px.clamp(16, 128);
        let key = format!("{sz}|{path}");
        if self.cache.contains(&key) {
            return self.cache.get(&key).and_then(|o| o.as_ref());
        }
        if !self.pending.contains(&key) && self.pending.len() < MAX_PENDING_IMAGE_ICONS {
            let ep = self.epoch.load(Ordering::Acquire);
            self.pending.insert(key.clone());
            self.in_flight.fetch_add(1, Ordering::AcqRel);
            if self.req_tx.send(ImageIconJob::Decode(ep, key.clone(), sz)).is_err() {
                self.in_flight.fetch_sub(1, Ordering::AcqRel);
                self.pending.remove(&key);
            }
        }
        None
    }

    pub fn drain_ready(&mut self) -> bool {
        self.notify_queued.store(false, Ordering::Release);
        let cur = self.epoch.load(Ordering::Acquire);
        let mut updated = false;
        while let Ok((ep, key, icon)) = self.res_rx.try_recv() {
            self.pending.remove(&key);
            if ep == cur {
                let ok = icon.is_some();
                self.cache.put(key, icon);
                if ok {
                    updated = true;
                }
            }
        }
        updated
    }

    pub fn wait_idle_and_drain(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while self.in_flight.load(Ordering::Acquire) > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let _ = self.drain_ready();
    }
}
