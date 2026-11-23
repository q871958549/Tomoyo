//! Query dispatch, debounced worker, preview hydration & Cairo/SDF frame assembly.

use crate::linux::ipc::schedule_on_ui;
use crate::linux::shell_state::{with_current_core, AppCore};
use crate::linux::{tomoyo_linux_add_timeout, tomoyo_linux_remove_source};
use crate::renderer::{
    compute_filter_counts, compute_filter_hitboxes, item_matches_filter, FilterPillAnim,
    RenderState, FILTER_BAR_HEIGHT_DIP, FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP, MAX_WINDOW_WIDTH_DIP,
    MIN_WINDOW_WIDTH_DIP, ROW_HEIGHT_DIP,
};
use std::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tomoyo_core::{Lang, QueryRouter, UpdateStatus};
use tomoyo_protocol::{
    build_visible_filter_tabs, AttachedContext, ClipboardSnapshot, ResultItem, SearchFilter,
    VisibleFilterTab,
};

pub(super) struct QueryJob {
    pub query: String,
    pub generation: u64,
    pub clipboard: Option<ClipboardSnapshot>,
    pub attached: Option<AttachedContext>,
    pub filter: SearchFilter,
    pub immediate: bool,
}

impl AppCore {
    pub(super) fn visible_filter_tabs(&self) -> Vec<VisibleFilterTab> {
        let zh = self.lang == Lang::Zh;
        self.config
            .read()
            .map(|c| build_visible_filter_tabs(zh, &c.hidden_builtin_filters, &c.custom_filters))
            .unwrap_or_else(|_| build_visible_filter_tabs(zh, &[], &[]))
    }

    pub(super) fn is_preview_visible(&self) -> bool {
        self.preview_enabled && !self.results.is_empty() && !self.action_menu_open
    }

    pub(super) fn is_filter_bar_visible(&self) -> bool {
        self.show_filter_bar
            && self.attached.is_none()
            && (!self.results.is_empty() || self.active_filter != SearchFilter::All)
    }

    pub(super) fn cancel_anim_timer(&mut self) {
        if self.anim_source != 0 {
            unsafe { tomoyo_linux_remove_source(self.anim_source); }
            self.anim_source = 0;
        }
    }

    fn current_pill_span(&self, tabs: &[VisibleFilterTab]) -> Option<(f32, f32)> {
        if let Some(a) = &self.pill_anim {
            let (x0, x1, done) = a.span_now();
            if !done {
                return Some((x0, x1));
            }
        }
        compute_filter_hitboxes(
            self.list_width_dip as f32,
            HEADER_HEIGHT_DIP as f32,
            tabs,
            self.filter_counts.as_ref(),
        )
        .into_iter()
        .find(|b| b.filter == self.active_filter)
        .map(|b| (b.x0, b.x1))
    }

    pub(super) fn set_active_filter(&mut self, filter: SearchFilter) {
        if self.active_filter == filter {
            return;
        }
        let tabs = self.visible_filter_tabs();
        let boxes = compute_filter_hitboxes(self.list_width_dip as f32, HEADER_HEIGHT_DIP as f32, &tabs, self.filter_counts.as_ref());
        let from_span = self.current_pill_span(&tabs);
        let to_span = boxes.iter().find(|b| b.filter == filter).map(|b| (b.x0, b.x1));
        if let Some(anim) = FilterPillAnim::from_spans(from_span, to_span) {
            self.pill_anim = Some(anim);
            self.ensure_anim_timer();
        }
        self.active_filter = filter;
        self.selected = 0;
        self.scroll_offset = 0;
        self.hovered = None;
        self.action_menu_open = false;
        self.dispatch_query_inner(true);
    }

    fn ensure_anim_timer(&mut self) {
        self.cancel_anim_timer();
        unsafe extern "C" fn on_anim_tick(_d: *mut c_void) -> c_int {
            let mut cont = 0;
            with_current_core(|st| {
                let done = st.pill_anim.is_none_or(|a| a.span_now().2);
                if done {
                    st.pill_anim = None;
                    st.anim_source = 0;
                } else {
                    cont = 1;
                }
                st.invalidate();
            });
            cont
        }
        self.anim_source = unsafe { tomoyo_linux_add_timeout(16, on_anim_tick, std::ptr::null_mut()) };
    }

    pub(super) fn desired_width_dip(&self) -> i32 {
        let base = self.list_width_dip.clamp(MIN_WINDOW_WIDTH_DIP, 1400);
        if self.is_preview_visible() { (base + self.preview_width_dip).min(MAX_WINDOW_WIDTH_DIP) } else { base }
    }

    pub(super) fn desired_height_dip(&self) -> i32 {
        let n = self.results.len().min(self.max_visible_rows);
        let filter_h = if self.is_filter_bar_visible() { FILTER_BAR_HEIGHT_DIP } else { 0 };
        let body = if n == 0 { 0 } else { 4 + (n as i32) * ROW_HEIGHT_DIP + 4 };
        HEADER_HEIGHT_DIP + filter_h + body + FOOTER_HEIGHT_DIP
    }

    pub(super) fn active_mode_badge(&self) -> Option<&'static str> {
        let q = self.current_query.trim_start();
        if q.starts_with("cb ") || q.eq_ignore_ascii_case("cb") { Some("剪贴板") }
        else if q.starts_with("f ") { Some("文件搜索") }
        else if q.starts_with("tr ") { Some("翻译") }
        else if q.starts_with(':') { Some("系统指令") }
        else { None }
    }

    pub(super) fn attached_capsule_label(&self) -> Option<String> {
        self.attached.as_ref().map(|a| match a {
            AttachedContext::Image { width, height, .. } => format!("🖼 {width}×{height}"),
            AttachedContext::Files { paths } => format!("📎 {} 项文件", paths.len()),
        })
    }

    pub(super) fn dispatch_query(&mut self) {
        self.dispatch_query_inner(false);
    }

    fn dispatch_query_inner(&mut self, immediate: bool) {
        let qgen = self.router.next_generation();
        let batch = self.router.search_fast(
            &self.current_query,
            qgen,
            30,
            self.clipboard.latest_snapshot(),
            self.attached.clone(),
            self.active_filter,
        );
        if immediate && batch.run_slow_ipc && batch.items.is_empty() {
            let tabs = self.visible_filter_tabs();
            let cached: Vec<ResultItem> = if self.cached_all_query == self.current_query {
                self.cached_all_results
                    .iter()
                    .filter(|it| item_matches_filter(it, self.active_filter, &tabs))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            if cached.is_empty() {
                self.results.clear();
                self.selected = 0;
                self.scroll_offset = 0;
                self.cached_preview_id = None;
                self.cached_preview = None;
                self.invalidate();
            } else {
                self.apply_results(cached);
            }
        } else {
            self.apply_results(batch.items);
        }
        if batch.run_slow_ipc {
            self.query_in_flight.fetch_add(1, Ordering::AcqRel);
            if self
                .query_tx
                .send(QueryJob {
                    query: self.current_query.clone(),
                    generation: qgen,
                    clipboard: self.clipboard.latest_snapshot(),
                    attached: self.attached.clone(),
                    filter: self.active_filter,
                    immediate,
                })
                .is_err()
            {
                self.query_in_flight.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }

    pub(super) fn apply_results(&mut self, items: Vec<ResultItem>) {
        self.results = items;
        if self.selected >= self.results.len() {
            self.selected = 0;
        }
        self.scroll_offset = 0;
        let tabs = self.visible_filter_tabs();
        if self.active_filter == SearchFilter::All {
            self.counts_query = self.current_query.clone();
            self.filter_counts = Some(compute_filter_counts(&self.results, &tabs));
            self.cached_all_query = self.current_query.clone();
            self.cached_all_results = self.results.clone();
        } else if self.counts_query != self.current_query {
            self.filter_counts = None;
        }
        self.cached_preview_id = None;
        self.refresh_selected_preview();
        self.resize_to_fit();
        self.invalidate();
    }

    pub(super) fn refresh_selected_preview(&mut self) {
        let Some(item) = self.results.get(self.selected) else {
            self.cached_preview = None;
            return;
        };
        if let Some(pr) = self.preview_loader.drain_latest()
            && pr.item_id == item.id
        {
            self.cached_preview = Some(pr.preview);
            self.cached_preview_id = Some(item.id.clone());
            return;
        }
        if self.cached_preview_id.as_deref() == Some(&item.id) {
            return;
        }
        self.cached_preview = item
            .preview
            .as_ref()
            .and_then(|spec| self.preview_loader.resolve_or_enqueue(&item.id, spec, self.lang));
        self.cached_preview_id = Some(item.id.clone());
    }

    pub(super) fn self_test_paint(&mut self, query: &str) -> usize {
        self.current_query = query.to_string();
        let qgen = self.router.next_generation();
        self.results = self.router.search_with_filter(
            query,
            qgen,
            20,
            self.clipboard.latest_snapshot(),
            self.attached.clone(),
            self.active_filter,
        );
        self.selected = 0;
        let tabs = self.visible_filter_tabs();
        self.filter_counts = (self.active_filter == SearchFilter::All)
            .then(|| compute_filter_counts(&self.results, &tabs));
        self.hovered = (self.results.len() > 1).then_some(1);
        self.cached_preview_id = None;
        self.refresh_selected_preview();
        let count = self.results.len();
        let (w, h) = (self.desired_width_dip(), self.desired_height_dip());
        self.render_internal(None, w, h);
        count
    }

    pub(super) fn render_with_cr(&mut self, cr: *mut c_void, w: i32, h: i32) {
        self.render_internal(Some(cr), w, h);
    }

    fn render_internal(&mut self, cr: Option<*mut c_void>, w: i32, h: i32) {
        let tabs = self.visible_filter_tabs();
        let badge = self.active_mode_badge();
        let cap = self.attached_capsule_label();
        let toast_str = self
            .toast
            .as_ref()
            .filter(|(_, exp)| Instant::now() < *exp)
            .map(|(s, _)| s.as_str());
        let upd_badge = match self.updater.snapshot() {
            UpdateStatus::Available { release, .. } => Some(format!("发现 v{}", release.version)),
            _ => None,
        };
        let pill_override = self.pill_anim.and_then(|a| {
            let (x0, x1, done) = a.span_now();
            (!done).then_some((x0, x1))
        });
        let state = RenderState {
            lang: self.lang,
            theme: self.theme,
            query: &self.current_query,
            hovered_logo: false,
            mode_badge: badge,
            attached_capsule: cap.as_deref(),
            update_badge: upd_badge.as_deref(),
            filter_bar_visible: self.is_filter_bar_visible(),
            filter_tabs: &tabs,
            active_filter: self.active_filter,
            hovered_filter: self.hovered_filter,
            filter_counts: self.filter_counts.as_ref(),
            filter_pill_override: pill_override,
            items: &self.results,
            selected: self.selected,
            hovered: self.hovered,
            launching_row: self.launching_row,
            scroll_offset: self.scroll_offset,
            max_visible_rows: self.max_visible_rows,
            preview_width_dip: self.preview_width_dip,
            action_menu_open: self.action_menu_open,
            selected_action: self.selected_action,
            preview: self.cached_preview.as_ref(),
            everything_status: Some(if self.everything_ok {
                "fd / plocate 就绪"
            } else {
                "未安装 fd/plocate"
            }),
            toast: toast_str,
            dpi_scale: self.dpi_scale,
        };
        match cr {
            Some(c) => self.renderer.paint_cairo(c, w, h, &state),
            None => self.renderer.paint_offscreen_for_test(w, h, &state),
        }
    }

    pub(super) fn toggle_clipboard_launcher(&mut self) {
        let enabled = self
            .config
            .read()
            .map(|c| c.is_plugin_enabled(tomoyo_core::PLUGIN_CLIPBOARD))
            .unwrap_or(true);
        if !enabled {
            return;
        }
        let vis = self.is_visible();
        let q_low = self.current_query.trim_start().to_ascii_lowercase();
        if vis && (q_low.starts_with("cb") || q_low.starts_with("clip")) {
            self.hide_launcher();
        } else if vis {
            self.active_filter = SearchFilter::All;
            self.set_query_text("cb ");
        } else {
            self.active_filter = SearchFilter::All;
            self.show_launcher_with_query(Some("cb "));
        }
    }
}

pub(super) fn spawn_query_worker(
    router: Arc<QueryRouter>,
    rx: Receiver<QueryJob>,
    in_flight: Arc<AtomicUsize>,
) {
    let _ = std::thread::Builder::new().name("tomoyo-query-worker".into()).spawn(move || {
        let mut pending: Option<QueryJob> = None;
        loop {
            let mut job = match pending.take() {
                Some(j) => j,
                None => match rx.recv() {
                    Ok(j) => j,
                    Err(_) => break,
                },
            };
            let mut dropped = 0usize;
            while let Ok(newer) = rx.try_recv() {
                job = newer;
                dropped += 1;
            }
            if dropped > 0 {
                in_flight.fetch_sub(dropped, Ordering::Release);
            }
            if router.is_stale(job.generation) {
                in_flight.fetch_sub(1, Ordering::Release);
                continue;
            }
            if !job.immediate {
                match rx.recv_timeout(Duration::from_millis(25)) {
                    Ok(newer) => {
                        pending = Some(newer);
                        in_flight.fetch_sub(1, Ordering::Release);
                        continue;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        in_flight.fetch_sub(1, Ordering::Release);
                        break;
                    }
                }
            }
            if !router.is_stale(job.generation) {
                let batch = router.search_fast(
                    &job.query,
                    job.generation,
                    30,
                    job.clipboard,
                    job.attached,
                    job.filter,
                );
                if batch.run_slow_ipc {
                    let merged = router.search_slow_and_merge(batch, 30);
                    let qgen = job.generation;
                    schedule_on_ui(move || {
                        with_current_core(|st| {
                            if !st.router.is_stale(qgen) {
                                st.apply_results(merged);
                            }
                        })
                    });
                }
            }
            in_flight.fetch_sub(1, Ordering::Release);
        }
    });
}
