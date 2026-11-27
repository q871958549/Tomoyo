//! Query dispatch, debounced worker, preview hydration & Cairo/SDF frame assembly.

use crate::linux::ipc::schedule_on_ui;
use crate::linux::shell_state::{with_current_core, AppCore};
use crate::renderer::{
    compute_filter_counts, item_matches_filter, should_show_filter_sidebar, RenderState,
    FOOTER_HEIGHT_DIP, HEADER_HEIGHT_DIP, MAX_WINDOW_WIDTH_DIP, MIN_WINDOW_WIDTH_DIP,
    SIDEBAR_WIDTH_DIP,
};
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tomoyo_core::{Lang, QueryRouter, UpdateStatus};
use tomoyo_protocol::{
    build_visible_filter_tabs_ordered, AttachedContext, ClipboardSnapshot, ResultItem, SearchFilter,
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
            .map(|c| {
                build_visible_filter_tabs_ordered(
                    zh,
                    &c.hidden_builtin_filters,
                    &c.custom_filters,
                    &c.filter_tab_order,
                )
            })
            .unwrap_or_else(|_| build_visible_filter_tabs_ordered(zh, &[], &[], &[]))
    }

    pub(super) fn is_preview_visible(&self) -> bool {
        self.preview_enabled && !self.results.is_empty() && !self.action_menu_open
    }

    pub(super) fn is_filter_bar_visible(&self) -> bool {
        should_show_filter_sidebar(
            self.show_filter_bar,
            self.attached.is_some(),
            &self.current_query,
            self.active_filter,
            self.results.len(),
        )
    }

    pub(super) fn set_active_filter(&mut self, filter: SearchFilter) {
        if self.active_filter == filter {
            return;
        }
        self.active_filter = filter;
        self.selected = 0;
        self.scroll_offset = 0;
        self.hovered = None;
        self.action_menu_open = false;
        self.dispatch_query_inner(true);
    }

    pub(super) fn desired_width_dip(&self) -> i32 {
        self.list_width_dip.clamp(MIN_WINDOW_WIDTH_DIP, MAX_WINDOW_WIDTH_DIP)
    }

    pub(super) fn list_bounds_dip(&self, w_dip: f32) -> (f32, f32) {
        let lx0 = if self.is_filter_bar_visible() { SIDEBAR_WIDTH_DIP } else { 0.0 };
        let avail_w = (w_dip - lx0).max(320.0);
        let list_w = if self.is_preview_visible() && avail_w >= 540.0 {
            let pw = (self.preview_width_dip as f32).clamp(220.0, (avail_w * 0.46).max(220.0));
            (avail_w - pw).max(320.0)
        } else {
            avail_w
        };
        (lx0, lx0 + list_w)
    }

    pub(super) fn desired_height_dip(&self) -> i32 {
        if let Some(h) = self.custom_height_dip {
            return h.clamp(HEADER_HEIGHT_DIP + FOOTER_HEIGHT_DIP, 1080);
        }
        if self.is_filter_bar_visible() {
            return HEADER_HEIGHT_DIP
                + 8
                + (self.max_visible_rows as i32) * self.typography.row_height_dip()
                + FOOTER_HEIGHT_DIP;
        }
        let rows = self.results.len().min(self.max_visible_rows) as i32;
        if rows == 0 {
            HEADER_HEIGHT_DIP + FOOTER_HEIGHT_DIP
        } else {
            let min_preview_rows = if self.is_preview_visible() { 5 } else { 0 };
            let eff_rows = rows.max(min_preview_rows);
            HEADER_HEIGHT_DIP + 8 + eff_rows * self.typography.row_height_dip() + FOOTER_HEIGHT_DIP
        }
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
            self.last_clip_snapshot.clone(),
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
                    clipboard: self.last_clip_snapshot.clone(),
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

    pub(super) fn inject_context_clipboard_card(&mut self) {
        if self.ui.is_null()
            || !self.current_query.trim().is_empty()
            || self.active_filter != SearchFilter::All
            || self.attached.is_some()
        {
            return;
        }
        let Ok(cfg) = self.config.read().map(|c| c.clone()) else {
            return;
        };
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let has_img = crate::actions::clipboard_has_image();
        let mut idx = 0usize;
        if let Some(card) = crate::context_card::build_context_card(
            self.last_clip_snapshot.as_ref(),
            has_img,
            now_unix,
            &cfg,
            self.lang,
        ) && !self.results.iter().any(|r| r.id == card.id)
        {
            self.results.insert(0, card);
            idx = 1;
        }
        if let Some(guide) = crate::context_card::build_onboarding_card(
            &cfg,
            self.router.is_first_run_frecency(),
            self.lang,
        ) && !self.results.iter().any(|r| r.id == guide.id)
        {
            self.results.insert(idx.min(self.results.len()), guide);
        }
        if self.results.len() > 16 {
            self.results.truncate(16);
        }
    }

    pub(super) fn apply_results(&mut self, items: Vec<ResultItem>) {
        self.results = items;
        self.inject_context_clipboard_card();
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
            self.last_clip_snapshot.clone(),
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
        let state = RenderState {
            lang: self.lang,
            theme: self.theme,
            query: &self.current_query,
            hovered_logo: self.hovered_logo,
            mode_badge: badge,
            attached_capsule: cap.as_deref(),
            update_badge: upd_badge.as_deref(),
            filter_bar_visible: self.is_filter_bar_visible(),
            filter_bar_wrap: false,
            filter_tabs: &tabs,
            active_filter: self.active_filter,
            hovered_filter: self.hovered_filter,
            hovered_overflow: false,
            filter_counts: self.filter_counts.as_ref(),
            filter_pill_override: None,
            sidebar_scroll: self.sidebar.scroll,
            hovered_add_tag: self.sidebar.hovered_add_tag,
            sidebar_drag: self.sidebar_drag_visual(),
            typography: &self.typography,
            items: &self.results,
            selected: self.selected,
            hovered: self.hovered,
            launching_row: self.launching_row,
            scroll_offset: self.scroll_offset,
            scrollbar_visual: self.scrollbar_visual(),
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
        let has_preedit = !self.ui.is_null()
            && unsafe { crate::linux::tomoyo_linux_entry_has_preedit(self.ui) != 0 };
        self.renderer.set_placeholder((self.current_query.is_empty() && !has_preedit).then(|| {
            self.lang.pick(
                "搜索应用、文件 (f )、计算、网页 (g/bd )、工作流 (flow )…",
                "Search apps, files (f ), calc, web (g ), flows (flow )…",
            )
        }));
        match cr {
            Some(c) => self.renderer.paint_cairo(c, w, h, &state),
            None => self.renderer.paint_offscreen_for_test(w, h, &state),
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
