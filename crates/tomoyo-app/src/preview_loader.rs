//! Non-blocking background preview loader for bounded disk reads & PNG thumbnails.

#[cfg(windows)]
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use tomoyo_core::{read_file_preview, read_image_thumbnail, Lang, ResolvedPreview};
use tomoyo_protocol::PreviewSpec;
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

#[cfg(windows)]
pub const WM_APP_PREVIEW_READY: u32 = WM_APP + 6;

unsafe extern "C" {
    fn mi_collect(force: bool);
}

pub struct PreviewOutcome {
    pub seq: u64,
    pub item_id: String,
    pub preview: ResolvedPreview,
}

enum PreviewJob {
    Resolve(u64, String, PreviewSpec),
    Purge,
}

type PreviewNotifyCb = Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>;

pub struct AsyncPreviewLoader {
    seq: Arc<AtomicU64>,
    in_flight: Arc<AtomicUsize>,
    #[allow(dead_code)]
    notify_hwnd: Arc<AtomicIsize>,
    #[allow(dead_code)]
    notify_cb: PreviewNotifyCb,
    req_tx: Sender<PreviewJob>,
    res_rx: Receiver<PreviewOutcome>,
}

impl AsyncPreviewLoader {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<PreviewJob>();
        let (res_tx, res_rx) = mpsc::channel::<PreviewOutcome>();
        let seq = Arc::new(AtomicU64::new(1));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let notify_hwnd = Arc::new(AtomicIsize::new(0));
        let notify_cb: PreviewNotifyCb = Arc::new(Mutex::new(None));

        let w_seq = Arc::clone(&seq);
        let w_flight = Arc::clone(&in_flight);
        let w_hwnd = Arc::clone(&notify_hwnd);
        #[cfg(not(windows))]
        let w_cb = Arc::clone(&notify_cb);

        let _ = std::thread::Builder::new()
            .name("tomoyo-preview-loader".into())
            .spawn(move || {
                while let Ok(mut job) = req_rx.recv() {
                    let mut dropped = 0usize;
                    while let Ok(newer) = req_rx.try_recv() {
                        job = newer;
                        dropped += 1;
                    }
                    if dropped > 0 {
                        w_flight.fetch_sub(dropped, Ordering::Release);
                    }
                    let mut ran_heavy_decode = false;
                    match job {
                        PreviewJob::Purge => {
                            ran_heavy_decode = true;
                        }
                        PreviewJob::Resolve(job_seq, item_id, spec) => {
                            if job_seq == w_seq.load(Ordering::Acquire) {
                                let resolved = match spec {
                                    PreviewSpec::FilePreview { path } => {
                                        ran_heavy_decode = true;
                                        Some(read_file_preview(&path))
                                    }
                                    PreviewSpec::ImagePreview { path, .. } => {
                                        ran_heavy_decode = true;
                                        Some(read_image_thumbnail(&path, 280, 200))
                                    }
                                    _ => None,
                                };
                                if let Some(preview) = resolved
                                    && job_seq == w_seq.load(Ordering::Acquire) {
                                        let _ = res_tx.send(PreviewOutcome {
                                            seq: job_seq,
                                            item_id,
                                            preview,
                                        });
                                        #[cfg(windows)]
                                        {
                                            let h = w_hwnd.load(Ordering::Acquire);
                                            if h != 0 {
                                                unsafe {
                                                    let _ = PostMessageW(
                                                        Some(HWND(h as *mut c_void)),
                                                        WM_APP_PREVIEW_READY,
                                                        WPARAM(0),
                                                        LPARAM(0),
                                                    );
                                                }
                                            }
                                        }
                                        #[cfg(not(windows))]
                                        {
                                            let _ = &w_hwnd;
                                            if let Ok(g) = w_cb.lock()
                                                && let Some(cb) = g.as_ref()
                                            {
                                                cb();
                                            }
                                        }
                                    }
                            }
                        }
                    }
                    if ran_heavy_decode {
                        // Release temporary file/PNG buffers from this worker thread's mimalloc heap immediately.
                        unsafe {
                            mi_collect(true);
                        }
                    }
                    w_flight.fetch_sub(1, Ordering::Release);
                }
            });

        Self {
            seq,
            in_flight,
            notify_hwnd,
            notify_cb,
            req_tx,
            res_rx,
        }
    }

    #[allow(dead_code)]
    pub fn set_notify_hwnd(&self, hwnd: isize) {
        self.notify_hwnd.store(hwnd, Ordering::Release);
    }

    #[cfg(not(windows))]
    pub fn set_notify_callback(&self, cb: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut g) = self.notify_cb.lock() {
            *g = Some(Box::new(cb));
        }
    }

    pub fn cancel_all(&self) {
        self.seq.fetch_add(1, Ordering::AcqRel);
        while self.res_rx.try_recv().is_ok() {}
    }

    pub fn purge_worker(&self) {
        self.cancel_all();
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        if self.req_tx.send(PreviewJob::Purge).is_err() {
            self.in_flight.fetch_sub(1, Ordering::Release);
        }
    }

    /// Resolves instant in-memory previews synchronously, or dispatches disk/image decoding.
    pub fn resolve_or_enqueue(
        &self,
        item_id: &str,
        spec: &PreviewSpec,
        lang: Lang,
    ) -> Option<ResolvedPreview> {
        let next_seq = self.seq.fetch_add(1, Ordering::AcqRel) + 1;
        match spec {
            PreviewSpec::Text { title, body } => Some(ResolvedPreview {
                title: title.clone(),
                meta_line: match lang {
                    Lang::Zh => format!(
                        "{} 字符 · {} 行",
                        body.chars().count(),
                        body.lines().count().max(1)
                    ),
                    Lang::En => format!(
                        "{} chars · {} lines",
                        body.chars().count(),
                        body.lines().count().max(1)
                    ),
                },
                badges: Vec::new(),
                body: body.clone(),
                is_monospace: body.contains('{') || body.contains('\n'),
                image_rgba: None,
            }),
            PreviewSpec::KeyValue { title, entries } => {
                let body = entries
                    .iter()
                    .map(|(k, v)| format!("{k:<14} {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(ResolvedPreview {
                    title: title.clone(),
                    meta_line: match lang {
                        Lang::Zh => format!("{} 项属性", entries.len()),
                        Lang::En => format!("{} properties", entries.len()),
                    },
                    badges: Vec::new(),
                    body,
                    is_monospace: true,
                    image_rgba: None,
                })
            }
            PreviewSpec::RichCard { title, subtitle, badges, rows, body } => {
                let mut sections = Vec::new();
                if !rows.is_empty() {
                    sections.push(
                        rows.iter()
                            .map(|(k, v)| format!("{k:<14} {v}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
                if !body.trim().is_empty() {
                    sections.push(body.trim().to_string());
                }
                Some(ResolvedPreview {
                    title: title.clone(),
                    meta_line: if subtitle.is_empty() {
                        match lang {
                            Lang::Zh => format!("富卡片 · {} 项属性", rows.len()),
                            Lang::En => format!("Rich Card · {} fields", rows.len()),
                        }
                    } else {
                        subtitle.clone()
                    },
                    badges: badges.iter().take(5).cloned().collect(),
                    body: sections.join("\n\n"),
                    is_monospace: !rows.is_empty(),
                    image_rgba: None,
                })
            }
            PreviewSpec::FilePreview { .. } | PreviewSpec::ImagePreview { .. } => {
                self.in_flight.fetch_add(1, Ordering::AcqRel);
                if self
                    .req_tx
                    .send(PreviewJob::Resolve(next_seq, item_id.to_string(), spec.clone()))
                    .is_err()
                {
                    self.in_flight.fetch_sub(1, Ordering::Release);
                }
                None
            }
        }
    }

    pub fn drain_latest(&self) -> Option<PreviewOutcome> {
        let cur = self.seq.load(Ordering::Acquire);
        let mut latest = None;
        while let Ok(out) = self.res_rx.try_recv() {
            if out.seq == cur {
                latest = Some(out);
            }
        }
        latest
    }

    pub fn wait_idle_and_drain(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while self.in_flight.load(Ordering::Acquire) > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        while self.res_rx.try_recv().is_ok() {}
    }
}
